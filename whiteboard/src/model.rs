//! WB1: the shared board as an operation-based CRDT with last-writer-wins registers.
//!
//! Every object field, board setting and author binding is a register that holds a value and the
//! [`Stamp`] of the op that wrote it. Merging keeps the highest stamp (ties broken by value), so
//! applying ops or state deltas is commutative and idempotent: replicas that have seen the same
//! ops hold the same state, whatever the order, duplication or loss-and-resend of delivery.
//!
//! Decisions where the contract leaves a gap:
//! - An object's id counter is the seq of the op that creates it, and a Clear's id is its own
//!   (author, seq). The contract compares `id.counter` with a version vector of seqs, which is
//!   only meaningful this way; it also lets the codec leave both ids implicit.
//! - Aggregate limits (5,000 objects, 4 MiB) depend on what a replica already holds, so close to
//!   the limit replicas may accept different ops. Per-op limits (points, text, ranges) are exact.
//!   Local edits are checked before they are emitted, so honest clients never cross a limit.
//! - Who may draw or change settings is not checked here: it depends on host settings that may
//!   arrive in any order. The sync layer filters by policy before calling [`Board::apply`].
//! - Limits the contract does not name: author names and cert keys up to 128 characters, at most
//!   256 allowed-set entries per op, at most 4,096 version-vector entries, coordinates within
//!   ±2^28 quarter pixels, Lamport times up to 2^53, and an op seq at most 10,000 ahead of what
//!   is contiguous for its author (the rest is fetched again later).
//! - A ClearRevoke is accepted only from the author of the Clear; the 30 s window is a UI rule.
//! - An object counts against the 5,000 limit while it is visible; tombstones only count
//!   against the 4 MiB state budget.

use std::collections::{BTreeMap, BTreeSet};

/// Random key that identifies one person's session on one board.
pub type AuthorKey = u32;
/// Version vector: author -> highest contiguous op seq applied.
pub type Vv = BTreeMap<AuthorKey, u32>;

pub const MAX_OBJECTS: usize = 5_000;
pub const MAX_STATE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_POINTS: usize = 2_000;
pub const MAX_TEXT_CHARS: usize = 500;
pub const MAX_ALT_CHARS: usize = 200;
pub const MAX_TITLE_CHARS: usize = 64;
pub const MAX_NAME_CHARS: usize = 128;
pub const MAX_KEY_CHARS: usize = 128;
pub const MAX_ALLOWED_PER_OP: usize = 256;
pub const MAX_VV_ENTRIES: usize = 4_096;
pub const MAX_Z_LEN: usize = 32;
pub const MAX_COORD: i32 = 1 << 28;
pub const MAX_LAMPORT: u64 = 1 << 53;
pub const MAX_SEQ_AHEAD: u32 = 10_000;
pub const INK_COUNT: u8 = 6;
pub const WIDTH_COUNT: u8 = 3;
pub const FILL_COUNT: u8 = 5;
pub const SHAPE_COUNT: u8 = 4;
pub const COLOR_COUNT: u8 = 6;
/// Ramer-Douglas-Peucker tolerance for finished strokes, in pixels.
pub const RDP_TOLERANCE_PX: f64 = 0.5;
const MAX_UNDO: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjId {
    pub author: AuthorKey,
    pub counter: u32,
}

/// Clock of one register write: Lamport time, then author, then the author's op seq. The seq
/// makes stamps unique per op and tells delta sync which op a value came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Stamp {
    pub lamport: u64,
    pub author: AuthorKey,
    pub seq: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Stroke,
    Highlight,
    Rect,
    Ellipse,
    Diamond,
    Line,
    Arrow,
    Note,
    Text,
    /// Reserved for a later version; rejected in 0.7.0.
    Image,
}

impl Kind {
    const ALL: [Kind; 10] = [
        Kind::Stroke,
        Kind::Highlight,
        Kind::Rect,
        Kind::Ellipse,
        Kind::Diamond,
        Kind::Line,
        Kind::Arrow,
        Kind::Note,
        Kind::Text,
        Kind::Image,
    ];

    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Option<Kind> {
        Self::ALL.get(usize::from(v)).copied()
    }

    /// Freehand kinds carry immutable base points.
    pub fn has_points(self) -> bool {
        matches!(self, Kind::Stroke | Kind::Highlight)
    }
}

/// Position and size in quarter pixels. Lines and arrows run from (x, y) to (x + w, y + h).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Geom {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// One object field write.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Field {
    Geom(Geom),
    Ink(u8),
    Width(u8),
    Fill(u8),
    Text(String),
    Z(String),
    Alt(String),
    Deleted(bool),
}

impl Field {
    /// Position in the canonical field order (also the wire tag).
    pub fn tag(&self) -> u8 {
        match self {
            Field::Geom(_) => 0,
            Field::Ink(_) => 1,
            Field::Width(_) => 2,
            Field::Fill(_) => 3,
            Field::Text(_) => 4,
            Field::Z(_) => 5,
            Field::Alt(_) => 6,
            Field::Deleted(_) => 7,
        }
    }
}

/// A set of object field writes; `None` leaves a field untouched.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fields {
    pub geom: Option<Geom>,
    pub ink: Option<u8>,
    pub width: Option<u8>,
    pub fill: Option<u8>,
    pub text: Option<String>,
    pub z: Option<String>,
    pub alt: Option<String>,
    pub deleted: Option<bool>,
}

impl Fields {
    pub fn is_empty(&self) -> bool {
        self.to_list().is_empty()
    }

    /// The present fields in canonical order.
    pub fn to_list(&self) -> Vec<Field> {
        let mut out = Vec::new();
        out.extend(self.geom.map(Field::Geom));
        out.extend(self.ink.map(Field::Ink));
        out.extend(self.width.map(Field::Width));
        out.extend(self.fill.map(Field::Fill));
        out.extend(self.text.clone().map(Field::Text));
        out.extend(self.z.clone().map(Field::Z));
        out.extend(self.alt.clone().map(Field::Alt));
        out.extend(self.deleted.map(Field::Deleted));
        out
    }

    pub fn set(&mut self, f: Field) {
        match f {
            Field::Geom(v) => self.geom = Some(v),
            Field::Ink(v) => self.ink = Some(v),
            Field::Width(v) => self.width = Some(v),
            Field::Fill(v) => self.fill = Some(v),
            Field::Text(v) => self.text = Some(v),
            Field::Z(v) => self.z = Some(v),
            Field::Alt(v) => self.alt = Some(v),
            Field::Deleted(v) => self.deleted = Some(v),
        }
    }

    pub fn from_list(list: impl IntoIterator<Item = Field>) -> Fields {
        let mut out = Fields::default();
        for f in list {
            out.set(f);
        }
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Policy {
    Everyone,
    Selected,
    HostOnly,
}

impl Policy {
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Option<Policy> {
        [Policy::Everyone, Policy::Selected, Policy::HostOnly]
            .get(usize::from(v))
            .copied()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Host {
    pub author: AuthorKey,
}

/// The key of `author` in the allowed list. Not `peer.key`: Mumcord derives that from a secret of
/// each install, so every client sees a different key for the same person. The author key is the
/// same on every client; a person who leaves and joins again is a new author and needs a new "may
/// draw".
pub fn allow_key(author: AuthorKey) -> String {
    format!("{author:08x}")
}

/// One board setting write.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BoardField {
    Title(String),
    Host(Host),
    Policy(Policy),
    /// [`allow_key`] of an author -> may draw (only used with [`Policy::Selected`]).
    Allowed(String, bool),
    LaserForViewers(bool),
    Presenter(Option<AuthorKey>),
}

impl BoardField {
    pub fn tag(&self) -> u8 {
        match self {
            BoardField::Title(_) => 0,
            BoardField::Host(_) => 1,
            BoardField::Policy(_) => 2,
            BoardField::Allowed(..) => 3,
            BoardField::LaserForViewers(_) => 4,
            BoardField::Presenter(_) => 5,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AuthorInfo {
    pub name: String,
    pub shape: u8,
    pub color: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    /// `id` must be (op author, op seq). `base` holds quarter-pixel points local to the original
    /// bounding box for strokes and highlights, and is empty for every other kind.
    Create {
        id: ObjId,
        kind: Kind,
        base: Vec<(i32, i32)>,
        fields: Fields,
    },
    Set {
        id: ObjId,
        fields: Fields,
    },
    /// Deletes every object the clearer had seen: `id.counter <= vv[id.author]`.
    /// `clear_id` must be (op author, op seq).
    Clear {
        clear_id: ObjId,
        vv: Vv,
    },
    ClearRevoke {
        clear_id: ObjId,
    },
    Board {
        fields: Vec<BoardField>,
    },
    /// Binds the op author's key to an identity.
    Author(AuthorInfo),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Op {
    pub author: AuthorKey,
    /// Dense per author, from 1.
    pub seq: u32,
    pub lamport: u64,
    pub body: Body,
}

impl Op {
    pub fn stamp(&self) -> Stamp {
        Stamp {
            lamport: self.lamport,
            author: self.author,
            seq: self.seq,
        }
    }
}

/// One register write as carried by a state delta. Several writes may share an op stamp.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Write {
    /// Kind and base points of an object; its id is (at.author, at.seq).
    Object {
        at: Stamp,
        kind: Kind,
        base: Vec<(i32, i32)>,
    },
    Field {
        id: ObjId,
        at: Stamp,
        field: Field,
    },
    /// A Clear; its id is (at.author, at.seq).
    Clear {
        at: Stamp,
        vv: Vv,
    },
    Revoke {
        clear_id: ObjId,
        at: Stamp,
    },
    Board {
        at: Stamp,
        field: BoardField,
    },
    /// Identity of the author `at.author`.
    Author {
        at: Stamp,
        info: AuthorInfo,
    },
}

impl Write {
    pub fn stamp(&self) -> Stamp {
        match self {
            Write::Object { at, .. }
            | Write::Field { at, .. }
            | Write::Clear { at, .. }
            | Write::Revoke { at, .. }
            | Write::Board { at, .. }
            | Write::Author { at, .. } => *at,
        }
    }
}

/// Why an op or write was not applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
    /// Already applied (not an error for the sender).
    Duplicate,
    /// Too far ahead of the contiguous seq; fetch the gap first.
    SeqTooFar,
    BadClock,
    BadId,
    /// Image objects are reserved for a later version.
    Unsupported,
    BadPoints,
    TooManyPoints,
    TextTooLong,
    AltTooLong,
    TitleTooLong,
    NameTooLong,
    KeyTooLong,
    BadZ,
    /// A colour, width, shape or coordinate out of range.
    BadValue,
    TooManyObjects,
    StateTooLarge,
    TooManyEntries,
    /// Local edit of an object this replica does not know.
    UnknownObject,
    /// Revoking somebody else's Clear.
    NotAllowed,
}

/// A last-writer-wins register.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reg<T> {
    pub val: T,
    pub at: Stamp,
}

/// Writes `val` if it is newer than the current value. Equal stamps only happen for conflicting
/// copies of one op; comparing values then keeps the merge deterministic.
fn put<T: Ord>(slot: &mut Option<Reg<T>>, val: T, at: Stamp) {
    let newer = match slot {
        None => true,
        Some(cur) => (at, &val) > (cur.at, &cur.val),
    };
    if newer {
        *slot = Some(Reg { val, at });
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Object {
    pub id: ObjId,
    /// `None` while only field writes have arrived and the Create has not.
    pub kind: Option<Kind>,
    pub base: Vec<(i32, i32)>,
    pub created: Option<Stamp>,
    pub geom: Option<Reg<Geom>>,
    pub ink: Option<Reg<u8>>,
    pub width: Option<Reg<u8>>,
    pub fill: Option<Reg<u8>>,
    pub text: Option<Reg<String>>,
    pub z: Option<Reg<String>>,
    pub alt: Option<Reg<String>>,
    pub deleted: Option<Reg<bool>>,
}

impl Object {
    fn new(id: ObjId) -> Object {
        Object {
            id,
            kind: None,
            base: Vec::new(),
            created: None,
            geom: None,
            ink: None,
            width: None,
            fill: None,
            text: None,
            z: None,
            alt: None,
            deleted: None,
        }
    }

    fn put(&mut self, field: Field, at: Stamp) {
        match field {
            Field::Geom(v) => put(&mut self.geom, v, at),
            Field::Ink(v) => put(&mut self.ink, v, at),
            Field::Width(v) => put(&mut self.width, v, at),
            Field::Fill(v) => put(&mut self.fill, v, at),
            Field::Text(v) => put(&mut self.text, v, at),
            Field::Z(v) => put(&mut self.z, v, at),
            Field::Alt(v) => put(&mut self.alt, v, at),
            Field::Deleted(v) => put(&mut self.deleted, v, at),
        }
    }

    /// Every present register as a field write, in canonical order.
    pub fn registers(&self) -> Vec<(Field, Stamp)> {
        let mut out = Vec::new();
        out.extend(self.geom.as_ref().map(|r| (Field::Geom(r.val), r.at)));
        out.extend(self.ink.as_ref().map(|r| (Field::Ink(r.val), r.at)));
        out.extend(self.width.as_ref().map(|r| (Field::Width(r.val), r.at)));
        out.extend(self.fill.as_ref().map(|r| (Field::Fill(r.val), r.at)));
        out.extend(
            self.text
                .as_ref()
                .map(|r| (Field::Text(r.val.clone()), r.at)),
        );
        out.extend(self.z.as_ref().map(|r| (Field::Z(r.val.clone()), r.at)));
        out.extend(self.alt.as_ref().map(|r| (Field::Alt(r.val.clone()), r.at)));
        out.extend(self.deleted.as_ref().map(|r| (Field::Deleted(r.val), r.at)));
        out
    }

    /// Current value and stamp of the register `f` refers to (its value is ignored).
    fn register(&self, f: &Field) -> Option<(Field, Stamp)> {
        self.registers()
            .into_iter()
            .find(|(r, _)| r.tag() == f.tag())
    }

    pub fn z_key(&self) -> &str {
        self.z.as_ref().map_or("", |r| r.val.as_str())
    }

    fn approx_size(&self) -> usize {
        let strings = [&self.text, &self.z, &self.alt]
            .iter()
            .map(|r| r.as_ref().map_or(0, |r| r.val.len()))
            .sum::<usize>();
        64 + self.base.len() * 4 + strings
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClearEntry {
    /// `None` while only the revoke has arrived.
    pub at: Option<Stamp>,
    pub vv: Vv,
    pub revoked: Option<Stamp>,
}

impl ClearEntry {
    fn covers(&self, id: ObjId) -> Option<Stamp> {
        let at = self.at?;
        let active = self.revoked.is_none() && covered(&self.vv, id.author, id.counter);
        active.then_some(at)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub title: Option<Reg<String>>,
    pub host: Option<Reg<Host>>,
    pub policy: Option<Reg<Policy>>,
    pub allowed: BTreeMap<String, Reg<bool>>,
    pub laser_for_viewers: Option<Reg<bool>>,
    pub presenter: Option<Reg<Option<AuthorKey>>>,
}

impl Settings {
    fn put(&mut self, field: BoardField, at: Stamp) {
        match field {
            BoardField::Title(v) => put(&mut self.title, v, at),
            BoardField::Host(v) => put(&mut self.host, v, at),
            BoardField::Policy(v) => put(&mut self.policy, v, at),
            BoardField::Allowed(key, v) => {
                let mut slot = self.allowed.remove(&key);
                put(&mut slot, v, at);
                if let Some(reg) = slot {
                    self.allowed.insert(key, reg);
                }
            }
            BoardField::LaserForViewers(v) => put(&mut self.laser_for_viewers, v, at),
            BoardField::Presenter(v) => put(&mut self.presenter, v, at),
        }
    }

    /// Every present register as a board field write, in canonical order.
    pub fn registers(&self) -> Vec<(BoardField, Stamp)> {
        let mut out = Vec::new();
        out.extend(
            self.title
                .as_ref()
                .map(|r| (BoardField::Title(r.val.clone()), r.at)),
        );
        out.extend(
            self.host
                .as_ref()
                .map(|r| (BoardField::Host(r.val.clone()), r.at)),
        );
        out.extend(
            self.policy
                .as_ref()
                .map(|r| (BoardField::Policy(r.val), r.at)),
        );
        for (key, r) in &self.allowed {
            out.push((BoardField::Allowed(key.clone(), r.val), r.at));
        }
        out.extend(
            self.laser_for_viewers
                .as_ref()
                .map(|r| (BoardField::LaserForViewers(r.val), r.at)),
        );
        out.extend(
            self.presenter
                .as_ref()
                .map(|r| (BoardField::Presenter(r.val), r.at)),
        );
        out
    }

    fn approx_size(&self) -> usize {
        self.registers()
            .iter()
            .map(|(f, _)| board_field_size(f))
            .sum()
    }
}

/// A local edit, turned into one op each by [`Board::local`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    Create {
        kind: Kind,
        base: Vec<(i32, i32)>,
        fields: Fields,
    },
    Set {
        id: ObjId,
        fields: Fields,
    },
}

/// Undo record for one edited object: `before` is restored as long as the registers still hold
/// what op `seq` wrote, so an undo never overwrites somebody else's later change.
#[derive(Clone, Debug)]
struct UndoItem {
    id: ObjId,
    seq: u32,
    before: Fields,
    after: Fields,
}

#[derive(Clone, Debug)]
enum Action {
    Edits(Vec<UndoItem>),
    Clear { id: ObjId, vv: Vv },
    Unclear { vv: Vv },
}

/// One replica of a board plus the local person's undo history.
#[derive(Clone, Debug)]
pub struct Board {
    me: AuthorKey,
    seq: u32,
    lamport: u64,
    vv: Vv,
    /// Seqs applied above the contiguous version vector.
    ahead: BTreeMap<AuthorKey, BTreeSet<u32>>,
    objects: BTreeMap<ObjId, Object>,
    clears: BTreeMap<ObjId, ClearEntry>,
    settings: Settings,
    authors: BTreeMap<AuthorKey, Reg<AuthorInfo>>,
    live: usize,
    bytes: usize,
    undo: Vec<Action>,
    redo: Vec<Action>,
}

#[derive(Clone, Copy)]
enum Unit {
    Obj(ObjId),
    Clear(ObjId),
    Author(AuthorKey),
    Settings,
}

fn covered(vv: &Vv, author: AuthorKey, seq: u32) -> bool {
    seq <= vv.get(&author).copied().unwrap_or(0)
}

fn chars_at_most(s: &str, max: usize) -> bool {
    s.chars().nth(max).is_none()
}

fn coord_ok(v: i32) -> bool {
    (-MAX_COORD..=MAX_COORD).contains(&v)
}

fn check_points(kind: Kind, base: &[(i32, i32)]) -> Result<(), Reject> {
    if kind == Kind::Image {
        return Err(Reject::Unsupported);
    }
    if kind.has_points() == base.is_empty() {
        return Err(Reject::BadPoints);
    }
    if base.len() > MAX_POINTS {
        return Err(Reject::TooManyPoints);
    }
    if base.iter().all(|&(x, y)| coord_ok(x) && coord_ok(y)) {
        Ok(())
    } else {
        Err(Reject::BadValue)
    }
}

fn check_key(key: &Option<String>) -> Result<(), Reject> {
    match key {
        Some(k) if !chars_at_most(k, MAX_KEY_CHARS) => Err(Reject::KeyTooLong),
        _ => Ok(()),
    }
}

pub fn check_field(f: &Field) -> Result<(), Reject> {
    let in_range = |v: u8, n: u8| if v < n { Ok(()) } else { Err(Reject::BadValue) };
    match f {
        Field::Geom(g) if [g.x, g.y, g.w, g.h].into_iter().all(coord_ok) => Ok(()),
        Field::Geom(_) => Err(Reject::BadValue),
        Field::Ink(v) => in_range(*v, INK_COUNT),
        Field::Width(v) => in_range(*v, WIDTH_COUNT),
        Field::Fill(v) => in_range(*v, FILL_COUNT),
        Field::Text(s) if !chars_at_most(s, MAX_TEXT_CHARS) => Err(Reject::TextTooLong),
        Field::Alt(s) if !chars_at_most(s, MAX_ALT_CHARS) => Err(Reject::AltTooLong),
        Field::Z(s) if !valid_z(s) => Err(Reject::BadZ),
        _ => Ok(()),
    }
}

fn check_author(info: &AuthorInfo) -> Result<(), Reject> {
    if !chars_at_most(&info.name, MAX_NAME_CHARS) {
        return Err(Reject::NameTooLong);
    }
    if info.shape >= SHAPE_COUNT || info.color >= COLOR_COUNT {
        return Err(Reject::BadValue);
    }
    Ok(())
}

fn check_board_field(f: &BoardField) -> Result<(), Reject> {
    match f {
        BoardField::Title(t) if !chars_at_most(t, MAX_TITLE_CHARS) => Err(Reject::TitleTooLong),
        BoardField::Allowed(k, _) => check_key(&Some(k.clone())),
        _ => Ok(()),
    }
}

/// Validates one write on its own (no aggregate limits).
pub fn check_write(w: &Write) -> Result<(), Reject> {
    let at = w.stamp();
    if at.seq == 0 || at.lamport == 0 || at.lamport > MAX_LAMPORT {
        return Err(Reject::BadClock);
    }
    match w {
        Write::Object { kind, base, .. } => check_points(*kind, base),
        Write::Field { id, field, .. } => {
            if id.counter == 0 {
                return Err(Reject::BadId);
            }
            check_field(field)
        }
        Write::Clear { vv, .. } if vv.len() > MAX_VV_ENTRIES => Err(Reject::TooManyEntries),
        Write::Clear { .. } => Ok(()),
        Write::Revoke { clear_id, at } if clear_id.author != at.author => Err(Reject::NotAllowed),
        Write::Revoke { clear_id, .. } if clear_id.counter == 0 => Err(Reject::BadId),
        Write::Revoke { .. } => Ok(()),
        Write::Board { field, .. } => check_board_field(field),
        Write::Author { info, .. } => check_author(info),
    }
}

/// Splits an op into its register writes (structure checks only).
pub fn op_writes(op: &Op) -> Result<Vec<Write>, Reject> {
    let at = op.stamp();
    let own_id = ObjId {
        author: op.author,
        counter: op.seq,
    };
    let field_writes = |id: ObjId, fields: &Fields| {
        fields
            .to_list()
            .into_iter()
            .map(move |field| Write::Field { id, at, field })
    };
    Ok(match &op.body {
        Body::Create {
            id,
            kind,
            base,
            fields,
        } => {
            if *id != own_id {
                return Err(Reject::BadId);
            }
            let mut out = vec![Write::Object {
                at,
                kind: *kind,
                base: base.clone(),
            }];
            out.extend(field_writes(*id, fields));
            out
        }
        Body::Set { id, fields } => field_writes(*id, fields).collect(),
        Body::Clear { clear_id, vv } => {
            if *clear_id != own_id {
                return Err(Reject::BadId);
            }
            vec![Write::Clear { at, vv: vv.clone() }]
        }
        Body::ClearRevoke { clear_id } => vec![Write::Revoke {
            clear_id: *clear_id,
            at,
        }],
        Body::Board { fields } => {
            if fields.len() > MAX_ALLOWED_PER_OP + 6 {
                return Err(Reject::TooManyEntries);
            }
            fields
                .iter()
                .map(|f| Write::Board {
                    at,
                    field: f.clone(),
                })
                .collect()
        }
        Body::Author(info) => vec![Write::Author {
            at,
            info: info.clone(),
        }],
    })
}

fn board_field_size(f: &BoardField) -> usize {
    16 + match f {
        BoardField::Title(t) => t.len(),
        BoardField::Allowed(k, _) => k.len(),
        _ => 0,
    }
}

/// Upper bound of what a write adds to the approximate state size.
fn write_size(w: &Write) -> usize {
    match w {
        Write::Object { base, .. } => 64 + base.len() * 4,
        Write::Field { field, .. } => match field {
            Field::Text(s) | Field::Z(s) | Field::Alt(s) => s.len(),
            _ => 0,
        },
        Write::Clear { vv, .. } => 32 + vv.len() * 8,
        Write::Revoke { .. } => 32,
        Write::Board { field, .. } => board_field_size(field),
        Write::Author { info, .. } => 32 + info.name.len(),
    }
}

impl Board {
    pub fn new(me: AuthorKey) -> Board {
        Board {
            me,
            seq: 0,
            lamport: 0,
            vv: Vv::new(),
            ahead: BTreeMap::new(),
            objects: BTreeMap::new(),
            clears: BTreeMap::new(),
            settings: Settings::default(),
            authors: BTreeMap::new(),
            live: 0,
            bytes: 0,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn me(&self) -> AuthorKey {
        self.me
    }

    pub fn vv(&self) -> &Vv {
        &self.vv
    }

    /// Ops applied in total as counted by the version vector (for DIGEST).
    pub fn total_ops(&self) -> u32 {
        self.vv.values().fold(0u32, |a, &s| a.saturating_add(s))
    }

    /// Missing seq ranges (author, from, to) below ops that already arrived out of order.
    pub fn gaps(&self) -> Vec<(AuthorKey, u32, u32)> {
        let mut out = Vec::new();
        for (&author, seqs) in &self.ahead {
            let mut next = self.vv.get(&author).copied().unwrap_or(0) + 1;
            for &s in seqs {
                if s > next {
                    out.push((author, next, s - 1));
                }
                next = s + 1;
            }
        }
        out
    }

    pub fn has_applied(&self, author: AuthorKey, seq: u32) -> bool {
        covered(&self.vv, author, seq) || self.ahead.get(&author).is_some_and(|s| s.contains(&seq))
    }

    pub fn object(&self, id: ObjId) -> Option<&Object> {
        self.objects.get(&id)
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn author(&self, key: AuthorKey) -> Option<&AuthorInfo> {
        self.authors.get(&key).map(|r| &r.val)
    }

    pub fn authors(&self) -> impl Iterator<Item = (AuthorKey, &AuthorInfo)> {
        self.authors.iter().map(|(k, r)| (*k, &r.val))
    }

    pub fn clears(&self) -> &BTreeMap<ObjId, ClearEntry> {
        &self.clears
    }

    /// Number of visible objects (counted against [`MAX_OBJECTS`]).
    pub fn live_count(&self) -> usize {
        self.live
    }

    /// Approximate state size (counted against [`MAX_STATE_BYTES`]).
    pub fn approx_bytes(&self) -> usize {
        self.bytes
    }

    /// Effective deleted: the newest of the object's deleted register and every active Clear
    /// that covers it.
    pub fn is_deleted(&self, id: ObjId) -> bool {
        let Some(o) = self.objects.get(&id) else {
            return false;
        };
        let mut best = o.deleted.as_ref().map(|r| (r.at, r.val));
        for c in self.clears.values() {
            if let Some(at) = c.covers(id)
                && best.is_none_or(|(b, _)| at > b)
            {
                best = Some((at, true));
            }
        }
        best.is_some_and(|(_, d)| d)
    }

    fn is_live(&self, id: ObjId) -> bool {
        self.objects.get(&id).is_some_and(|o| o.kind.is_some()) && !self.is_deleted(id)
    }

    /// Visible objects, back to front.
    pub fn visible_objects(&self) -> Vec<&Object> {
        let mut out: Vec<&Object> = self
            .objects
            .values()
            .filter(|o| o.kind.is_some() && !self.is_deleted(o.id))
            .collect();
        out.sort_by(|a, b| a.z_key().cmp(b.z_key()).then(a.id.cmp(&b.id)));
        out
    }

    /// Highest z key in use, to put a new object on top with [`z_after`].
    pub fn top_z(&self) -> Option<&str> {
        self.visible_objects().last().map(|o| o.z_key())
    }

    /// Applies a remote (or replayed) op. Rejections other than `Duplicate`, `SeqTooFar` and a
    /// zero seq still count the op as seen: every replica rejects it alike, and the version
    /// vector must move on.
    pub fn apply(&mut self, op: &Op) -> Result<(), Reject> {
        if op.seq == 0 {
            return Err(Reject::BadClock);
        }
        if self.has_applied(op.author, op.seq) {
            return Err(Reject::Duplicate);
        }
        if op.seq - self.vv.get(&op.author).copied().unwrap_or(0) > MAX_SEQ_AHEAD {
            return Err(Reject::SeqTooFar);
        }
        let prepared = self.prepare(op);
        self.consume(op.author, op.seq);
        for w in prepared? {
            self.merge(w);
        }
        Ok(())
    }

    fn prepare(&self, op: &Op) -> Result<Vec<Write>, Reject> {
        let writes = op_writes(op)?;
        for w in &writes {
            check_write(w)?;
        }
        self.check_room(&writes)?;
        Ok(writes)
    }

    fn check_room(&self, writes: &[Write]) -> Result<(), Reject> {
        let creates = writes
            .iter()
            .filter(|w| match w {
                Write::Object { at, .. } => !self
                    .objects
                    .get(&ObjId {
                        author: at.author,
                        counter: at.seq,
                    })
                    .is_some_and(|o| o.kind.is_some()),
                _ => false,
            })
            .count();
        if creates > 0 && self.live + creates > MAX_OBJECTS {
            return Err(Reject::TooManyObjects);
        }
        let added: usize = writes.iter().map(write_size).sum();
        if self.bytes + added > MAX_STATE_BYTES {
            return Err(Reject::StateTooLarge);
        }
        Ok(())
    }

    fn consume(&mut self, author: AuthorKey, seq: u32) {
        let mut top = self.vv.get(&author).copied().unwrap_or(0);
        if seq != top + 1 {
            self.ahead.entry(author).or_default().insert(seq);
            return;
        }
        top = seq;
        if let Some(set) = self.ahead.get_mut(&author) {
            while set.remove(&(top + 1)) {
                top += 1;
            }
            if set.is_empty() {
                self.ahead.remove(&author);
            }
        }
        self.vv.insert(author, top);
    }

    fn unit_size(&self, unit: Unit) -> usize {
        match unit {
            Unit::Obj(id) => self.objects.get(&id).map_or(0, Object::approx_size),
            Unit::Clear(id) => self.clears.get(&id).map_or(0, |c| 32 + c.vv.len() * 8),
            Unit::Author(k) => self.authors.get(&k).map_or(0, |r| 32 + r.val.name.len()),
            Unit::Settings => self.settings.approx_size(),
        }
    }

    fn count_live(&self) -> usize {
        self.objects.keys().filter(|id| self.is_live(**id)).count()
    }

    /// Merges one validated write and keeps the live count and size estimate up to date.
    fn merge(&mut self, w: Write) {
        let at = w.stamp();
        self.lamport = self.lamport.max(at.lamport);
        let unit = match &w {
            Write::Object { at, .. } => Unit::Obj(ObjId {
                author: at.author,
                counter: at.seq,
            }),
            Write::Field { id, .. } => Unit::Obj(*id),
            Write::Clear { at, .. } => Unit::Clear(ObjId {
                author: at.author,
                counter: at.seq,
            }),
            Write::Revoke { clear_id, .. } => Unit::Clear(*clear_id),
            Write::Author { at, .. } => Unit::Author(at.author),
            Write::Board { .. } => Unit::Settings,
        };
        let size_before = self.unit_size(unit);
        let live_before = matches!(unit, Unit::Obj(id) if self.is_live(id));
        match w {
            Write::Object { at, kind, base } => {
                let id = ObjId {
                    author: at.author,
                    counter: at.seq,
                };
                let o = self.objects.entry(id).or_insert_with(|| Object::new(id));
                if o.kind.is_none() {
                    o.kind = Some(kind);
                    o.base = base;
                    o.created = Some(at);
                }
            }
            Write::Field { id, at, field } => {
                self.objects
                    .entry(id)
                    .or_insert_with(|| Object::new(id))
                    .put(field, at);
            }
            Write::Clear { at, vv } => {
                let id = ObjId {
                    author: at.author,
                    counter: at.seq,
                };
                let c = self.clears.entry(id).or_default();
                if c.at.is_none() {
                    c.at = Some(at);
                    c.vv = vv;
                }
            }
            Write::Revoke { clear_id, at } => {
                let c = self.clears.entry(clear_id).or_default();
                c.revoked = c.revoked.max(Some(at));
            }
            Write::Board { at, field } => self.settings.put(field, at),
            Write::Author { at, info } => {
                let mut slot = self.authors.remove(&at.author);
                put(&mut slot, info, at);
                if let Some(reg) = slot {
                    self.authors.insert(at.author, reg);
                }
            }
        }
        self.bytes = self.bytes - size_before + self.unit_size(unit);
        match unit {
            Unit::Obj(id) => {
                let live_after = self.is_live(id);
                self.live = self.live + usize::from(live_after) - usize::from(live_before);
            }
            Unit::Clear(_) => self.live = self.count_live(),
            _ => {}
        }
    }

    /// Every register whose origin op is not covered by `vv`: what a peer at `vv` is missing.
    pub fn delta_since(&self, vv: &Vv) -> Vec<Write> {
        let fresh = |at: &Stamp| !covered(vv, at.author, at.seq);
        let mut out = Vec::new();
        for o in self.objects.values() {
            if let (Some(kind), Some(at)) = (o.kind, o.created)
                && fresh(&at)
            {
                out.push(Write::Object {
                    at,
                    kind,
                    base: o.base.clone(),
                });
            }
            for (field, at) in o.registers() {
                if fresh(&at) {
                    out.push(Write::Field {
                        id: o.id,
                        at,
                        field,
                    });
                }
            }
        }
        for (id, c) in &self.clears {
            if let Some(at) = c.at.filter(fresh) {
                out.push(Write::Clear {
                    at,
                    vv: c.vv.clone(),
                });
            }
            if let Some(at) = c.revoked.filter(fresh) {
                out.push(Write::Revoke { clear_id: *id, at });
            }
        }
        for (field, at) in self.settings.registers() {
            if fresh(&at) {
                out.push(Write::Board { at, field });
            }
        }
        for r in self.authors.values() {
            if fresh(&r.at) {
                out.push(Write::Author {
                    at: r.at,
                    info: r.val.clone(),
                });
            }
        }
        out
    }

    /// Merges a state delta from a peer. Call only with a complete delta: afterwards this replica
    /// counts every op in `sender_vv` as seen. Returns how many writes were rejected.
    pub fn merge_delta(&mut self, writes: Vec<Write>, sender_vv: &Vv) -> usize {
        let mut rejected = 0;
        for w in writes {
            let ok = check_write(&w).and_then(|()| self.check_room(std::slice::from_ref(&w)));
            if ok.is_ok() {
                self.merge(w);
            } else {
                rejected += 1;
            }
        }
        for (&author, &seq) in sender_vv {
            if seq > self.vv.get(&author).copied().unwrap_or(0) {
                if let Some(set) = self.ahead.get_mut(&author) {
                    set.retain(|&s| s > seq);
                }
                // Consuming `seq` again pulls in any contiguous seqs from `ahead`.
                self.vv.insert(author, seq - 1);
                self.consume(author, seq);
            }
        }
        rejected
    }

    /// Deterministic hash of every register (values and stamps), clear and author binding.
    /// Equal on all replicas that have merged the same ops.
    pub fn state_hash(&self) -> u64 {
        let mut h = Fnv::new();
        h.u64(self.objects.len() as u64);
        for o in self.objects.values() {
            h.u32(o.id.author);
            h.u32(o.id.counter);
            match (o.kind, o.created) {
                (Some(kind), Some(at)) => {
                    h.u8(1);
                    h.u8(kind.to_u8());
                    h.stamp(at);
                }
                _ => h.u8(0),
            }
            h.u64(o.base.len() as u64);
            for &(x, y) in &o.base {
                h.i32(x);
                h.i32(y);
            }
            let regs = o.registers();
            h.u64(regs.len() as u64);
            for (field, at) in regs {
                h.field(&field);
                h.stamp(at);
            }
        }
        h.u64(self.clears.len() as u64);
        for (id, c) in &self.clears {
            h.u32(id.author);
            h.u32(id.counter);
            h.opt_stamp(c.at);
            h.vv(&c.vv);
            h.opt_stamp(c.revoked);
        }
        let regs = self.settings.registers();
        h.u64(regs.len() as u64);
        for (field, at) in regs {
            h.board_field(&field);
            h.stamp(at);
        }
        h.u64(self.authors.len() as u64);
        for (k, r) in &self.authors {
            h.u32(*k);
            h.author(&r.val);
            h.stamp(r.at);
        }
        h.0
    }

    // ---- local edits -------------------------------------------------------------------------

    fn next_op(&self, offset: u32, body: Body) -> Op {
        Op {
            author: self.me,
            seq: self.seq + 1 + offset,
            lamport: self.lamport + 1 + u64::from(offset),
            body,
        }
    }

    fn commit(&mut self, op: &Op, writes: Vec<Write>) {
        self.consume(op.author, op.seq);
        self.seq = op.seq;
        for w in writes {
            self.merge(w);
        }
    }

    fn emit(&mut self, body: Body) -> Result<Op, Reject> {
        let op = self.next_op(0, body);
        let writes = self.prepare(&op)?;
        self.commit(&op, writes);
        Ok(op)
    }

    fn push_undo(&mut self, action: Action) {
        self.undo.push(action);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Current values of the registers `fields` names; a missing `deleted` counts as false.
    fn current(&self, id: ObjId, fields: &Fields) -> Fields {
        let mut out = Fields::default();
        if let Some(o) = self.objects.get(&id) {
            for f in fields.to_list() {
                if let Some((cur, _)) = o.register(&f) {
                    out.set(cur);
                } else if matches!(f, Field::Deleted(_)) {
                    out.set(Field::Deleted(false));
                }
            }
        }
        out
    }

    /// Applies local edits as one undoable action. All edits are checked first, so either every
    /// edit becomes an op or none does.
    pub fn local(&mut self, edits: Vec<Edit>) -> Result<Vec<Op>, Reject> {
        let mut ops = Vec::with_capacity(edits.len());
        for (i, edit) in edits.into_iter().enumerate() {
            let offset = u32::try_from(i).map_err(|_| Reject::TooManyEntries)?;
            let body = match edit {
                Edit::Create { kind, base, fields } => Body::Create {
                    id: ObjId {
                        author: self.me,
                        counter: self.seq + 1 + offset,
                    },
                    kind,
                    base,
                    fields,
                },
                Edit::Set { id, fields } => {
                    if !self.objects.get(&id).is_some_and(|o| o.kind.is_some()) {
                        return Err(Reject::UnknownObject);
                    }
                    Body::Set { id, fields }
                }
            };
            ops.push(self.next_op(offset, body));
        }
        let mut all = Vec::new();
        let mut per_op = Vec::with_capacity(ops.len());
        for op in &ops {
            let writes = op_writes(op)?;
            for w in &writes {
                check_write(w)?;
            }
            all.extend(writes.iter().cloned());
            per_op.push(writes);
        }
        self.check_room(&all)?;
        let mut items = Vec::with_capacity(ops.len());
        for (op, writes) in ops.iter().zip(per_op) {
            let item = match &op.body {
                Body::Create { id, .. } => UndoItem {
                    id: *id,
                    seq: op.seq,
                    before: Fields {
                        deleted: Some(true),
                        ..Fields::default()
                    },
                    after: Fields {
                        deleted: Some(false),
                        ..Fields::default()
                    },
                },
                Body::Set { id, fields } => UndoItem {
                    id: *id,
                    seq: op.seq,
                    before: self.current(*id, fields),
                    after: fields.clone(),
                },
                _ => continue,
            };
            items.push(item);
            self.commit(op, writes);
        }
        self.push_undo(Action::Edits(items));
        Ok(ops)
    }

    /// Clears everything this replica has seen (undoable while it is on the undo stack).
    pub fn local_clear(&mut self) -> Result<Op, Reject> {
        let vv = self.vv.clone();
        let op = self.emit(Body::Clear {
            clear_id: ObjId {
                author: self.me,
                counter: self.seq + 1,
            },
            vv: vv.clone(),
        })?;
        self.push_undo(Action::Clear {
            id: ObjId {
                author: op.author,
                counter: op.seq,
            },
            vv,
        });
        Ok(op)
    }

    pub fn local_board(&mut self, fields: Vec<BoardField>) -> Result<Op, Reject> {
        self.emit(Body::Board { fields })
    }

    pub fn local_author(&mut self, info: AuthorInfo) -> Result<Op, Reject> {
        self.emit(Body::Author(info))
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Reverts the newest own action that still has an effect. Returns the ops to send.
    pub fn undo(&mut self) -> Option<Vec<Op>> {
        while let Some(action) = self.undo.pop() {
            if let Some((ops, inverse)) = self.revert(action) {
                self.redo.push(inverse);
                return Some(ops);
            }
        }
        None
    }

    pub fn redo(&mut self) -> Option<Vec<Op>> {
        while let Some(action) = self.redo.pop() {
            if let Some((ops, inverse)) = self.revert(action) {
                self.undo.push(inverse);
                return Some(ops);
            }
        }
        None
    }

    /// Performs the inverse of `action` and returns its ops plus the action that undoes it.
    fn revert(&mut self, action: Action) -> Option<(Vec<Op>, Action)> {
        match action {
            Action::Edits(items) => {
                let mut ops = Vec::new();
                let mut inverse = Vec::new();
                for item in items.into_iter().rev() {
                    let Some(o) = self.objects.get(&item.id) else {
                        continue;
                    };
                    let restore: Vec<Field> = item
                        .before
                        .to_list()
                        .into_iter()
                        .filter(|f| {
                            o.register(f)
                                .is_none_or(|(_, at)| at.author == self.me && at.seq == item.seq)
                        })
                        .collect();
                    if restore.is_empty() {
                        continue;
                    }
                    let tags: Vec<u8> = restore.iter().map(Field::tag).collect();
                    let redo = Fields::from_list(
                        item.after
                            .to_list()
                            .into_iter()
                            .filter(|f| tags.contains(&f.tag())),
                    );
                    let fields = Fields::from_list(restore);
                    if let Ok(op) = self.emit(Body::Set {
                        id: item.id,
                        fields: fields.clone(),
                    }) {
                        inverse.push(UndoItem {
                            id: item.id,
                            seq: op.seq,
                            before: redo,
                            after: fields,
                        });
                        ops.push(op);
                    }
                }
                (!ops.is_empty()).then_some((ops, Action::Edits(inverse)))
            }
            Action::Clear { id, vv } => {
                if self.clears.get(&id).is_some_and(|c| c.revoked.is_some()) {
                    return None;
                }
                let op = self.emit(Body::ClearRevoke { clear_id: id }).ok()?;
                Some((vec![op], Action::Unclear { vv }))
            }
            Action::Unclear { vv } => {
                let id = ObjId {
                    author: self.me,
                    counter: self.seq + 1,
                };
                let op = self
                    .emit(Body::Clear {
                        clear_id: id,
                        vv: vv.clone(),
                    })
                    .ok()?;
                Some((vec![op], Action::Clear { id, vv }))
            }
        }
    }
}

/// Hash of a version vector, for DIGEST messages.
pub fn vv_hash(vv: &Vv) -> u64 {
    let mut h = Fnv::new();
    h.vv(vv);
    h.0
}

/// FNV-1a: tiny, stable across builds and platforms (std's hasher is not guaranteed to be).
struct Fnv(u64);

impl Fnv {
    fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, b: &[u8]) {
        for &x in b {
            self.0 ^= u64::from(x);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn u8(&mut self, v: u8) {
        self.bytes(&[v]);
    }

    fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }

    fn i32(&mut self, v: i32) {
        self.bytes(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }

    fn str(&mut self, s: &str) {
        self.u64(s.len() as u64);
        self.bytes(s.as_bytes());
    }

    fn stamp(&mut self, s: Stamp) {
        self.u64(s.lamport);
        self.u32(s.author);
        self.u32(s.seq);
    }

    fn opt_stamp(&mut self, s: Option<Stamp>) {
        match s {
            Some(s) => {
                self.u8(1);
                self.stamp(s);
            }
            None => self.u8(0),
        }
    }

    fn vv(&mut self, vv: &Vv) {
        self.u64(vv.len() as u64);
        for (a, s) in vv {
            self.u32(*a);
            self.u32(*s);
        }
    }

    fn field(&mut self, f: &Field) {
        self.u8(f.tag());
        match f {
            Field::Geom(g) => {
                for v in [g.x, g.y, g.w, g.h] {
                    self.i32(v);
                }
            }
            Field::Ink(v) | Field::Width(v) | Field::Fill(v) => self.u8(*v),
            Field::Text(s) | Field::Z(s) | Field::Alt(s) => self.str(s),
            Field::Deleted(b) => self.u8(u8::from(*b)),
        }
    }

    fn board_field(&mut self, f: &BoardField) {
        self.u8(f.tag());
        match f {
            BoardField::Title(t) => self.str(t),
            BoardField::Host(h) => self.u32(h.author),
            BoardField::Policy(p) => self.u8(p.to_u8()),
            BoardField::Allowed(k, v) => {
                self.str(k);
                self.u8(u8::from(*v));
            }
            BoardField::LaserForViewers(v) => self.u8(u8::from(*v)),
            BoardField::Presenter(p) => match p {
                Some(a) => {
                    self.u8(1);
                    self.u32(*a);
                }
                None => self.u8(0),
            },
        }
    }

    fn author(&mut self, a: &AuthorInfo) {
        self.str(&a.name);
        self.u8(a.shape);
        self.u8(a.color);
    }
}

// ---- z order -----------------------------------------------------------------------------------

/// A fractional index: 1 to 32 bytes from `a` to `z`.
pub fn valid_z(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_Z_LEN && s.bytes().all(|b| b.is_ascii_lowercase())
}

/// Base-26 midpoint of digit strings; `b = None` means "after everything".
/// Precondition: a < b, and b does not end with digit 0.
fn midpoint(a: &[u8], b: Option<&[u8]>) -> Vec<u8> {
    if let Some(b) = b {
        let mut n = 0;
        while n < b.len() && a.get(n).copied().unwrap_or(0) == b[n] {
            n += 1;
        }
        if n > 0 {
            let mut out = b[..n].to_vec();
            out.extend(midpoint(a.get(n..).unwrap_or(&[]), Some(&b[n..])));
            return out;
        }
    }
    let da = u16::from(a.first().copied().unwrap_or(0));
    let db = b.and_then(|b| b.first()).map_or(26, |&d| u16::from(d));
    if db > da + 1 {
        return vec![((da + db) / 2) as u8];
    }
    if let Some(b) = b
        && b.len() > 1
    {
        return vec![b[0]];
    }
    let mut out = vec![da as u8];
    out.extend(midpoint(a.get(1..).unwrap_or(&[]), None));
    out
}

/// A z key strictly between `lo` and `hi` (`None` = open end). `None` if there is no room
/// within 32 bytes or the bounds are invalid; then the caller re-spreads with [`z_spread`].
pub fn z_between(lo: Option<&str>, hi: Option<&str>) -> Option<String> {
    let lo = lo.unwrap_or("");
    if (!lo.is_empty() && !valid_z(lo)) || hi.is_some_and(|h| !valid_z(h) || h.ends_with('a')) {
        return None;
    }
    if hi.is_some_and(|h| lo >= h) {
        return None;
    }
    let digits = |s: &str| s.bytes().map(|b| b - b'a').collect::<Vec<u8>>();
    let hi_digits = hi.map(digits);
    let mid = midpoint(&digits(lo), hi_digits.as_deref());
    let out: String = mid.into_iter().map(|d| char::from(b'a' + d)).collect();
    let ordered = out.as_str() > lo && hi.is_none_or(|h| out.as_str() < h);
    (ordered && valid_z(&out)).then_some(out)
}

pub fn z_before(first: &str) -> Option<String> {
    z_between(None, Some(first))
}

pub fn z_after(last: &str) -> Option<String> {
    z_between(Some(last), None)
}

/// `n` evenly spread keys in ascending order, for re-indexing when keys get too long.
pub fn z_spread(n: usize) -> Vec<String> {
    let mut width = 1;
    while 26usize.pow(width) <= n + 1 {
        width += 1;
    }
    let total = 26u64.pow(width);
    (1..=n as u64)
        .map(|i| {
            let mut v = i * total / (n as u64 + 1);
            let mut s = vec![b'a'; width as usize];
            for slot in s.iter_mut().rev() {
                *slot = b'a' + (v % 26) as u8;
                v /= 26;
            }
            while s.len() > 1 && s.last() == Some(&b'a') {
                s.pop();
            }
            String::from_utf8_lossy(&s).into_owned()
        })
        .collect()
}

// ---- strokes -----------------------------------------------------------------------------------

fn seg_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (cx, cy) = (a.0 + t * dx, a.1 + t * dy);
    ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()
}

/// Ramer-Douglas-Peucker simplification (iterative, so long strokes cannot overflow the stack).
pub fn simplify(points: &[(f64, f64)], tolerance: f64) -> Vec<(f64, f64)> {
    let n = points.len();
    if n < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut stack = vec![(0, n - 1)];
    while let Some((a, b)) = stack.pop() {
        let mut best = (0.0, 0);
        for (i, &p) in points.iter().enumerate().take(b).skip(a + 1) {
            let d = seg_dist(p, points[a], points[b]);
            if d > best.0 {
                best = (d, i);
            }
        }
        if best.0 > tolerance {
            keep[best.1] = true;
            stack.push((a, best.1));
            stack.push((best.1, b));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect()
}

/// Pixels to quarter pixels. Clamped to half the coordinate range so a bounding box size still
/// fits the range.
pub fn quantize(points: &[(f64, f64)]) -> Vec<(i32, i32)> {
    let lim = f64::from(MAX_COORD / 2);
    let q = |v: f64| (v * 4.0).round().clamp(-lim, lim) as i32;
    points.iter().map(|&(x, y)| (q(x), q(y))).collect()
}

/// Turns a finished pen path (pixels) into the geometry and base points of a stroke op:
/// simplified with [`RDP_TOLERANCE_PX`], quantised, de-duplicated and made local to its bounding
/// box (the geometry).
pub fn prepare_stroke(points: &[(f64, f64)]) -> Result<(Geom, Vec<(i32, i32)>), Reject> {
    let finite: Vec<(f64, f64)> = points
        .iter()
        .copied()
        .filter(|(x, y)| x.is_finite() && y.is_finite())
        .collect();
    let mut q = quantize(&simplify(&finite, RDP_TOLERANCE_PX));
    q.dedup();
    if q.is_empty() {
        return Err(Reject::BadPoints);
    }
    if q.len() > MAX_POINTS {
        return Err(Reject::TooManyPoints);
    }
    let min_x = q.iter().map(|p| p.0).min().unwrap_or(0);
    let min_y = q.iter().map(|p| p.1).min().unwrap_or(0);
    let max_x = q.iter().map(|p| p.0).max().unwrap_or(0);
    let max_y = q.iter().map(|p| p.1).max().unwrap_or(0);
    let base = q.iter().map(|&(x, y)| (x - min_x, y - min_y)).collect();
    let geom = Geom {
        x: min_x,
        y: min_y,
        w: max_x - min_x,
        h: max_y - min_y,
    };
    Ok((geom, base))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn stroke_edit(x: i32, z: &str) -> Edit {
        Edit::Create {
            kind: Kind::Stroke,
            base: vec![(0, 0), (4, 8)],
            fields: Fields {
                geom: Some(Geom {
                    x,
                    y: 0,
                    w: 4,
                    h: 8,
                }),
                ink: Some(1),
                z: Some(z.to_string()),
                ..Fields::default()
            },
        }
    }

    fn note_edit(text: &str) -> Edit {
        Edit::Create {
            kind: Kind::Note,
            base: vec![],
            fields: Fields {
                geom: Some(Geom::default()),
                text: Some(text.into()),
                fill: Some(2),
                z: Some("n".into()),
                ..Fields::default()
            },
        }
    }

    fn created_id(ops: &[Op]) -> ObjId {
        match &ops[0].body {
            Body::Create { id, .. } => *id,
            _ => panic!("not a create"),
        }
    }

    fn sync(from: &Board, to: &mut Board, ops: &[Op]) {
        for op in ops {
            let _ = to.apply(op);
        }
        let _ = from;
    }

    #[test]
    fn create_set_and_lww() {
        let mut a = Board::new(1);
        let mut b = Board::new(2);
        let ops = a.local(vec![note_edit("hi")]).unwrap();
        let id = created_id(&ops);
        sync(&a, &mut b, &ops);
        let set_a = a
            .local(vec![Edit::Set {
                id,
                fields: Fields {
                    text: Some("from a".into()),
                    ..Fields::default()
                },
            }])
            .unwrap();
        let set_b = b
            .local(vec![Edit::Set {
                id,
                fields: Fields {
                    text: Some("from b".into()),
                    ..Fields::default()
                },
            }])
            .unwrap();
        sync(&a, &mut b, &set_a);
        sync(&b, &mut a, &set_b);
        // Same lamport, author 2 wins the tie.
        assert_eq!(a.object(id).unwrap().text.as_ref().unwrap().val, "from b");
        assert_eq!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn every_field_round_trips_through_set() {
        let mut a = Board::new(1);
        let id = created_id(&a.local(vec![note_edit("x")]).unwrap());
        let fields = Fields {
            geom: Some(Geom {
                x: -5,
                y: 6,
                w: 7,
                h: -8,
            }),
            ink: Some(5),
            width: Some(2),
            fill: Some(4),
            text: Some("t".into()),
            z: Some("q".into()),
            alt: Some("alt".into()),
            deleted: Some(true),
        };
        a.local(vec![Edit::Set {
            id,
            fields: fields.clone(),
        }])
        .unwrap();
        let o = a.object(id).unwrap();
        let got = Fields::from_list(o.registers().into_iter().map(|(f, _)| f));
        assert_eq!(got, fields);
        assert!(a.is_deleted(id));
        assert!(a.visible_objects().is_empty());
    }

    #[test]
    fn rejects_bad_values() {
        let mut a = Board::new(1);
        let bad = |f: Fields| Edit::Create {
            kind: Kind::Rect,
            base: vec![],
            fields: f,
        };
        let cases = [
            (
                Fields {
                    ink: Some(INK_COUNT),
                    ..Fields::default()
                },
                Reject::BadValue,
            ),
            (
                Fields {
                    width: Some(WIDTH_COUNT),
                    ..Fields::default()
                },
                Reject::BadValue,
            ),
            (
                Fields {
                    fill: Some(FILL_COUNT),
                    ..Fields::default()
                },
                Reject::BadValue,
            ),
            (
                Fields {
                    text: Some("x".repeat(MAX_TEXT_CHARS + 1)),
                    ..Fields::default()
                },
                Reject::TextTooLong,
            ),
            (
                Fields {
                    alt: Some("x".repeat(MAX_ALT_CHARS + 1)),
                    ..Fields::default()
                },
                Reject::AltTooLong,
            ),
            (
                Fields {
                    z: Some("A".into()),
                    ..Fields::default()
                },
                Reject::BadZ,
            ),
            (
                Fields {
                    geom: Some(Geom {
                        x: MAX_COORD + 1,
                        ..Geom::default()
                    }),
                    ..Fields::default()
                },
                Reject::BadValue,
            ),
        ];
        for (fields, want) in cases {
            assert_eq!(a.local(vec![bad(fields)]), Err(want));
        }
        let img = Edit::Create {
            kind: Kind::Image,
            base: vec![],
            fields: Fields::default(),
        };
        assert_eq!(a.local(vec![img]), Err(Reject::Unsupported));
        let no_points = Edit::Create {
            kind: Kind::Stroke,
            base: vec![],
            fields: Fields::default(),
        };
        assert_eq!(a.local(vec![no_points]), Err(Reject::BadPoints));
        let too_many = Edit::Create {
            kind: Kind::Stroke,
            base: vec![(0, 0); MAX_POINTS + 1],
            fields: Fields::default(),
        };
        assert_eq!(a.local(vec![too_many]), Err(Reject::TooManyPoints));
        assert_eq!(
            a.local_board(vec![BoardField::Title("x".repeat(65))]),
            Err(Reject::TitleTooLong)
        );
        let author = AuthorInfo {
            name: "a".into(),
            shape: SHAPE_COUNT,
            color: 0,
        };
        assert_eq!(a.local_author(author), Err(Reject::BadValue));
        // Nothing was emitted, so the seq did not move.
        assert!(a.vv().is_empty());
        assert_eq!(
            a.local(vec![Edit::Set {
                id: ObjId {
                    author: 9,
                    counter: 1
                },
                fields: Fields::default()
            }]),
            Err(Reject::UnknownObject)
        );
    }

    #[test]
    fn remote_rejects_and_duplicates() {
        let mut a = Board::new(1);
        let op = Op {
            author: 7,
            seq: 1,
            lamport: 1,
            body: Body::Create {
                id: ObjId {
                    author: 7,
                    counter: 2,
                },
                kind: Kind::Rect,
                base: vec![],
                fields: Fields::default(),
            },
        };
        assert_eq!(a.apply(&op), Err(Reject::BadId));
        // A rejected op is still counted as seen.
        assert_eq!(a.vv().get(&7), Some(&1));
        assert_eq!(a.apply(&op), Err(Reject::Duplicate));
        let far = Op {
            seq: MAX_SEQ_AHEAD + 2,
            ..op.clone()
        };
        assert_eq!(a.apply(&far), Err(Reject::SeqTooFar));
        let revoke = Op {
            author: 7,
            seq: 2,
            lamport: 2,
            body: Body::ClearRevoke {
                clear_id: ObjId {
                    author: 8,
                    counter: 1,
                },
            },
        };
        assert_eq!(a.apply(&revoke), Err(Reject::NotAllowed));
        let clock = Op {
            author: 7,
            seq: 3,
            lamport: MAX_LAMPORT + 1,
            body: Body::Author(AuthorInfo {
                name: "x".into(),
                shape: 0,
                color: 0,
            }),
        };
        assert_eq!(a.apply(&clock), Err(Reject::BadClock));
    }

    #[test]
    fn object_limit() {
        let mut a = Board::new(1);
        for chunk in 0..(MAX_OBJECTS / 100) {
            let edits = (0..100).map(|i| stroke_edit((chunk * 100 + i) as i32, "n"));
            a.local(edits.collect()).unwrap();
        }
        assert_eq!(a.live_count(), MAX_OBJECTS);
        assert_eq!(
            a.local(vec![stroke_edit(0, "n")]),
            Err(Reject::TooManyObjects)
        );
        a.local_clear().unwrap();
        assert_eq!(a.live_count(), 0);
        assert!(a.local(vec![stroke_edit(0, "n")]).is_ok());
    }

    #[test]
    fn state_size_limit() {
        let mut a = Board::new(1);
        let big = |i: i32| Edit::Create {
            kind: Kind::Stroke,
            base: vec![(i, 0); MAX_POINTS],
            fields: Fields::default(),
        };
        let mut i = 0;
        let err = loop {
            match a.local(vec![big(i)]) {
                Ok(_) => i += 1,
                Err(e) => break e,
            }
        };
        assert_eq!(err, Reject::StateTooLarge);
        assert!(a.approx_bytes() <= MAX_STATE_BYTES);
    }

    #[test]
    fn clear_covers_only_seen_objects_and_revoke_restores() {
        let mut host = Board::new(1);
        let mut ben = Board::new(2);
        let seen = ben.local(vec![stroke_edit(0, "n")]).unwrap();
        sync(&ben, &mut host, &seen);
        let unseen = ben.local(vec![stroke_edit(1, "o")]).unwrap();
        let clear = host.local_clear().unwrap();
        sync(&host, &mut ben, std::slice::from_ref(&clear));
        sync(&ben, &mut host, &unseen);
        for b in [&host, &ben] {
            let vis = b.visible_objects();
            assert_eq!(vis.len(), 1);
            assert_eq!(vis[0].id, created_id(&unseen));
        }
        let revoke = host.undo().unwrap();
        assert!(matches!(revoke[0].body, Body::ClearRevoke { .. }));
        sync(&host, &mut ben, &revoke);
        assert_eq!(ben.visible_objects().len(), 2);
        // Redo issues a fresh Clear with the same version vector.
        let again = host.redo().unwrap();
        sync(&host, &mut ben, &again);
        assert_eq!(ben.visible_objects().len(), 1);
        assert_eq!(host.state_hash(), ben.state_hash());
    }

    #[test]
    fn edit_after_clear_keeps_object_deleted() {
        let mut host = Board::new(1);
        let mut ben = Board::new(2);
        let ops = ben.local(vec![note_edit("a")]).unwrap();
        let id = created_id(&ops);
        sync(&ben, &mut host, &ops);
        let clear = host.local_clear().unwrap();
        // Concurrent move by Ben does not bring the note back.
        let mv = ben
            .local(vec![Edit::Set {
                id,
                fields: Fields {
                    geom: Some(Geom {
                        x: 9,
                        ..Geom::default()
                    }),
                    ..Fields::default()
                },
            }])
            .unwrap();
        sync(&host, &mut ben, &[clear]);
        sync(&ben, &mut host, &mv);
        assert!(host.is_deleted(id) && ben.is_deleted(id));
    }

    #[test]
    fn undo_redo_own_create_and_move() {
        let mut a = Board::new(1);
        let id = created_id(&a.local(vec![note_edit("x")]).unwrap());
        let g = |x| Fields {
            geom: Some(Geom {
                x,
                ..Geom::default()
            }),
            ..Fields::default()
        };
        a.local(vec![Edit::Set { id, fields: g(40) }]).unwrap();
        a.undo().unwrap();
        assert_eq!(a.object(id).unwrap().geom.as_ref().unwrap().val.x, 0);
        a.undo().unwrap();
        assert!(a.is_deleted(id));
        assert!(!a.can_undo());
        a.redo().unwrap();
        assert!(!a.is_deleted(id));
        a.redo().unwrap();
        assert_eq!(a.object(id).unwrap().geom.as_ref().unwrap().val.x, 40);
        assert!(!a.can_redo());
        // A new action drops the redo stack.
        a.undo().unwrap();
        a.local(vec![stroke_edit(0, "p")]).unwrap();
        assert!(!a.can_redo());
    }

    #[test]
    fn undo_never_overwrites_a_later_change_by_someone_else() {
        let mut a = Board::new(1);
        let mut b = Board::new(2);
        let ops = a.local(vec![note_edit("mine")]).unwrap();
        let id = created_id(&ops);
        sync(&a, &mut b, &ops);
        let edit = a
            .local(vec![Edit::Set {
                id,
                fields: Fields {
                    text: Some("mine 2".into()),
                    ..Fields::default()
                },
            }])
            .unwrap();
        sync(&a, &mut b, &edit);
        let theirs = b
            .local(vec![Edit::Set {
                id,
                fields: Fields {
                    text: Some("theirs".into()),
                    ..Fields::default()
                },
            }])
            .unwrap();
        sync(&b, &mut a, &theirs);
        // The text edit is skipped; the next undo (the create) hides the note.
        let undo = a.undo().unwrap();
        assert!(matches!(&undo[0].body, Body::Set { fields, .. } if fields.deleted == Some(true)));
        assert_eq!(a.object(id).unwrap().text.as_ref().unwrap().val, "theirs");
    }

    #[test]
    fn board_and_author_registers() {
        let mut a = Board::new(1);
        let mut b = Board::new(2);
        let ops = vec![
            a.local_board(vec![
                BoardField::Title("Plan".into()),
                BoardField::Host(Host { author: 1 }),
                BoardField::Policy(Policy::Selected),
                BoardField::Allowed("k2".into(), true),
                BoardField::LaserForViewers(true),
                BoardField::Presenter(Some(1)),
            ])
            .unwrap(),
            a.local_author(AuthorInfo {
                name: "Ann".into(),
                shape: 3,
                color: 5,
            })
            .unwrap(),
        ];
        sync(&a, &mut b, &ops);
        let s = b.settings();
        assert_eq!(s.title.as_ref().unwrap().val, "Plan");
        assert_eq!(s.policy.as_ref().unwrap().val, Policy::Selected);
        assert!(s.allowed["k2"].val);
        assert_eq!(s.presenter.as_ref().unwrap().val, Some(1));
        assert_eq!(b.author(1).unwrap().name, "Ann");
        assert_eq!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn gaps_and_out_of_order() {
        let mut a = Board::new(1);
        let mut b = Board::new(2);
        let ops: Vec<Op> = (0..4)
            .flat_map(|i| a.local(vec![stroke_edit(i, "n")]).unwrap())
            .collect();
        b.apply(&ops[3]).unwrap();
        b.apply(&ops[1]).unwrap();
        assert_eq!(b.gaps(), vec![(1, 1, 1), (1, 3, 3)]);
        assert!(b.vv().get(&1).is_none());
        b.apply(&ops[0]).unwrap();
        b.apply(&ops[2]).unwrap();
        assert_eq!(b.vv().get(&1), Some(&4));
        assert!(b.gaps().is_empty());
    }

    #[test]
    fn set_before_create_converges() {
        let mut a = Board::new(1);
        let mut b = Board::new(2);
        let create = a.local(vec![note_edit("x")]).unwrap();
        let id = created_id(&create);
        let set = a
            .local(vec![Edit::Set {
                id,
                fields: Fields {
                    fill: Some(3),
                    ..Fields::default()
                },
            }])
            .unwrap();
        b.apply(&set[0]).unwrap();
        assert!(b.visible_objects().is_empty());
        b.apply(&create[0]).unwrap();
        assert_eq!(b.visible_objects().len(), 1);
        assert_eq!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn visible_objects_in_z_order() {
        let mut a = Board::new(1);
        a.local(vec![
            stroke_edit(0, "t"),
            stroke_edit(1, "c"),
            stroke_edit(2, "m"),
        ])
        .unwrap();
        let xs: Vec<i32> = a
            .visible_objects()
            .iter()
            .map(|o| o.geom.as_ref().unwrap().val.x)
            .collect();
        assert_eq!(xs, vec![1, 2, 0]);
        assert_eq!(a.top_z(), Some("t"));
    }

    #[test]
    fn z_helpers() {
        assert_eq!(z_between(None, None).as_deref(), Some("n"));
        let a = z_after("n").unwrap();
        assert!(a.as_str() > "n");
        let b = z_before("n").unwrap();
        assert!(b.as_str() < "n");
        let m = z_between(Some("n"), Some("o")).unwrap();
        assert!("n" < m.as_str() && m.as_str() < "o");
        assert_eq!(z_between(Some("o"), Some("n")), None);
        assert_eq!(z_between(Some("n"), Some("na")), None);
        // Repeated "bring to front" eventually runs out of room; then re-spread.
        let mut top = "n".to_string();
        let mut steps = 0;
        while let Some(next) = z_after(&top) {
            assert!(next > top);
            top = next;
            steps += 1;
        }
        assert!(steps > 100);
        let spread = z_spread(1000);
        assert!(spread.windows(2).all(|w| w[0] < w[1]));
        assert!(spread.iter().all(|z| valid_z(z) && !z.ends_with('a')));
    }

    #[test]
    fn stroke_helpers() {
        let line: Vec<(f64, f64)> = (0..100).map(|i| (f64::from(i), 0.2)).collect();
        assert_eq!(simplify(&line, 0.5).len(), 2);
        assert_eq!(quantize(&[(1.13, -2.5)]), vec![(5, -10)]);
        let (g, base) = prepare_stroke(&[(10.0, 20.0), (12.0, 25.0), (f64::NAN, 0.0)]).unwrap();
        assert_eq!(
            g,
            Geom {
                x: 40,
                y: 80,
                w: 8,
                h: 20
            }
        );
        assert_eq!(base, vec![(0, 0), (8, 20)]);
        assert_eq!(prepare_stroke(&[]), Err(Reject::BadPoints));
    }

    #[test]
    fn delta_carries_missing_state() {
        let mut a = Board::new(1);
        let mut b = Board::new(2);
        let first = a.local(vec![note_edit("x")]).unwrap();
        sync(&a, &mut b, &first);
        a.local(vec![stroke_edit(3, "p")]).unwrap();
        a.local_clear().unwrap();
        let delta = a.delta_since(b.vv());
        assert!(
            delta
                .iter()
                .all(|w| !covered(b.vv(), w.stamp().author, w.stamp().seq))
        );
        b.merge_delta(delta, a.vv());
        assert_eq!(a.state_hash(), b.state_hash());
        assert_eq!(a.vv(), b.vv());
        assert!(a.delta_since(b.vv()).is_empty());
    }

    // ---- property tests ------------------------------------------------------------------------

    #[derive(Clone, Debug)]
    enum Step {
        Create(usize, u8, i32),
        Move(usize, usize, i32),
        Text(usize, usize, u8),
        Delete(usize, usize),
        Front(usize, usize),
        Clear(usize),
        Undo(usize),
        Redo(usize),
        Title(usize, u8),
        Name(usize, u8),
        Sync(usize, usize),
    }

    fn step() -> impl Strategy<Value = Step> {
        let who = 0usize..3;
        prop_oneof![
            4 => (who.clone(), 0u8..9, -50i32..50).prop_map(|(a, k, x)| Step::Create(a, k, x)),
            2 => (who.clone(), any::<usize>(), -50i32..50).prop_map(|(a, i, x)| Step::Move(a, i, x)),
            2 => (who.clone(), any::<usize>(), any::<u8>()).prop_map(|(a, i, t)| Step::Text(a, i, t)),
            1 => (who.clone(), any::<usize>()).prop_map(|(a, i)| Step::Delete(a, i)),
            1 => (who.clone(), any::<usize>()).prop_map(|(a, i)| Step::Front(a, i)),
            1 => who.clone().prop_map(Step::Clear),
            2 => who.clone().prop_map(Step::Undo),
            1 => who.clone().prop_map(Step::Redo),
            1 => (who.clone(), any::<u8>()).prop_map(|(a, t)| Step::Title(a, t)),
            1 => (who.clone(), any::<u8>()).prop_map(|(a, t)| Step::Name(a, t)),
            3 => (who.clone(), who).prop_map(|(a, b)| Step::Sync(a, b)),
        ]
    }

    /// Runs a script on three authors with partial syncing; returns their boards and all ops.
    fn run(steps: &[Step]) -> (Vec<Board>, Vec<Op>) {
        let mut boards: Vec<Board> = (0..3).map(|i| Board::new(100 + i)).collect();
        let mut sent: Vec<Vec<Op>> = vec![Vec::new(); 3];
        let mut all = Vec::new();
        let pick = |b: &Board, i: usize| {
            let vis = b.visible_objects();
            (!vis.is_empty()).then(|| vis[i % vis.len()].id)
        };
        for s in steps {
            let (who, ops): (usize, Vec<Op>) = match *s {
                Step::Create(a, k, x) => {
                    let kind = Kind::from_u8(k).unwrap_or(Kind::Rect);
                    let base = if kind.has_points() {
                        vec![(0, 0), (x.abs(), 3)]
                    } else {
                        vec![]
                    };
                    let z = boards[a].top_z().and_then(z_after).unwrap_or("n".into());
                    let fields = Fields {
                        geom: Some(Geom {
                            x,
                            y: x,
                            w: 4,
                            h: 4,
                        }),
                        ink: Some((x.unsigned_abs() % 6) as u8),
                        z: Some(z),
                        ..Fields::default()
                    };
                    let edit = Edit::Create { kind, base, fields };
                    (a, boards[a].local(vec![edit]).unwrap_or_default())
                }
                Step::Move(a, i, x) => {
                    let Some(id) = pick(&boards[a], i) else {
                        continue;
                    };
                    let f = Fields {
                        geom: Some(Geom {
                            x,
                            y: 0,
                            w: 1,
                            h: 1,
                        }),
                        ..Fields::default()
                    };
                    (
                        a,
                        boards[a].local(vec![Edit::Set { id, fields: f }]).unwrap(),
                    )
                }
                Step::Text(a, i, t) => {
                    let Some(id) = pick(&boards[a], i) else {
                        continue;
                    };
                    let f = Fields {
                        text: Some(format!("t{t}")),
                        alt: Some(format!("a{t}")),
                        ..Fields::default()
                    };
                    (
                        a,
                        boards[a].local(vec![Edit::Set { id, fields: f }]).unwrap(),
                    )
                }
                Step::Delete(a, i) => {
                    let Some(id) = pick(&boards[a], i) else {
                        continue;
                    };
                    let f = Fields {
                        deleted: Some(true),
                        ..Fields::default()
                    };
                    (
                        a,
                        boards[a].local(vec![Edit::Set { id, fields: f }]).unwrap(),
                    )
                }
                Step::Front(a, i) => {
                    let Some(id) = pick(&boards[a], i) else {
                        continue;
                    };
                    let z = boards[a].top_z().and_then(z_after);
                    let f = Fields {
                        z,
                        ..Fields::default()
                    };
                    (
                        a,
                        boards[a]
                            .local(vec![Edit::Set { id, fields: f }])
                            .unwrap_or_default(),
                    )
                }
                Step::Clear(a) => (a, vec![boards[a].local_clear().unwrap()]),
                Step::Undo(a) => (a, boards[a].undo().unwrap_or_default()),
                Step::Redo(a) => (a, boards[a].redo().unwrap_or_default()),
                Step::Title(a, t) => (
                    a,
                    vec![
                        boards[a]
                            .local_board(vec![
                                BoardField::Title(format!("b{t}")),
                                BoardField::Allowed(format!("k{}", t % 3), t % 2 == 0),
                            ])
                            .unwrap(),
                    ],
                ),
                Step::Name(a, t) => {
                    let info = AuthorInfo {
                        name: format!("n{t}"),
                        shape: t % 4,
                        color: t % 6,
                    };
                    (a, vec![boards[a].local_author(info).unwrap()])
                }
                Step::Sync(from, to) => {
                    for op in sent[from].clone() {
                        let _ = boards[to].apply(&op);
                    }
                    (from, vec![])
                }
            };
            sent[who].extend(ops.iter().cloned());
            all.extend(ops);
        }
        (boards, all)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn replicas_converge(
            steps in prop::collection::vec(step(), 1..60),
            orders in prop::collection::vec(any::<prop::sample::Index>(), 3),
            dups in prop::collection::vec(any::<prop::sample::Index>(), 0..30),
            drops in prop::collection::vec(any::<bool>(), 0..120),
        ) {
            let (mut gens, all) = run(&steps);
            prop_assume!(!all.is_empty());
            let mut hashes = Vec::new();
            for (r, order) in orders.iter().enumerate() {
                // A rotation plus a deterministic shuffle, then duplicates, then a first pass
                // that drops some ops and a second pass that resends everything.
                let mut list = all.clone();
                let shift = order.index(list.len());
                list.rotate_left(shift);
                if r == 1 {
                    list.reverse();
                }
                if r == 2 {
                    let n = list.len();
                    for i in 0..n {
                        list.swap(i, (i * 7 + 3) % n);
                    }
                }
                for d in &dups {
                    list.push(all[d.index(all.len())].clone());
                }
                let mut replica = Board::new(999);
                for (i, op) in list.iter().enumerate() {
                    if !drops.get(i).copied().unwrap_or(false) {
                        let _ = replica.apply(op);
                    }
                }
                for op in &list {
                    let _ = replica.apply(op);
                }
                prop_assert!(replica.gaps().is_empty());
                hashes.push(replica.state_hash());
            }
            for g in &mut gens {
                for op in &all {
                    let _ = g.apply(op);
                }
                hashes.push(g.state_hash());
            }
            prop_assert!(hashes.windows(2).all(|w| w[0] == w[1]), "{hashes:?}");
        }

        #[test]
        fn delta_round_trip(
            steps in prop::collection::vec(step(), 1..60),
            keep in prop::collection::vec(any::<bool>(), 0..120),
        ) {
            let (_, all) = run(&steps);
            let mut full = Board::new(999);
            let mut partial = Board::new(998);
            for (i, op) in all.iter().enumerate() {
                let _ = full.apply(op);
                if keep.get(i).copied().unwrap_or(false) {
                    let _ = partial.apply(op);
                }
            }
            let delta = full.delta_since(partial.vv());
            prop_assert_eq!(partial.merge_delta(delta, full.vv()), 0);
            prop_assert_eq!(partial.state_hash(), full.state_hash());
            prop_assert_eq!(partial.vv(), full.vv());
            prop_assert!(partial.gaps().is_empty());
        }

        #[test]
        fn z_between_is_ordered(a in "[a-z]{1,8}", b in "[a-z]{1,8}") {
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            let m = z_between(Some(&lo), Some(&hi));
            if lo < hi && !hi.ends_with('a') {
                let m = m.unwrap();
                prop_assert!(lo < m && m < hi && valid_z(&m));
            } else {
                prop_assert!(m.is_none());
            }
        }
    }
}
