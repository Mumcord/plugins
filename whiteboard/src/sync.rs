//! WB3: the sans-IO sync engine.
//!
//! A [`Session`] is one opened board. It never calls the host: the glue feeds it local edits,
//! incoming messages, presence snapshots and timer ticks, and carries out the returned
//! [`Outgoing`] list (send on the relay, update the page).
//!
//! Board data only travels through Mumcord's relay (`peers.relay`: the community relay or a
//! direct peer-to-peer connection), never as Mumble plugin data: a board is too large and too
//! busy for Mumble's plugin-data rate. While the relay is unavailable nothing is sent; own ops
//! wait in the queue and go out once it is back.
//!
//! How it keeps everyone on the same board:
//! - Own ops are logged and queued. Unsent field writes that a newer own op overwrites are
//!   stripped (coalescing), then the queue goes out as OPS every 50 ms (at most 20 messages per
//!   second) within the relay budget.
//! - A joiner sends HELLO with its version vector. The participant who has been on the board the
//!   longest answers with SNAP: the state delta since that vector, sent directly to the joiner.
//!   If nothing arrived after 3 s the joiner repeats HELLO, and the second-longest participant
//!   answers too. Every participant also introduces itself with a direct HELLO; the joiner
//!   answers that with whatever its own older copy holds that the participant lacks, so older
//!   copies merge in.
//! - A receiver that sees a seq gap asks the author with NEED; the author resends the ops to the
//!   channel. A DIGEST (hash of the version vector) goes out every 30 s and after a reconnect;
//!   on a mismatch the side with fewer ops asks the other for a delta with a direct HELLO.
//! - Each author key is bound to the Mumble session that first used it; messages that claim an
//!   author bound to another present session are dropped.
//! - Permissions are checked before [`Board::apply`]: drawing per the host's policy (by the
//!   sender's attested certificate key), Clear and board settings only from the host. A refused
//!   op is applied as an empty op, so the version vector still moves on and nobody asks for it
//!   again.
//!
//! Decisions where the contract leaves a gap:
//! - The host is derived, not written, when the host leaves: the stored host if present (by
//!   author key or certificate key), otherwise the participant with the earliest `joined_at_ms`
//!   (then the lowest author key). An explicit handover writes the host register.
//! - Enforcement is cooperative and decided when an op arrives. When a policy change races with
//!   an op, replicas may decide differently; state deltas (late join, resync) are trusted as
//!   they come from a participant that already filtered them.
//! - SNAP is always sent directly: a delta is only complete for the vector it was computed
//!   from, so it is accepted only from sessions this replica asked (or introduced itself to).
//! - Relay events carry no "direct" flag, so a HELLO says itself whether it was sent directly
//!   (an introduction or a resync request) or to the channel (a join).
//! - Flood caps: per sender at most [`FLOOD_MSGS`] messages and [`FLOOD_OPS`] ops per 10 s; the
//!   rest is dropped and counted ([`Session::dropped`]).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::codec::{
    self, BoardId, BoardMsg, Cursor, Digest, Hello, Laser, Live, Msg, Need, OpsMsg, Snap, View,
};
use crate::model::{
    AuthorInfo, AuthorKey, Board, BoardField, Body, Edit, Fields, Host, MAX_TITLE_CHARS, ObjId,
    Object, Op, Policy, Reject, Vv, vv_hash,
};

/// A Mumble session id as the host reports it.
pub type SessionId = u32;

/// Relay: 256 kbit/s sustained with a 1 MiB burst.
const RELAY_BYTES_PER_MS: u64 = 32;
const RELAY_CAP_BYTES: u64 = 1 << 20;
const RELAY_OPS_EVERY_MS: u64 = 50;
const CURSOR_EVERY_MS: u64 = 66;
const LIVE_EVERY_MS: u64 = 50;
const LASER_EVERY_MS: u64 = 50;
const VIEW_EVERY_MS: u64 = 200;
const DIGEST_EVERY_MS: u64 = 30_000;
const HELLO_RETRY_MS: u64 = 3_000;
const LOADING_GIVE_UP_MS: u64 = 6_000;
/// A repeated HELLO within this window (but not a near-duplicate) asks the second participant.
const RETRY_WINDOW_MS: (u64, u64) = (2_000, 30_000);
const GAP_GRACE_MS: u64 = 1_000;
const NEED_RETRY_MS: u64 = 10_000;
const NEED_TRIES: u8 = 2;
const RESYNC_EVERY_MS: u64 = 20_000;
const SNAP_STALL_MS: u64 = 20_000;
const EXPECT_SNAP_MS: u64 = 60_000;
const ANNOUNCE_EVERY_MS: u64 = 2_000;
const FLOOD_WINDOW_MS: u64 = 10_000;
/// An honest relay sender stays far below: ops 20/s, cursor 15/s, live 20/s, laser 20/s.
pub const FLOOD_MSGS: u32 = 1_200;
pub const FLOOD_OPS: u32 = 1_000;
const MAX_PEERS: usize = 256;
const MAX_BOUND: usize = 1_024;
const MAX_SNAP_INBOUND: usize = 4;
const MAX_SNAP_BYTES: usize = 8 << 20;
const MAX_SNAP_PARTS: u16 = 9_000;
const MAX_SNAP_OUT: usize = 8;
const MAX_OWN_LOG: usize = 50_000;
const MAX_RESEND: u32 = 2_000;
const MAX_BATCH: usize = 512;
const MAX_CTRL: usize = 64;
const MAX_GAPS: usize = 64;
const MAX_LIVE_STROKES: usize = 4;
/// How many unsent own ops coalescing looks back at.
const COALESCE_WINDOW: usize = 64;

/// Where a message goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Channel,
    Direct(SessionId),
}

/// Which objects the page has to redraw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Changed {
    /// Everything (after a snapshot, a clear or a hide toggle).
    All,
    /// These ids; the glue upserts the visible ones and removes the rest.
    Ids(Vec<ObjId>),
}

/// Something for the screen-reader live region; the glue turns it into text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Announce {
    Joined(AuthorKey),
    Left(AuthorKey),
    HostChanged(AuthorKey),
    /// Objects another person added since the last announcement (throttled to one per 2 s).
    Added {
        author: AuthorKey,
        count: u32,
    },
    Presenting(Option<AuthorKey>),
}

/// Page updates. `Peers` and `Status` only signal a change: the glue reads [`Session::peers`]
/// and [`Session::status`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ui {
    Objects(Changed),
    Peers,
    Status,
    Cursor {
        author: AuthorKey,
        x: i32,
        y: i32,
        status: u8,
    },
    /// A live stroke chunk (quarter pixels); `done` ends the preview.
    Live {
        author: AuthorKey,
        sid: u16,
        points: Vec<(i32, i32)>,
        done: bool,
    },
    Laser {
        author: AuthorKey,
        points: Vec<(i32, i32)>,
    },
    /// The presenter's view, only while following.
    View {
        author: AuthorKey,
        x: i32,
        y: i32,
        zoom_permille: u32,
    },
    Announce(Announce),
    /// A BOARD announcement for any board in the channel.
    BoardSeen(BoardMsg),
}

/// What the glue has to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outgoing {
    RelaySend { payload: Vec<u8>, target: Target },
    Ui(Ui),
}

/// A user in the channel as the host reports them (presence or message sender).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerInfo {
    pub session: SessionId,
    pub name: String,
    /// The attested per-plugin certificate key.
    pub key: Option<String>,
}

pub struct Config {
    pub board: BoardId,
    /// The local user's Mumble session.
    pub session: SessionId,
    /// The local identity; `cert_key` must be the local user's presence key.
    pub info: AuthorInfo,
    /// Wall-clock time the board was opened; the earliest participant answers joiners and
    /// becomes host when the host leaves.
    pub joined_at_ms: u64,
    pub relay: bool,
}

/// Why a local action was not carried out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalError {
    /// The board's rules do not allow it (draw policy, host-only action).
    NotAllowed,
    Model(Reject),
}

/// Toolbar and banner state for the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub relay: bool,
    /// Own ops not sent yet ("Sending n…").
    pub queue: usize,
    pub host: bool,
    pub presenting: Option<AuthorKey>,
    pub following: bool,
    pub can_draw: bool,
    /// Late-join progress in percent while a snapshot loads.
    pub loading: Option<u8>,
    pub undo: bool,
    pub redo: bool,
}

/// One other participant for the side list and cursor chips.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerView {
    pub session: SessionId,
    pub author: AuthorKey,
    pub name: String,
    pub shape: u8,
    pub color: u8,
    pub cursor: Option<(i32, i32)>,
    pub status: u8,
    pub host: bool,
    pub can_draw: bool,
    pub hidden: bool,
}

#[derive(Clone, Copy, Debug)]
struct PeerCursor {
    x: i32,
    y: i32,
    status: u8,
}

#[derive(Debug)]
struct Peer {
    name: String,
    key: Option<String>,
    present: bool,
    author: Option<AuthorKey>,
    /// Set by HELLO: the peer has this board open.
    joined_at: Option<u64>,
    last_hello: Option<u64>,
    cursor: Option<PeerCursor>,
    flood_start: u64,
    flood_msgs: u32,
    flood_ops: u32,
}

impl Peer {
    fn new(info: &PeerInfo, now: u64) -> Peer {
        Peer {
            name: info.name.clone(),
            key: info.key.clone(),
            present: true,
            author: None,
            joined_at: None,
            last_hello: None,
            cursor: None,
            flood_start: now,
            flood_msgs: 0,
            flood_ops: 0,
        }
    }

    fn flood_window(&mut self, now: u64) {
        if now.saturating_sub(self.flood_start) >= FLOOD_WINDOW_MS {
            self.flood_start = now;
            self.flood_msgs = 0;
            self.flood_ops = 0;
        }
    }
}

struct SnapOut {
    to: SessionId,
    parts: VecDeque<Vec<u8>>,
}

struct Inbound {
    parts: Vec<Option<Vec<u8>>>,
    got: usize,
    bytes: usize,
    vv: Option<Vv>,
    last: u64,
}

impl Inbound {
    fn new(of: u16, now: u64) -> Inbound {
        Inbound {
            parts: vec![None; usize::from(of)],
            got: 0,
            bytes: 0,
            vv: None,
            last: now,
        }
    }
}

struct Loading {
    since: u64,
    retried: bool,
}

#[derive(Clone, Copy)]
struct GapState {
    to: u32,
    seen: u64,
    asked: Option<u64>,
    tries: u8,
}

#[derive(Clone, Copy)]
struct LocalCursor {
    x: i32,
    y: i32,
    status: u8,
    tool: u8,
    dirty: bool,
}

struct LiveOut {
    seq: u16,
    points: Vec<(i32, i32)>,
    done: bool,
    last: u64,
}

#[derive(Clone, Copy)]
struct ViewOut {
    x: i32,
    y: i32,
    zoom: u32,
    dirty: bool,
}

/// A participant on the board, for ranking and host election.
struct Part {
    joined: u64,
    author: AuthorKey,
    session: Option<SessionId>,
    key: Option<String>,
}

pub struct Session {
    board: Board,
    id: BoardId,
    me: AuthorKey,
    my_session: SessionId,
    info: AuthorInfo,
    joined_at: u64,
    relay: bool,
    now: u64,
    peers: BTreeMap<SessionId, Peer>,
    bound: BTreeMap<AuthorKey, SessionId>,
    hidden: BTreeSet<AuthorKey>,
    following: bool,
    own_log: BTreeMap<u32, Op>,
    unsent: VecDeque<u32>,
    resend_relay: BTreeSet<u32>,
    relay_ctrl: VecDeque<(Vec<u8>, Target)>,
    snap_out: VecDeque<SnapOut>,
    relay_credit: u64,
    relay_at: u64,
    last_relay_ops: u64,
    last_relay_resend: u64,
    last_cursor: u64,
    last_laser: u64,
    last_view: u64,
    cursor: Option<LocalCursor>,
    live: BTreeMap<u16, LiveOut>,
    laser: Vec<(i32, i32)>,
    view: Option<ViewOut>,
    inbound: BTreeMap<SessionId, Inbound>,
    expect_any_until: u64,
    expect: BTreeMap<SessionId, u64>,
    loading: Option<Loading>,
    gaps: BTreeMap<(AuthorKey, u32), GapState>,
    last_resync: BTreeMap<SessionId, u64>,
    last_digest: u64,
    out: Vec<Outgoing>,
    changed: BTreeSet<ObjId>,
    changed_all: bool,
    peers_dirty: bool,
    added: BTreeMap<AuthorKey, u32>,
    last_added: u64,
    last_status: Option<Status>,
    last_host: Option<AuthorKey>,
    last_presenter: Option<AuthorKey>,
    dropped: u64,
}

fn vv_le(a: &Vv, b: &Vv) -> bool {
    a.iter().all(|(k, &v)| v <= b.get(k).copied().unwrap_or(0))
}

/// The field writes of `newer` make the same writes in `older` redundant.
fn strip(older: &mut Fields, newer: &Fields) {
    if newer.geom.is_some() {
        older.geom = None;
    }
    if newer.ink.is_some() {
        older.ink = None;
    }
    if newer.width.is_some() {
        older.width = None;
    }
    if newer.fill.is_some() {
        older.fill = None;
    }
    if newer.text.is_some() {
        older.text = None;
    }
    if newer.z.is_some() {
        older.z = None;
    }
    if newer.alt.is_some() {
        older.alt = None;
    }
    if newer.deleted.is_some() {
        older.deleted = None;
    }
}

/// Same author, seq and clock, but no writes: what a refused op becomes.
fn neutral(op: &Op) -> Op {
    Op {
        author: op.author,
        seq: op.seq,
        lamport: op.lamport,
        body: Body::Set {
            id: ObjId {
                author: op.author,
                counter: op.seq,
            },
            fields: Fields::default(),
        },
    }
}

impl Session {
    /// `board` is the replica to start from: empty, or the local saved copy.
    pub fn new(cfg: Config, board: Board, now_ms: u64) -> Session {
        let me = board.me();
        Session {
            board,
            id: cfg.board,
            me,
            my_session: cfg.session,
            info: cfg.info,
            joined_at: cfg.joined_at_ms,
            relay: cfg.relay,
            now: now_ms,
            peers: BTreeMap::new(),
            bound: BTreeMap::new(),
            hidden: BTreeSet::new(),
            following: false,
            own_log: BTreeMap::new(),
            unsent: VecDeque::new(),
            resend_relay: BTreeSet::new(),
            relay_ctrl: VecDeque::new(),
            snap_out: VecDeque::new(),
            relay_credit: RELAY_CAP_BYTES,
            relay_at: now_ms,
            last_relay_ops: 0,
            last_relay_resend: 0,
            last_cursor: 0,
            last_laser: 0,
            last_view: 0,
            cursor: None,
            live: BTreeMap::new(),
            laser: Vec::new(),
            view: None,
            inbound: BTreeMap::new(),
            expect_any_until: 0,
            expect: BTreeMap::new(),
            loading: None,
            gaps: BTreeMap::new(),
            last_resync: BTreeMap::new(),
            last_digest: now_ms,
            out: Vec::new(),
            changed: BTreeSet::new(),
            changed_all: false,
            peers_dirty: false,
            added: BTreeMap::new(),
            last_added: now_ms,
            last_status: None,
            last_host: None,
            last_presenter: None,
            dropped: 0,
        }
    }

    // ---- accessors -----------------------------------------------------------------------------

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn board_id(&self) -> &BoardId {
        &self.id
    }

    pub fn me(&self) -> AuthorKey {
        self.me
    }

    /// Messages and ops dropped (malformed, unauthorised, flooded or over a cap).
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn is_hidden(&self, author: AuthorKey) -> bool {
        self.hidden.contains(&author)
    }

    /// Whether the page shows this object: it exists, is not deleted and its author is not
    /// hidden locally.
    pub fn is_visible(&self, id: ObjId) -> bool {
        !self.hidden.contains(&id.author)
            && self.board.object(id).is_some_and(|o| o.kind.is_some())
            && !self.board.is_deleted(id)
    }

    pub fn visible_objects(&self) -> Vec<&Object> {
        self.board
            .visible_objects()
            .into_iter()
            .filter(|o| !self.hidden.contains(&o.id.author))
            .collect()
    }

    pub fn status(&self) -> Status {
        Status {
            relay: self.relay,
            queue: self.unsent.len(),
            host: self.is_host(self.me),
            presenting: self.presenter(),
            following: self.following,
            can_draw: self.can_draw(self.me, self.info.cert_key.as_deref()),
            loading: self.loading_percent(),
            undo: self.board.can_undo(),
            redo: self.board.can_redo(),
        }
    }

    /// Other participants of this board, longest present first.
    pub fn peers(&self) -> Vec<PeerView> {
        let host = self.effective_host();
        let mut out: Vec<(u64, PeerView)> = Vec::new();
        for (&session, p) in &self.peers {
            let (Some(author), Some(joined), true) = (p.author, p.joined_at, p.present) else {
                continue;
            };
            let info = self.board.author(author);
            out.push((
                joined,
                PeerView {
                    session,
                    author,
                    name: p.name.clone(),
                    shape: info.map_or(0, |i| i.shape),
                    color: info.map_or(0, |i| i.color),
                    cursor: p.cursor.map(|c| (c.x, c.y)),
                    status: p.cursor.map_or(0, |c| c.status),
                    host: host == Some(author),
                    can_draw: self.can_draw(author, p.key.as_deref()),
                    hidden: self.hidden.contains(&author),
                },
            ));
        }
        out.sort_by_key(|(joined, v)| (*joined, v.author));
        out.into_iter().map(|(_, v)| v).collect()
    }

    /// The current presenter, if they are still on the board.
    pub fn presenter(&self) -> Option<AuthorKey> {
        let p = self.board.settings().presenter.as_ref()?.val?;
        (p == self.me || self.participant_session(p).is_some()).then_some(p)
    }

    /// The stored host if present, otherwise the longest-present participant.
    pub fn effective_host(&self) -> Option<AuthorKey> {
        let parts = self.participants();
        if let Some(reg) = &self.board.settings().host {
            let h = &reg.val;
            if parts.iter().any(|p| p.author == h.author) {
                return Some(h.author);
            }
            if let Some(k) = &h.cert_key
                && let Some(p) = parts.iter().find(|p| p.key.as_ref() == Some(k))
            {
                return Some(p.author);
            }
        }
        parts.first().map(|p| p.author)
    }

    // ---- local actions -------------------------------------------------------------------------

    /// Binds the local identity and asks the channel for the board (HELLO).
    pub fn start(&mut self, now: u64) -> Vec<Outgoing> {
        self.now = now;
        if self.board.author(self.me) != Some(&self.info)
            && let Ok(op) = self.board.local_author(self.info.clone())
        {
            self.push_own(vec![op]);
        }
        self.join();
        self.flush()
    }

    /// Writes the settings of a newly started board (the local user becomes host).
    pub fn init_board(&mut self, title: &str, now: u64) -> Result<Vec<Outgoing>, LocalError> {
        self.now = now;
        let title: String = title.chars().take(MAX_TITLE_CHARS).collect();
        let op = self
            .board
            .local_board(vec![
                BoardField::Title(title),
                BoardField::Host(Host {
                    author: self.me,
                    cert_key: self.info.cert_key.clone(),
                }),
                BoardField::Policy(Policy::Everyone),
            ])
            .map_err(LocalError::Model)?;
        self.push_own(vec![op]);
        // A new board has nothing to load.
        self.loading = None;
        Ok(self.flush())
    }

    /// Announces the board to the channel (BOARD), e.g. after starting or re-sharing it.
    pub fn announce_board(&mut self, started_ms: u64, now: u64) -> Vec<Outgoing> {
        self.now = now;
        let title = self
            .board
            .settings()
            .title
            .as_ref()
            .map(|r| r.val.clone())
            .unwrap_or_default();
        let msg = Msg::Board(BoardMsg {
            board: self.id,
            title,
            host: self.effective_host().unwrap_or(self.me),
            started_ms,
        });
        self.send_ctrl(&msg, Target::Channel);
        self.flush()
    }

    pub fn local_edit(&mut self, edits: Vec<Edit>, now: u64) -> Result<Vec<Outgoing>, LocalError> {
        self.now = now;
        self.require_draw()?;
        let ops = self.board.local(edits).map_err(LocalError::Model)?;
        self.push_own(ops);
        Ok(self.flush())
    }

    pub fn undo(&mut self, now: u64) -> Result<Vec<Outgoing>, LocalError> {
        self.now = now;
        self.require_draw()?;
        if let Some(ops) = self.board.undo() {
            self.push_own(ops);
        }
        Ok(self.flush())
    }

    pub fn redo(&mut self, now: u64) -> Result<Vec<Outgoing>, LocalError> {
        self.now = now;
        self.require_draw()?;
        if let Some(ops) = self.board.redo() {
            self.push_own(ops);
        }
        Ok(self.flush())
    }

    /// Host only: clears everything this replica has seen (undo restores it).
    pub fn clear(&mut self, now: u64) -> Result<Vec<Outgoing>, LocalError> {
        self.now = now;
        self.require_host()?;
        let op = self.board.local_clear().map_err(LocalError::Model)?;
        self.push_own(vec![op]);
        Ok(self.flush())
    }

    /// Host only: title, draw policy, allowed people, laser for viewers.
    pub fn set_settings(
        &mut self,
        fields: Vec<BoardField>,
        now: u64,
    ) -> Result<Vec<Outgoing>, LocalError> {
        self.now = now;
        self.require_host()?;
        let ok = fields.iter().all(|f| {
            matches!(
                f,
                BoardField::Title(_)
                    | BoardField::Policy(_)
                    | BoardField::Allowed(..)
                    | BoardField::LaserForViewers(_)
            )
        });
        if !ok || fields.is_empty() {
            return Err(LocalError::NotAllowed);
        }
        let op = self.board.local_board(fields).map_err(LocalError::Model)?;
        self.push_own(vec![op]);
        Ok(self.flush())
    }

    /// Host only: makes another participant the host.
    pub fn handover(&mut self, author: AuthorKey, now: u64) -> Result<Vec<Outgoing>, LocalError> {
        self.now = now;
        self.require_host()?;
        let session = self
            .participant_session(author)
            .ok_or(LocalError::NotAllowed)?;
        let cert_key = self.peers.get(&session).and_then(|p| p.key.clone());
        let op = self
            .board
            .local_board(vec![BoardField::Host(Host { author, cert_key })])
            .map_err(LocalError::Model)?;
        self.push_own(vec![op]);
        Ok(self.flush())
    }

    /// Host only: deletes every object of `author` for everyone (one undoable action).
    pub fn remove_author(
        &mut self,
        author: AuthorKey,
        now: u64,
    ) -> Result<Vec<Outgoing>, LocalError> {
        self.now = now;
        self.require_host()?;
        let edits: Vec<Edit> = self
            .board
            .visible_objects()
            .into_iter()
            .filter(|o| o.id.author == author)
            .map(|o| Edit::Set {
                id: o.id,
                fields: Fields {
                    deleted: Some(true),
                    ..Fields::default()
                },
            })
            .collect();
        if !edits.is_empty() {
            let ops = self.board.local(edits).map_err(LocalError::Model)?;
            self.push_own(ops);
        }
        Ok(self.flush())
    }

    /// Hides (or shows again) one person's objects on this screen only.
    pub fn set_hidden(&mut self, author: AuthorKey, on: bool, now: u64) -> Vec<Outgoing> {
        self.now = now;
        let changed = if on {
            self.hidden.insert(author)
        } else {
            self.hidden.remove(&author)
        };
        if changed {
            self.changed_all = true;
            self.peers_dirty = true;
        }
        self.flush()
    }

    /// Starts or stops presenting (anyone who may draw can present).
    pub fn set_presenting(&mut self, on: bool, now: u64) -> Result<Vec<Outgoing>, LocalError> {
        self.now = now;
        let current = self.presenter();
        let field = if on {
            self.require_draw()?;
            (current != Some(self.me)).then_some(BoardField::Presenter(Some(self.me)))
        } else {
            (current == Some(self.me)).then_some(BoardField::Presenter(None))
        };
        if let Some(f) = field {
            let op = self.board.local_board(vec![f]).map_err(LocalError::Model)?;
            self.push_own(vec![op]);
        }
        Ok(self.flush())
    }

    pub fn set_following(&mut self, on: bool, now: u64) -> Vec<Outgoing> {
        self.now = now;
        self.following = on;
        self.flush()
    }

    /// The local cursor in quarter pixels; `status` 0 none, 1 drawing, 2 typing, 3 highlighting.
    pub fn local_cursor(
        &mut self,
        x: i32,
        y: i32,
        status: u8,
        tool: u8,
        now: u64,
    ) -> Vec<Outgoing> {
        self.now = now;
        self.cursor = Some(LocalCursor {
            x,
            y,
            status: status.min(3),
            tool,
            dirty: true,
        });
        self.flush()
    }

    /// Appends points to a live stroke preview (relay only); `done` ends it.
    pub fn local_live(
        &mut self,
        sid: u16,
        points: &[(i32, i32)],
        done: bool,
        now: u64,
    ) -> Vec<Outgoing> {
        self.now = now;
        if self.relay && self.can_draw(self.me, self.info.cert_key.as_deref()) {
            if !self.live.contains_key(&sid) && self.live.len() >= MAX_LIVE_STROKES {
                self.live.pop_first();
            }
            let e = self.live.entry(sid).or_insert(LiveOut {
                seq: 0,
                points: Vec::new(),
                done: false,
                last: 0,
            });
            e.points.extend_from_slice(points);
            // A stalled relay must not grow the buffer: keep the newest points.
            let cap = codec::MAX_LIVE_POINTS * 4;
            if e.points.len() > cap {
                e.points.drain(..e.points.len() - cap);
            }
            e.done |= done;
        }
        self.flush()
    }

    /// Laser trail points (quarter pixels).
    pub fn local_laser(&mut self, points: &[(i32, i32)], now: u64) -> Vec<Outgoing> {
        self.now = now;
        if self.may_laser(self.me, self.info.cert_key.as_deref()) {
            self.laser.extend_from_slice(points);
            if self.laser.len() > codec::MAX_LASER_POINTS {
                self.laser
                    .drain(..self.laser.len() - codec::MAX_LASER_POINTS);
            }
        }
        self.flush()
    }

    /// The presenter's view (centre in quarter pixels, zoom in per mille).
    pub fn local_view(&mut self, x: i32, y: i32, zoom_permille: u32, now: u64) -> Vec<Outgoing> {
        self.now = now;
        if self.presenter() == Some(self.me) {
            self.view = Some(ViewOut {
                x,
                y,
                zoom: zoom_permille,
                dirty: true,
            });
        }
        self.flush()
    }

    // ---- host events ---------------------------------------------------------------------------

    /// A presence snapshot (the local user may be included).
    pub fn on_presence(&mut self, list: &[PeerInfo], now: u64) -> Vec<Outgoing> {
        self.now = now;
        let present: BTreeSet<SessionId> = list.iter().map(|p| p.session).collect();
        for info in list {
            if info.session == self.my_session {
                continue;
            }
            if let Some(p) = self.peers.get_mut(&info.session) {
                p.name.clone_from(&info.name);
                p.key.clone_from(&info.key);
                p.present = true;
            } else if self.peers.len() < MAX_PEERS {
                self.peers.insert(info.session, Peer::new(info, now));
            }
        }
        let gone: Vec<SessionId> = self
            .peers
            .keys()
            .copied()
            .filter(|s| !present.contains(s))
            .collect();
        for s in gone {
            self.peer_gone(s);
        }
        self.flush()
    }

    pub fn on_peer_left(&mut self, session: SessionId, now: u64) -> Vec<Outgoing> {
        self.now = now;
        self.peer_gone(session);
        self.flush()
    }

    /// A `relay-message` event.
    pub fn on_relay_message(&mut self, from: &PeerInfo, payload: &[u8], now: u64) -> Vec<Outgoing> {
        self.now = now;
        match codec::decode_relay(payload) {
            Ok(msg) => self.receive(from, msg),
            Err(_) => self.dropped += 1,
        }
        self.flush()
    }

    /// Call every ~50 ms while the board is open.
    pub fn tick(&mut self, now: u64) -> Vec<Outgoing> {
        self.now = now;
        self.tick_loading();
        self.tick_inbound();
        self.check_gaps();
        if now.saturating_sub(self.last_digest) >= DIGEST_EVERY_MS {
            self.last_digest = now;
            if self.has_others() {
                self.send_digest();
            }
        }
        if now.saturating_sub(self.last_added) >= ANNOUNCE_EVERY_MS {
            self.last_added = now;
            for (author, count) in std::mem::take(&mut self.added) {
                self.out.push(Outgoing::Ui(Ui::Announce(Announce::Added {
                    author,
                    count,
                })));
            }
        }
        self.expect.retain(|_, until| *until > now);
        self.flush()
    }

    pub fn set_relay_available(&mut self, on: bool, now: u64) -> Vec<Outgoing> {
        self.now = now;
        if self.relay != on {
            self.relay = on;
            if on {
                // Whatever went missing meanwhile: introduce again and compare digests.
                self.send_hello(Target::Channel, false);
                self.send_digest();
                self.last_digest = now;
            } else {
                // Stale by the time the relay is back; the digest heals what they carried.
                self.relay_ctrl.clear();
                self.live.clear();
                self.laser.clear();
                self.snap_out.clear();
                self.resend_relay.clear();
            }
            self.peers_dirty = true;
        }
        self.flush()
    }

    /// After reconnecting to the server: introduce again and compare digests.
    pub fn reconnected(&mut self, now: u64) -> Vec<Outgoing> {
        self.now = now;
        self.join();
        self.send_digest();
        self.last_digest = now;
        self.flush()
    }

    // ---- permissions ---------------------------------------------------------------------------

    fn is_host(&self, author: AuthorKey) -> bool {
        self.board
            .settings()
            .host
            .as_ref()
            .is_some_and(|r| r.val.author == author)
            || self.effective_host() == Some(author)
    }

    fn can_draw(&self, author: AuthorKey, key: Option<&str>) -> bool {
        if self.is_host(author) {
            return true;
        }
        let s = self.board.settings();
        match s.policy.as_ref().map(|r| r.val) {
            None | Some(Policy::Everyone) => true,
            Some(Policy::HostOnly) => false,
            Some(Policy::Selected) => key.is_some_and(|k| s.allowed.get(k).is_some_and(|r| r.val)),
        }
    }

    fn may_laser(&self, author: AuthorKey, key: Option<&str>) -> bool {
        self.can_draw(author, key)
            || self
                .board
                .settings()
                .laser_for_viewers
                .as_ref()
                .is_some_and(|r| r.val)
    }

    fn require_draw(&self) -> Result<(), LocalError> {
        if self.can_draw(self.me, self.info.cert_key.as_deref()) {
            Ok(())
        } else {
            Err(LocalError::NotAllowed)
        }
    }

    fn require_host(&self) -> Result<(), LocalError> {
        if self.is_host(self.me) {
            Ok(())
        } else {
            Err(LocalError::NotAllowed)
        }
    }

    /// Whether a remote op may change the board; `key` is the sender's attested key.
    fn allowed(&self, op: &Op, key: Option<&str>) -> bool {
        match &op.body {
            Body::Create { .. } | Body::Set { .. } => self.can_draw(op.author, key),
            Body::Clear { .. } => self.is_host(op.author),
            // The model only accepts a revoke from the Clear's author.
            Body::ClearRevoke { .. } => true,
            Body::Board { fields } => {
                // Without a stored host the board was just started: its creator's first
                // settings may arrive before its HELLO, so nobody's rank can be known yet.
                if self.board.settings().host.is_none() || self.is_host(op.author) {
                    return true;
                }
                let current = self.board.settings().presenter.as_ref().and_then(|r| r.val);
                !fields.is_empty()
                    && self.can_draw(op.author, key)
                    && fields.iter().all(|f| match f {
                        BoardField::Presenter(Some(a)) => *a == op.author,
                        BoardField::Presenter(None) => current == Some(op.author),
                        _ => false,
                    })
            }
            Body::Author(info) => info.cert_key.as_deref() == key,
        }
    }

    // ---- participants --------------------------------------------------------------------------

    fn participants(&self) -> Vec<Part> {
        let mut out = vec![Part {
            joined: self.joined_at,
            author: self.me,
            session: None,
            key: self.info.cert_key.clone(),
        }];
        for (&s, p) in &self.peers {
            if let (true, Some(joined), Some(author)) = (p.present, p.joined_at, p.author) {
                out.push(Part {
                    joined,
                    author,
                    session: Some(s),
                    key: p.key.clone(),
                });
            }
        }
        out.sort_by_key(|p| (p.joined, p.author));
        out
    }

    fn participant_session(&self, author: AuthorKey) -> Option<SessionId> {
        let s = *self.bound.get(&author)?;
        let p = self.peers.get(&s)?;
        (p.present && p.joined_at.is_some() && p.author == Some(author)).then_some(s)
    }

    fn has_others(&self) -> bool {
        self.peers
            .values()
            .any(|p| p.present && p.joined_at.is_some())
    }

    /// My position among the participants, not counting `exclude`.
    fn my_rank(&self, exclude: SessionId) -> usize {
        self.participants()
            .iter()
            .filter(|p| p.session != Some(exclude))
            .position(|p| p.author == self.me)
            .unwrap_or(0)
    }

    fn peer_gone(&mut self, s: SessionId) {
        if let Some(p) = self.peers.remove(&s)
            && p.joined_at.is_some()
            && let Some(a) = p.author
        {
            self.out.push(Outgoing::Ui(Ui::Announce(Announce::Left(a))));
            self.peers_dirty = true;
        }
        self.snap_out.retain(|o| o.to != s);
        self.expect.remove(&s);
        if self.inbound.remove(&s).is_some()
            && let Some(l) = &mut self.loading
        {
            // Let the next tick ask the channel again.
            l.since = self.now.saturating_sub(HELLO_RETRY_MS);
            l.retried = false;
        }
    }

    /// Creates or refreshes the sender's entry and counts the message against its flood cap.
    fn touch_peer(&mut self, from: &PeerInfo) -> bool {
        let now = self.now;
        if !self.peers.contains_key(&from.session) {
            if self.peers.len() >= MAX_PEERS {
                return false;
            }
            self.peers.insert(from.session, Peer::new(from, now));
        }
        let Some(p) = self.peers.get_mut(&from.session) else {
            return false;
        };
        p.key.clone_from(&from.key);
        p.present = true;
        p.flood_window(now);
        p.flood_msgs += 1;
        p.flood_msgs <= FLOOD_MSGS
    }

    /// Binds `author` to `session`. Only a HELLO may bind a new key to a session that already
    /// had one (the board was reopened) or move a key whose session left (a reconnect).
    fn bind(&mut self, session: SessionId, author: AuthorKey, hello: bool) -> bool {
        if author == self.me {
            return false;
        }
        let current = self.peers.get(&session).and_then(|p| p.author);
        match self.bound.get(&author) {
            Some(&s) if s == session => return current == Some(author) || hello,
            Some(&s) => {
                let other_present = self.peers.get(&s).is_some_and(|p| p.present);
                if other_present || !hello {
                    return false;
                }
            }
            None => {
                if current.is_some() && !hello {
                    return false;
                }
            }
        }
        if self.bound.len() >= MAX_BOUND {
            let peers = &self.peers;
            self.bound.retain(|_, s| peers.contains_key(s));
            if self.bound.len() >= MAX_BOUND {
                return false;
            }
        }
        self.bound.insert(author, session);
        if let Some(p) = self.peers.get_mut(&session)
            && p.author != Some(author)
        {
            p.author = Some(author);
            p.joined_at = None;
            p.cursor = None;
        }
        true
    }

    fn bound_to(&self, author: AuthorKey, session: SessionId) -> bool {
        self.bound.get(&author) == Some(&session)
            && self
                .peers
                .get(&session)
                .is_some_and(|p| p.author == Some(author))
    }

    fn key_of(&self, session: SessionId) -> Option<String> {
        self.peers.get(&session).and_then(|p| p.key.clone())
    }

    // ---- receiving -----------------------------------------------------------------------------

    fn receive(&mut self, from: &PeerInfo, msg: Msg) {
        if from.session == self.my_session || !self.touch_peer(from) {
            self.dropped += 1;
            return;
        }
        let s = from.session;
        match msg {
            Msg::Board(b) => self.out.push(Outgoing::Ui(Ui::BoardSeen(b))),
            Msg::Hello(h) if h.board == self.id => self.on_hello(s, h),
            Msg::Ops(m) if m.board == self.id => self.on_ops(s, m),
            Msg::Need(n) if n.board == self.id => self.on_need(n),
            Msg::Digest(d) if d.board == self.id => self.on_digest(s, d),
            Msg::Snap(sn) if sn.board == self.id => self.on_snap(s, sn),
            Msg::Cursor(c) => {
                if self.bound_to(c.author, s) {
                    self.set_peer_cursor(s, c.author, c.x, c.y, c.status.min(3));
                } else {
                    self.dropped += 1;
                }
            }
            Msg::Live(l) => {
                let key = self.key_of(s);
                if self.bound_to(l.author, s)
                    && self.can_draw(l.author, key.as_deref())
                    && !self.hidden.contains(&l.author)
                {
                    let done = l.points.is_empty();
                    self.out.push(Outgoing::Ui(Ui::Live {
                        author: l.author,
                        sid: l.sid,
                        points: l.points,
                        done,
                    }));
                } else {
                    self.dropped += 1;
                }
            }
            Msg::Laser(l) => {
                let key = self.key_of(s);
                if self.bound_to(l.author, s) && self.may_laser(l.author, key.as_deref()) {
                    self.out.push(Outgoing::Ui(Ui::Laser {
                        author: l.author,
                        points: l.points,
                    }));
                } else {
                    self.dropped += 1;
                }
            }
            Msg::View(v) => {
                if self.bound_to(v.author, s) && self.presenter() == Some(v.author) {
                    if self.following {
                        self.out.push(Outgoing::Ui(Ui::View {
                            author: v.author,
                            x: v.x,
                            y: v.y,
                            zoom_permille: v.zoom_permille,
                        }));
                    }
                } else {
                    self.dropped += 1;
                }
            }
            // Another board in the same channel.
            _ => {}
        }
    }

    fn set_peer_cursor(&mut self, s: SessionId, author: AuthorKey, x: i32, y: i32, status: u8) {
        if let Some(p) = self.peers.get_mut(&s) {
            p.cursor = Some(PeerCursor { x, y, status });
            self.out.push(Outgoing::Ui(Ui::Cursor {
                author,
                x,
                y,
                status,
            }));
        }
    }

    fn on_hello(&mut self, s: SessionId, h: Hello) {
        if !self.bind(s, h.author, true) {
            self.dropped += 1;
            return;
        }
        let now = self.now;
        let Some(p) = self.peers.get_mut(&s) else {
            return;
        };
        let prev = p.last_hello.replace(now);
        let new_participant = p.joined_at.is_none();
        p.joined_at = Some(h.joined_at_ms);
        if new_participant {
            self.out
                .push(Outgoing::Ui(Ui::Announce(Announce::Joined(h.author))));
            self.peers_dirty = true;
        }
        if h.direct {
            self.answer_direct_hello(s, &h.vv);
            return;
        }
        if new_participant {
            // Introduce myself so the joiner learns my rank and can push its older copy.
            self.send_hello(Target::Direct(s), true);
            self.expect_from(s);
        }
        let retry = prev.is_some_and(|t0| {
            let d = now.saturating_sub(t0);
            d >= RETRY_WINDOW_MS.0 && d < RETRY_WINDOW_MS.1
        });
        let rank = self.my_rank(s);
        if rank == 0 || (retry && rank == 1) {
            self.queue_snapshot(s, &h.vv, false);
        }
    }

    /// A direct HELLO is an introduction or a resync request: send what the peer lacks. My own
    /// ops go as OPS (the log has them); a state delta covers everything else.
    fn answer_direct_hello(&mut self, s: SessionId, vv: &Vv) {
        let mine = self.board.vv().get(&self.me).copied().unwrap_or(0);
        let theirs = vv.get(&self.me).copied().unwrap_or(0);
        let own_in_log = theirs >= mine || self.queue_resend(theirs + 1, mine);
        self.queue_snapshot(s, vv, own_in_log);
        if !vv_le(vv, self.board.vv()) && self.loading.is_none() {
            self.resync_with(s);
        }
    }

    fn on_ops(&mut self, s: SessionId, m: OpsMsg) {
        if !self.bind(s, m.author, false) {
            self.dropped += 1;
            return;
        }
        let key = self.key_of(s);
        let now = self.now;
        let room = match self.peers.get_mut(&s) {
            Some(p) => {
                p.flood_window(now);
                let room = FLOOD_OPS.saturating_sub(p.flood_ops) as usize;
                p.flood_ops = p
                    .flood_ops
                    .saturating_add(u32::try_from(m.ops.len()).unwrap_or(u32::MAX));
                room
            }
            None => 0,
        };
        if m.ops.len() > room {
            self.dropped += (m.ops.len() - room) as u64;
        }
        for op in m.ops.iter().take(room) {
            if self.board.has_applied(op.author, op.seq) {
                continue;
            }
            if self.allowed(op, key.as_deref()) {
                if self.board.apply(op).is_ok() {
                    self.note_remote(op);
                }
            } else {
                self.dropped += 1;
                let _ = self.board.apply(&neutral(op));
            }
        }
        if let Some((x, y)) = m.cursor {
            self.set_peer_cursor(s, m.author, x, y, 0);
        }
    }

    fn note_remote(&mut self, op: &Op) {
        match &op.body {
            Body::Create { id, .. } => {
                self.changed.insert(*id);
                *self.added.entry(op.author).or_default() += 1;
            }
            Body::Set { id, .. } => {
                self.changed.insert(*id);
            }
            Body::Clear { .. } | Body::ClearRevoke { .. } => self.changed_all = true,
            Body::Board { .. } | Body::Author(_) => self.peers_dirty = true,
        }
    }

    fn on_need(&mut self, n: Need) {
        if n.author != self.me || n.from_seq == 0 || n.from_seq > n.to_seq {
            return;
        }
        if !self.queue_resend(n.from_seq, n.to_seq) {
            // Not in the log (too old or too large): the requester resyncs with a state delta.
            self.dropped += 1;
        }
    }

    fn on_digest(&mut self, s: SessionId, d: Digest) {
        let mine = (self.board.total_ops(), vv_hash(self.board.vv()));
        // Only the side with fewer ops asks, so a mismatch costs one request. While a snapshot
        // is loading the digest is expected to differ.
        if mine < (d.total_ops, d.vv_hash) && self.loading.is_none() && self.inbound.is_empty() {
            self.resync_with(s);
        }
    }

    fn on_snap(&mut self, s: SessionId, sn: Snap) {
        let now = self.now;
        // A transfer that started while expected may take longer than the expectation (a large
        // board on a busy relay); the stall timeout ends it instead.
        let expected = self.expect_any_until > now
            || self.expect.get(&s).is_some_and(|&u| u > now)
            || self.inbound.contains_key(&s);
        if !expected || sn.of == 0 || sn.part >= sn.of || sn.of > MAX_SNAP_PARTS {
            self.dropped += 1;
            return;
        }
        if !self.inbound.contains_key(&s) && self.inbound.len() >= MAX_SNAP_INBOUND {
            self.dropped += 1;
            return;
        }
        let e = self
            .inbound
            .entry(s)
            .or_insert_with(|| Inbound::new(sn.of, now));
        if e.parts.len() != usize::from(sn.of) {
            *e = Inbound::new(sn.of, now);
        }
        if sn.part == 0 {
            if sn.vv.is_none() {
                self.dropped += 1;
                return;
            }
            e.vv = sn.vv;
        }
        let idx = usize::from(sn.part);
        if e.parts[idx].is_none() {
            if e.bytes + sn.data.len() > MAX_SNAP_BYTES {
                self.inbound.remove(&s);
                self.dropped += 1;
                return;
            }
            e.bytes += sn.data.len();
            e.parts[idx] = Some(sn.data);
            e.got += 1;
        }
        e.last = now;
        if let Some(l) = &mut self.loading {
            l.since = now;
        }
        if e.got < e.parts.len() || e.vv.is_none() {
            return;
        }
        let Some(e) = self.inbound.remove(&s) else {
            return;
        };
        let vv = e.vv.unwrap_or_default();
        let data: Vec<u8> = e.parts.into_iter().flatten().flatten().collect();
        match codec::decode_delta(&data) {
            Ok(writes) => {
                let rejected = self.board.merge_delta(writes, &vv);
                self.dropped += rejected as u64;
                self.changed_all = true;
                self.peers_dirty = true;
                if self.inbound.is_empty() {
                    self.loading = None;
                }
            }
            Err(_) => {
                self.dropped += 1;
                if let Some(l) = &mut self.loading {
                    l.since = now.saturating_sub(HELLO_RETRY_MS);
                    l.retried = false;
                }
            }
        }
    }

    // ---- timers --------------------------------------------------------------------------------

    fn join(&mut self) {
        self.send_hello(Target::Channel, false);
        self.loading = Some(Loading {
            since: self.now,
            retried: false,
        });
        self.expect_any_until = self.now + EXPECT_SNAP_MS;
    }

    fn tick_loading(&mut self) {
        let now = self.now;
        let Some(l) = &mut self.loading else {
            return;
        };
        if !self.inbound.is_empty() {
            return;
        }
        let idle = now.saturating_sub(l.since);
        if !l.retried && idle >= HELLO_RETRY_MS {
            l.retried = true;
            l.since = now;
            self.expect_any_until = now + EXPECT_SNAP_MS;
            self.send_hello(Target::Channel, false);
        } else if l.retried && idle >= LOADING_GIVE_UP_MS - HELLO_RETRY_MS {
            self.loading = None;
        }
    }

    fn tick_inbound(&mut self) {
        let now = self.now;
        let before = self.inbound.len();
        self.inbound
            .retain(|_, e| now.saturating_sub(e.last) < SNAP_STALL_MS);
        if self.inbound.len() < before
            && let Some(l) = &mut self.loading
        {
            l.since = now.saturating_sub(HELLO_RETRY_MS);
            l.retried = false;
        }
    }

    fn check_gaps(&mut self) {
        let now = self.now;
        let mut next = BTreeMap::new();
        for (a, from, to) in self.board.gaps().into_iter().take(MAX_GAPS) {
            let st = self.gaps.get(&(a, from)).copied().unwrap_or(GapState {
                to,
                seen: now,
                asked: None,
                tries: 0,
            });
            next.insert((a, from), GapState { to, ..st });
        }
        self.gaps = next;
        let mut needs = Vec::new();
        let mut resync = false;
        let keys: Vec<(AuthorKey, u32)> = self.gaps.keys().copied().collect();
        for (a, from) in keys {
            let session = self.participant_session(a);
            let Some(st) = self.gaps.get_mut(&(a, from)) else {
                continue;
            };
            if now.saturating_sub(st.seen) < GAP_GRACE_MS
                || st
                    .asked
                    .is_some_and(|t| now.saturating_sub(t) < NEED_RETRY_MS)
            {
                continue;
            }
            st.asked = Some(now);
            st.tries = st.tries.saturating_add(1);
            match session {
                Some(s) if st.tries <= NEED_TRIES => {
                    let to = st.to.min(from.saturating_add(MAX_RESEND - 1));
                    needs.push((s, a, from, to));
                }
                _ => resync = true,
            }
        }
        for (s, author, from_seq, to_seq) in needs {
            let msg = Msg::Need(Need {
                board: self.id,
                author,
                from_seq,
                to_seq,
            });
            self.send_ctrl(&msg, Target::Direct(s));
        }
        if resync && let Some(s) = self.participants().iter().find_map(|p| p.session) {
            self.resync_with(s);
        }
    }

    // ---- sending -------------------------------------------------------------------------------

    fn send_hello(&mut self, target: Target, direct: bool) {
        let msg = Msg::Hello(Hello {
            board: self.id,
            author: self.me,
            info: self.info.clone(),
            vv: self.board.vv().clone(),
            host: self.is_host(self.me),
            joined_at_ms: self.joined_at,
            direct,
        });
        self.send_ctrl(&msg, target);
    }

    fn send_digest(&mut self) {
        let msg = Msg::Digest(Digest {
            board: self.id,
            vv_hash: vv_hash(self.board.vv()),
            total_ops: self.board.total_ops(),
        });
        self.send_ctrl(&msg, Target::Channel);
    }

    /// Asks `s` for what it has and I lack (direct HELLO), at most every 20 s per peer.
    fn resync_with(&mut self, s: SessionId) {
        let now = self.now;
        if self
            .last_resync
            .get(&s)
            .is_some_and(|&t| now.saturating_sub(t) < RESYNC_EVERY_MS)
        {
            return;
        }
        if self.last_resync.len() >= MAX_PEERS {
            let peers = &self.peers;
            self.last_resync.retain(|s, _| peers.contains_key(s));
        }
        self.last_resync.insert(s, now);
        self.send_hello(Target::Direct(s), true);
        self.expect_from(s);
    }

    fn expect_from(&mut self, s: SessionId) {
        if self.expect.len() < MAX_PEERS || self.expect.contains_key(&s) {
            self.expect.insert(s, self.now + EXPECT_SNAP_MS);
        }
    }

    fn send_ctrl(&mut self, msg: &Msg, target: Target) {
        match codec::encode_relay(msg) {
            Ok(payload) if self.relay_ctrl.len() < MAX_CTRL => {
                self.relay_ctrl.push_back((payload, target));
            }
            _ => self.dropped += 1,
        }
    }

    /// Queues own ops `from..=to` for a resend to the channel. False if some are not in the log.
    fn queue_resend(&mut self, from: u32, to: u32) -> bool {
        let to = to.min(from.saturating_add(MAX_RESEND - 1));
        let first_unsent = self.unsent.front().copied().unwrap_or(u32::MAX);
        let mut complete = true;
        for seq in from..=to {
            if seq >= first_unsent {
                break;
            }
            if !self.own_log.contains_key(&seq) {
                complete = false;
                continue;
            }
            self.resend_relay.insert(seq);
        }
        complete
    }

    /// Queues a direct SNAP with the state `to` lacks given its vector `base`. With
    /// `exclude_own` my own ops are left out (they go as OPS) and nothing is sent if that leaves
    /// the delta empty.
    fn queue_snapshot(&mut self, to: SessionId, base: &Vv, exclude_own: bool) {
        let mut from = base.clone();
        let mut claim = self.board.vv().clone();
        if exclude_own {
            let mine = claim.get(&self.me).copied().unwrap_or(0);
            from.insert(self.me, mine);
            match base.get(&self.me) {
                Some(&v) if v > 0 => claim.insert(self.me, v),
                _ => claim.remove(&self.me),
            };
        }
        if self.snap_out.iter().any(|o| o.to == to) {
            // Restarting would mix two snapshots at the receiver; a later digest catches up.
            return;
        }
        let writes = self.board.delta_since(&from);
        if exclude_own && writes.is_empty() {
            return;
        }
        let parts = codec::encode_delta(&writes)
            .and_then(|data| codec::snap_messages(&self.id, &claim, &data));
        let Ok(parts) = parts else {
            self.dropped += 1;
            return;
        };
        let parts: VecDeque<Vec<u8>> = parts
            .into_iter()
            .filter_map(|p| codec::encode_relay(&Msg::Snap(p)).ok())
            .collect();
        if self.snap_out.len() >= MAX_SNAP_OUT {
            self.snap_out.pop_front();
        }
        self.snap_out.push_back(SnapOut { to, parts });
        self.expect_from(to);
    }

    fn push_own(&mut self, ops: Vec<Op>) {
        for op in ops {
            if let Body::Create { id, fields, .. } | Body::Set { id, fields } = &op.body {
                self.changed.insert(*id);
                self.coalesce(*id, fields);
            }
            if matches!(op.body, Body::Clear { .. } | Body::ClearRevoke { .. }) {
                self.changed_all = true;
            }
            if matches!(op.body, Body::Board { .. }) {
                self.peers_dirty = true;
            }
            self.unsent.push_back(op.seq);
            self.own_log.insert(op.seq, op);
        }
        // Forget the oldest sent ops; NEED for them falls back to a state delta.
        let first_unsent = self.unsent.front().copied().unwrap_or(u32::MAX);
        while self.own_log.len() > MAX_OWN_LOG {
            match self.own_log.first_key_value() {
                Some((&seq, _)) if seq < first_unsent => {
                    self.own_log.remove(&seq);
                }
                _ => break,
            }
        }
    }

    /// Strips writes from recent unsent Set ops that `fields` overwrites anyway.
    fn coalesce(&mut self, id: ObjId, fields: &Fields) {
        for seq in self.unsent.iter().rev().take(COALESCE_WINDOW) {
            if let Some(Op {
                body: Body::Set { id: oid, fields: f },
                ..
            }) = self.own_log.get_mut(seq)
                && *oid == id
            {
                strip(f, fields);
            }
        }
    }

    fn flush(&mut self) -> Vec<Outgoing> {
        self.pump();
        if self.changed_all {
            self.out.push(Outgoing::Ui(Ui::Objects(Changed::All)));
        } else if !self.changed.is_empty() {
            let ids = self.changed.iter().copied().collect();
            self.out.push(Outgoing::Ui(Ui::Objects(Changed::Ids(ids))));
        }
        self.changed.clear();
        self.changed_all = false;
        let host = self.effective_host();
        if host != self.last_host {
            if let (Some(h), Some(_)) = (host, self.last_host) {
                self.out
                    .push(Outgoing::Ui(Ui::Announce(Announce::HostChanged(h))));
            }
            self.last_host = host;
            self.peers_dirty = true;
        }
        let presenter = self.presenter();
        if presenter != self.last_presenter {
            self.out
                .push(Outgoing::Ui(Ui::Announce(Announce::Presenting(presenter))));
            self.last_presenter = presenter;
            self.peers_dirty = true;
        }
        if std::mem::take(&mut self.peers_dirty) {
            self.out.push(Outgoing::Ui(Ui::Peers));
        }
        let status = self.status();
        if self.last_status.as_ref() != Some(&status) {
            self.last_status = Some(status);
            self.out.push(Outgoing::Ui(Ui::Status));
        }
        std::mem::take(&mut self.out)
    }

    fn loading_percent(&self) -> Option<u8> {
        self.loading.as_ref()?;
        let best = self
            .inbound
            .values()
            .map(|e| e.got * 100 / e.parts.len().max(1))
            .max()
            .unwrap_or(0);
        Some(best.min(99) as u8)
    }

    fn pump(&mut self) {
        if self.relay {
            self.pump_relay();
        }
    }

    /// Packs the next own ops (the unsent queue, or resends) into one OPS payload. Ops too large
    /// for the transport are skipped; receivers heal the gap with a state delta.
    fn take_ops(&mut self, resend: bool) -> Option<Vec<u8>> {
        loop {
            let seqs: Vec<u32> = if resend {
                self.resend_relay.iter().take(MAX_BATCH).copied().collect()
            } else {
                self.unsent.iter().take(MAX_BATCH).copied().collect()
            };
            if seqs.is_empty() {
                return None;
            }
            let ops: Vec<Op> = seqs
                .iter()
                .map_while(|s| self.own_log.get(s).cloned())
                .collect();
            let packed = if ops.is_empty() {
                Err(codec::EncodeError::BadOp)
            } else {
                codec::pack_ops(&self.id, self.me, &ops)
            };
            let (payload, n) = match packed {
                Ok((payload, n)) => (Some(payload), n),
                Err(_) => {
                    self.dropped += 1;
                    (None, 1)
                }
            };
            for s in &seqs[..n.min(seqs.len())] {
                if resend {
                    self.resend_relay.remove(s);
                } else {
                    self.unsent.pop_front();
                }
            }
            if payload.is_some() {
                return payload;
            }
        }
    }

    fn relay_send(&mut self, payload: Vec<u8>, target: Target) {
        self.relay_credit = self.relay_credit.saturating_sub(payload.len() as u64);
        self.out.push(Outgoing::RelaySend { payload, target });
    }

    fn pump_relay(&mut self) {
        let now = self.now;
        if now > self.relay_at {
            self.relay_credit = (self.relay_credit + (now - self.relay_at) * RELAY_BYTES_PER_MS)
                .min(RELAY_CAP_BYTES);
            self.relay_at = now;
        }
        let bulk_ok = |credit: u64| credit >= codec::MAX_RELAY_PAYLOAD as u64;
        while self
            .relay_ctrl
            .front()
            .is_some_and(|(p, _)| self.relay_credit >= p.len() as u64)
        {
            if let Some((p, target)) = self.relay_ctrl.pop_front() {
                self.relay_send(p, target);
            }
        }
        if !self.unsent.is_empty()
            && now.saturating_sub(self.last_relay_ops) >= RELAY_OPS_EVERY_MS
            && bulk_ok(self.relay_credit)
            && let Some(p) = self.take_ops(false)
        {
            self.last_relay_ops = now;
            self.relay_send(p, Target::Channel);
        }
        if !self.resend_relay.is_empty()
            && now.saturating_sub(self.last_relay_resend) >= RELAY_OPS_EVERY_MS
            && bulk_ok(self.relay_credit)
            && let Some(p) = self.take_ops(true)
        {
            self.last_relay_resend = now;
            self.relay_send(p, Target::Channel);
        }
        while bulk_ok(self.relay_credit) {
            let Some(o) = self.snap_out.front_mut() else {
                break;
            };
            let to = o.to;
            match o.parts.pop_front() {
                Some(p) => self.relay_send(p, Target::Direct(to)),
                None => self.snap_out.retain(|o| !o.parts.is_empty()),
            }
        }
        self.pump_realtime();
    }

    /// Cursor, live strokes, laser and view: newest wins, dropped when the relay budget is low.
    fn pump_realtime(&mut self) {
        let now = self.now;
        let mut msgs: Vec<Msg> = Vec::new();
        if let Some(c) = &mut self.cursor
            && c.dirty
            && now.saturating_sub(self.last_cursor) >= CURSOR_EVERY_MS
        {
            c.dirty = false;
            self.last_cursor = now;
            msgs.push(Msg::Cursor(Cursor {
                author: self.me,
                x: c.x,
                y: c.y,
                status: c.status,
                tool: c.tool,
            }));
        }
        let mut finished = Vec::new();
        for (&sid, l) in &mut self.live {
            if now.saturating_sub(l.last) < LIVE_EVERY_MS {
                continue;
            }
            if l.points.is_empty() && !l.done {
                continue;
            }
            let n = l.points.len().min(codec::MAX_LIVE_POINTS);
            let points: Vec<(i32, i32)> = l.points.drain(..n).collect();
            if points.is_empty() {
                finished.push(sid);
            }
            l.last = now;
            l.seq = l.seq.wrapping_add(1);
            msgs.push(Msg::Live(Live {
                author: self.me,
                sid,
                seq: l.seq,
                points,
            }));
        }
        for sid in finished {
            self.live.remove(&sid);
        }
        if !self.laser.is_empty() && now.saturating_sub(self.last_laser) >= LASER_EVERY_MS {
            self.last_laser = now;
            let points = std::mem::take(&mut self.laser);
            msgs.push(Msg::Laser(Laser {
                author: self.me,
                points,
            }));
        }
        if let Some(v) = &mut self.view
            && v.dirty
            && now.saturating_sub(self.last_view) >= VIEW_EVERY_MS
        {
            v.dirty = false;
            self.last_view = now;
            msgs.push(Msg::View(View {
                author: self.me,
                x: v.x,
                y: v.y,
                zoom_permille: v.zoom,
            }));
        }
        for m in msgs {
            match codec::encode_relay(&m) {
                Ok(p) if self.relay_credit >= p.len() as u64 => self.relay_send(p, Target::Channel),
                _ => self.dropped += 1,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Geom, Kind, prepare_stroke, z_after};

    const BOARD: BoardId = [9; 16];

    /// xorshift64*: deterministic and dependency-free.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n.max(1)
        }

        fn permille(&mut self, p: u64) -> bool {
            self.below(1_000) < p
        }
    }

    #[derive(Clone, Copy)]
    struct Chaos {
        drop: u64,
        dup: u64,
        min_delay: u64,
        max_delay: u64,
    }

    const CALM: Chaos = Chaos {
        drop: 0,
        dup: 0,
        min_delay: 30,
        max_delay: 120,
    };

    const ROUGH: Chaos = Chaos {
        drop: 60,
        dup: 60,
        min_delay: 20,
        max_delay: 1_500,
    };

    struct Client {
        s: Session,
        session: SessionId,
        key: String,
        online: bool,
        last_ui: Vec<Ui>,
    }

    struct Net {
        now: u64,
        seq: u64,
        rng: Rng,
        chaos: Chaos,
        clients: Vec<Client>,
        queue: BTreeMap<(u64, u64), (usize, usize, Vec<u8>)>,
    }

    fn author_key(i: usize) -> AuthorKey {
        0x1357_0000 + (i as u32) * 7_919
    }

    fn info(i: usize) -> AuthorInfo {
        AuthorInfo {
            name: format!("P{i}"),
            cert_key: Some(format!("{:064x}", i + 1)),
            shape: (i % 4) as u8,
            color: (i % 6) as u8,
        }
    }

    impl Net {
        fn new(seed: u64, chaos: Chaos) -> Net {
            Net {
                now: 0,
                seq: 0,
                rng: Rng(seed | 1),
                chaos,
                clients: Vec::new(),
                queue: BTreeMap::new(),
            }
        }

        /// Adds an offline client with key index `i` starting from `board`.
        fn add(&mut self, i: usize, board: Board) -> usize {
            let cfg = Config {
                board: BOARD,
                session: 100 + i as u32,
                info: info(i),
                joined_at_ms: 0,
                relay: true,
            };
            self.clients.push(Client {
                s: Session::new(cfg, board, self.now),
                session: 100 + i as u32,
                key: format!("{:064x}", i + 1),
                online: false,
                last_ui: Vec::new(),
            });
            self.clients.len() - 1
        }

        fn peer_info(&self, c: usize) -> PeerInfo {
            PeerInfo {
                session: self.clients[c].session,
                name: format!("User {c}"),
                key: Some(self.clients[c].key.clone()),
            }
        }

        fn online(&mut self, c: usize) {
            let now = self.now;
            self.clients[c].online = true;
            self.clients[c].s.joined_at = now;
            self.presence();
            let outs = self.clients[c].s.start(now);
            self.handle(c, outs);
        }

        fn offline(&mut self, c: usize) {
            self.clients[c].online = false;
            self.presence();
        }

        fn presence(&mut self) {
            let list: Vec<PeerInfo> = (0..self.clients.len())
                .filter(|&c| self.clients[c].online)
                .map(|c| self.peer_info(c))
                .collect();
            for c in 0..self.clients.len() {
                if self.clients[c].online {
                    let outs = self.clients[c].s.on_presence(&list, self.now);
                    self.handle(c, outs);
                }
            }
        }

        fn handle(&mut self, c: usize, outs: Vec<Outgoing>) {
            for o in outs {
                match o {
                    Outgoing::RelaySend { payload, target } => {
                        assert!(payload.len() <= codec::MAX_RELAY_PAYLOAD);
                        self.route(c, target, || payload.clone());
                    }
                    Outgoing::Ui(u) => {
                        let ui = &mut self.clients[c].last_ui;
                        if ui.len() < 10_000 {
                            ui.push(u);
                        }
                    }
                }
            }
        }

        fn route(&mut self, from: usize, target: Target, wire: impl Fn() -> Vec<u8>) {
            for to in 0..self.clients.len() {
                let hit = match target {
                    Target::Channel => to != from,
                    Target::Direct(s) => self.clients[to].session == s,
                };
                if !hit || !self.clients[to].online {
                    continue;
                }
                let copies = if self.rng.permille(self.chaos.drop) {
                    0
                } else if self.rng.permille(self.chaos.dup) {
                    2
                } else {
                    1
                };
                for _ in 0..copies {
                    let span = self.chaos.max_delay - self.chaos.min_delay + 1;
                    let at = self.now + self.chaos.min_delay + self.rng.below(span);
                    self.seq += 1;
                    self.queue.insert((at, self.seq), (to, from, wire()));
                }
            }
        }

        fn deliver_due(&mut self) {
            while let Some(entry) = self.queue.first_entry() {
                if entry.key().0 > self.now {
                    break;
                }
                let (to, from, wire) = entry.remove();
                if !self.clients[to].online || !self.clients[from].online {
                    continue;
                }
                let sender = self.peer_info(from);
                let now = self.now;
                let outs = self.clients[to].s.on_relay_message(&sender, &wire, now);
                self.handle(to, outs);
            }
        }

        /// Advances simulated time in 10 ms steps, ticking every 50 ms.
        fn run(&mut self, ms: u64) {
            let end = self.now + ms;
            while self.now < end {
                self.now += 10;
                self.deliver_due();
                if self.now.is_multiple_of(50) {
                    for c in 0..self.clients.len() {
                        if self.clients[c].online {
                            let outs = self.clients[c].s.tick(self.now);
                            self.handle(c, outs);
                        }
                    }
                }
            }
        }

        /// Runs until every online client has the same state hash, or `max_ms` passes.
        fn settle(&mut self, max_ms: u64) -> Option<u64> {
            let start = self.now;
            while self.now - start < max_ms {
                self.run(500);
                if self.converged() && self.queue.is_empty() {
                    return Some(self.now - start);
                }
            }
            None
        }

        fn converged(&self) -> bool {
            let hashes: BTreeSet<u64> = self
                .clients
                .iter()
                .filter(|c| c.online)
                .map(|c| c.s.board().state_hash())
                .collect();
            hashes.len() == 1
        }

        fn edit(&mut self, c: usize, edits: Vec<Edit>) -> Result<(), LocalError> {
            let outs = self.clients[c].s.local_edit(edits, self.now)?;
            self.handle(c, outs);
            Ok(())
        }

        fn act(&mut self, c: usize, f: impl FnOnce(&mut Session, u64) -> Vec<Outgoing>) {
            let now = self.now;
            let outs = f(&mut self.clients[c].s, now);
            self.handle(c, outs);
        }

        fn random_edit(&mut self, c: usize) {
            let roll = self.rng.below(100);
            let board = self.clients[c].s.board();
            let visible: Vec<ObjId> = board.visible_objects().iter().map(|o| o.id).collect();
            let pick = (!visible.is_empty())
                .then(|| visible[self.rng.below(visible.len() as u64) as usize]);
            let edits = match (roll, pick) {
                (0..40, _) | (_, None) => vec![stroke(&mut self.rng, board)],
                (40..55, _) => vec![note(&mut self.rng, board)],
                (55..80, Some(id)) => vec![Edit::Set {
                    id,
                    fields: Fields {
                        geom: Some(rect(&mut self.rng)),
                        ..Fields::default()
                    },
                }],
                (80..90, Some(id)) => vec![Edit::Set {
                    id,
                    fields: Fields {
                        deleted: Some(true),
                        ..Fields::default()
                    },
                }],
                _ => {
                    let now = self.now;
                    if let Ok(outs) = self.clients[c].s.undo(now) {
                        self.handle(c, outs);
                    }
                    return;
                }
            };
            self.edit(c, edits).expect("honest edit");
        }
    }

    fn next_z(b: &Board) -> String {
        b.top_z().and_then(z_after).unwrap_or_else(|| "n".into())
    }

    fn rect(rng: &mut Rng) -> Geom {
        Geom {
            x: rng.below(16_000) as i32,
            y: rng.below(16_000) as i32,
            w: 40 + rng.below(800) as i32,
            h: 40 + rng.below(800) as i32,
        }
    }

    fn stroke(rng: &mut Rng, b: &Board) -> Edit {
        let n = 40 + rng.below(160) as usize;
        let (ox, oy) = (rng.below(4_000) as f64, rng.below(4_000) as f64);
        let phase = rng.below(1_000) as f64 / 100.0;
        let pts: Vec<(f64, f64)> = (0..n)
            .map(|i| {
                let t = i as f64 / 10.0 + phase;
                (
                    ox + i as f64 * 1.2 + 12.0 * t.cos(),
                    oy + 25.0 * (t * 0.7).sin(),
                )
            })
            .collect();
        let (geom, base) = prepare_stroke(&pts).expect("stroke");
        Edit::Create {
            kind: Kind::Stroke,
            base,
            fields: Fields {
                geom: Some(geom),
                ink: Some(rng.below(6) as u8),
                width: Some(1),
                z: Some(next_z(b)),
                ..Fields::default()
            },
        }
    }

    fn note(rng: &mut Rng, b: &Board) -> Edit {
        Edit::Create {
            kind: Kind::Note,
            base: Vec::new(),
            fields: Fields {
                geom: Some(rect(rng)),
                fill: Some(rng.below(5) as u8),
                text: Some(format!("note {}", rng.below(1_000))),
                z: Some(next_z(b)),
                ..Fields::default()
            },
        }
    }

    /// Client 0 starts the board alone, the others join half a second later.
    fn start_net(seed: u64, chaos: Chaos, clients: usize) -> Net {
        let mut net = Net::new(seed, chaos);
        for i in 0..clients {
            net.add(i, Board::new(author_key(i)));
        }
        net.online(0);
        let now = net.now;
        let outs = net.clients[0].s.init_board("Plan", now).unwrap();
        net.handle(0, outs);
        net.run(500);
        for c in 1..clients {
            net.online(c);
            net.run(100);
        }
        net
    }

    fn chaos_run(seed: u64, clients: usize, edit_every: u64) -> Net {
        let mut net = start_net(seed, ROUGH, clients);
        let end = net.now + 60_000;
        while net.now < end {
            let c = net.rng.below(clients as u64) as usize;
            net.random_edit(c);
            let (x, y) = (net.rng.below(4_000) as i32, net.rng.below(4_000) as i32);
            net.act(c, |s, now| s.local_cursor(x, y, 1, 0, now));
            net.act(c, |s, now| {
                s.local_live(1, &[(x, y), (x + 4, y + 2)], false, now)
            });
            let pause = edit_every / 2 + net.rng.below(edit_every);
            net.run(pause);
        }
        let took = net.settle(600_000);
        if took.is_none() {
            for (i, c) in net.clients.iter().enumerate() {
                let s = &c.s;
                eprintln!(
                    "c{i} hash {:x} vv {:?} gaps {:?} loading {:?} inbound {} snap_out {} unsent {} resend {} q {} dropped {} live {}",
                    s.board().state_hash(),
                    s.board().vv(),
                    s.board().gaps(),
                    s.loading.is_some(),
                    s.inbound.len(),
                    s.snap_out.len(),
                    s.unsent.len(),
                    s.resend_relay.len(),
                    net.queue.len(),
                    s.dropped(),
                    s.board().live_count()
                );
            }
        }
        assert!(took.is_some(), "replicas did not converge");
        net
    }

    #[test]
    fn four_clients_converge_under_chaos() {
        for seed in [11, 12] {
            let net = chaos_run(seed, 4, 300);
            let objects = net.clients[0].s.board().visible_objects().len();
            assert!(objects > 50, "too few objects: {objects}");
        }
    }

    #[test]
    fn coalescing_strips_overwritten_fields() {
        let mut net = start_net(6, CALM, 2);
        net.run(3_000);
        let mut rng = Rng(77);
        let b = net.clients[1].s.board();
        let edit = note(&mut rng, b);
        net.edit(1, vec![edit]).unwrap();
        let id = net.clients[1]
            .s
            .board()
            .visible_objects()
            .last()
            .map(|o| o.id)
            .unwrap();
        // A burst of moves while the queue waits for the rate limit.
        net.run(10);
        for i in 0..30 {
            let g = Geom {
                x: i * 8,
                y: 0,
                w: 400,
                h: 400,
            };
            let fields = Fields {
                geom: Some(g),
                ..Fields::default()
            };
            net.edit(1, vec![Edit::Set { id, fields }]).unwrap();
        }
        let s = &net.clients[1].s;
        let carrying: usize = s
            .unsent
            .iter()
            .filter(|q| match &s.own_log[q].body {
                Body::Set { fields, .. } => fields.geom.is_some(),
                _ => false,
            })
            .count();
        assert_eq!(carrying, 1, "only the last move should carry a position");
        assert!(net.settle(120_000).is_some());
        let geom = net.clients[0]
            .s
            .board()
            .object(id)
            .and_then(|o| o.geom.clone());
        assert_eq!(geom.map(|r| r.val.x), Some(29 * 8));
    }

    /// Returns (board, visible ids) of a board with `n` strokes by `author`.
    fn drawn_board(author: AuthorKey, seed: u64, min_bytes: usize) -> Board {
        let mut b = Board::new(author);
        let mut rng = Rng(seed);
        while b.approx_bytes() < min_bytes {
            let e = stroke(&mut rng, &b);
            b.local(vec![e]).unwrap();
        }
        b
    }

    /// Simulated time until a fresh joiner holds a board of `kib` KiB (model state size).
    fn join_time(kib: usize) -> u64 {
        let mut net = Net::new(21, CALM);
        let host = drawn_board(author_key(0), 3, kib * 1024);
        let delta = codec::encode_delta(&host.delta_since(&Vv::new())).unwrap();
        let raw = miniz_oxide::inflate::decompress_to_vec(&delta)
            .unwrap()
            .len();
        eprintln!(
            "{kib} KiB board: {} objects, snapshot {} bytes deflated, {raw} raw",
            host.visible_objects().len(),
            delta.len()
        );
        net.add(0, host);
        net.add(1, Board::new(author_key(1)));
        net.online(0);
        // The answering participant has been idle for a while (full send budget).
        net.run(10_000);
        net.online(1);
        let start = net.now;
        while !net.converged() && net.now - start < 120_000 {
            net.run(10);
        }
        assert!(net.converged());
        let took = net.now - start;
        eprintln!("late join of {kib} KiB : {took} ms");
        took
    }

    #[test]
    fn snapshot_of_150_kib_board_loads_in_time() {
        assert!(join_time(150) <= 3_000);
    }

    #[test]
    fn joiner_shows_loading_until_the_board_is_complete() {
        let mut net = Net::new(22, CALM);
        net.add(0, drawn_board(author_key(0), 4, 60 * 1024));
        net.add(1, Board::new(author_key(1)));
        net.online(0);
        net.run(1_000);
        net.online(1);
        let mut seen = BTreeSet::new();
        for _ in 0..1_500 {
            net.run(10);
            if let Some(p) = net.clients[1].s.status().loading {
                seen.insert(p);
            }
        }
        // The relay delivers the parts within a few milliseconds of each other: loading shows
        // up when the joiner starts and is gone once the board is complete.
        assert!(seen.contains(&0), "{seen:?}");
        assert!(net.clients[1].s.status().loading.is_none());
        assert!(net.converged());
    }

    fn late_joiner(older_copy: bool) {
        let mut net = start_net(31, ROUGH, 2);
        for _ in 0..10 {
            let c = net.rng.below(2) as usize;
            let b = net.clients[c].s.board();
            let e = stroke(&mut net.rng, b);
            net.edit(c, vec![e]).unwrap();
            net.run(2_000);
        }
        assert!(net.settle(300_000).is_some());
        let mut expected = net.clients[0].s.board().visible_objects().len();
        let joiner = if older_copy {
            // The joiner was here before: its saved copy has the board so far plus three strokes
            // it drew while nobody else was around (under its old author key).
            let snap = net.clients[0].s.board();
            let mut old = Board::new(0xdead_0001);
            old.merge_delta(snap.delta_since(&Vv::new()), snap.vv());
            let mut rng = Rng(9);
            for _ in 0..3 {
                let e = stroke(&mut rng, &old);
                old.local(vec![e]).unwrap();
            }
            expected += 3;
            let mut fresh = Board::new(author_key(2));
            fresh.merge_delta(old.delta_since(&Vv::new()), old.vv());
            fresh
        } else {
            Board::new(author_key(2))
        };
        // Meanwhile the others keep drawing.
        for c in 0..2 {
            let b = net.clients[c].s.board();
            let e = stroke(&mut net.rng, b);
            net.edit(c, vec![e]).unwrap();
            expected += 1;
        }
        let j = net.add(2, joiner);
        net.online(j);
        assert!(
            net.settle(300_000).is_some(),
            "late joiner did not converge"
        );
        for c in &net.clients {
            assert_eq!(c.s.board().visible_objects().len(), expected);
        }
    }

    #[test]
    fn late_joiner_without_copy_converges() {
        late_joiner(false);
    }

    #[test]
    fn late_joiner_with_older_copy_merges_without_duplicates() {
        late_joiner(true);
    }

    /// Lets a modified client bypass its own permission checks.
    fn forge(net: &mut Net, c: usize, edits: Vec<Edit>) -> Vec<ObjId> {
        let ops = net.clients[c].s.board.local(edits).unwrap();
        let ids = ops
            .iter()
            .filter_map(|op| match &op.body {
                Body::Create { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        net.clients[c].s.push_own(ops);
        let outs = net.clients[c].s.flush();
        net.handle(c, outs);
        ids
    }

    fn known(net: &Net, c: usize, id: ObjId) -> bool {
        net.clients[c].s.is_visible(id)
    }

    #[test]
    fn draw_policy_is_enforced_by_receivers() {
        let mut net = start_net(41, CALM, 3);
        net.run(5_000);
        let key_b = net.clients[1].key.clone();
        let outs = net.clients[0]
            .s
            .set_settings(
                vec![
                    BoardField::Policy(Policy::Selected),
                    BoardField::Allowed(key_b, true),
                ],
                net.now,
            )
            .unwrap();
        net.handle(0, outs);
        assert!(net.settle(120_000).is_some());
        // B may draw, C may not.
        let e = stroke(&mut net.rng, net.clients[1].s.board());
        net.edit(1, vec![e]).unwrap();
        let e = stroke(&mut net.rng, net.clients[2].s.board());
        assert_eq!(
            net.edit(2, vec![e]),
            Err(LocalError::NotAllowed),
            "{:?} {:?}",
            net.clients[2].s.board().settings().policy,
            net.clients[2].s.effective_host()
        );
        assert!(!net.clients[2].s.status().can_draw);
        // A modified C draws anyway: everyone else ignores it.
        let e = stroke(&mut net.rng, net.clients[2].s.board());
        let forged = forge(&mut net, 2, vec![e]);
        net.run(30_000);
        for c in 0..2 {
            assert!(!known(&net, c, forged[0]), "client {c}");
        }
        assert!(net.clients[0].s.dropped() >= 1);
        let b_objects = net.clients[1].s.board().visible_objects().len();
        assert_eq!(net.clients[0].s.board().visible_objects().len(), b_objects);
        // Host only: B is refused now as well.
        let outs = net.clients[0]
            .s
            .set_settings(vec![BoardField::Policy(Policy::HostOnly)], net.now)
            .unwrap();
        net.handle(0, outs);
        net.run(30_000);
        let e = stroke(&mut net.rng, net.clients[1].s.board());
        assert_eq!(net.edit(1, vec![e]), Err(LocalError::NotAllowed));
        let e = stroke(&mut net.rng, net.clients[0].s.board());
        net.edit(0, vec![e]).unwrap();
        net.run(30_000);
        assert_eq!(
            net.clients[1].s.board().visible_objects().len(),
            net.clients[0].s.board().visible_objects().len()
        );
        // Settings from a non-host (a modified B) are ignored too.
        let ops = net.clients[1]
            .s
            .board
            .local_board(vec![BoardField::Policy(Policy::Everyone)])
            .unwrap();
        net.clients[1].s.push_own(vec![ops]);
        let outs = net.clients[1].s.flush();
        net.handle(1, outs);
        net.run(30_000);
        let policy = net.clients[0].s.board().settings().policy.clone();
        assert_eq!(policy.map(|r| r.val), Some(Policy::HostOnly));
        // A non-host cannot clear.
        assert_eq!(
            net.clients[1].s.clear(net.now).err(),
            Some(LocalError::NotAllowed)
        );
    }

    #[test]
    fn host_hands_over_automatically_and_explicitly() {
        let mut net = start_net(51, CALM, 3);
        net.run(5_000);
        let (a, b, c) = (author_key(0), author_key(1), author_key(2));
        for cl in &net.clients {
            assert_eq!(cl.s.effective_host(), Some(a));
        }
        net.offline(0);
        net.run(1_000);
        // B joined before C, so B is host on both.
        assert_eq!(net.clients[1].s.effective_host(), Some(b));
        assert_eq!(net.clients[2].s.effective_host(), Some(b));
        assert!(
            net.clients[2]
                .last_ui
                .contains(&Ui::Announce(Announce::HostChanged(b)))
        );
        let now = net.now;
        assert_eq!(
            net.clients[2]
                .s
                .set_settings(vec![BoardField::Title("C".into())], now)
                .err(),
            Some(LocalError::NotAllowed)
        );
        let outs = net.clients[1]
            .s
            .set_settings(vec![BoardField::Title("B's board".into())], now)
            .unwrap();
        net.handle(1, outs);
        net.run(1_000);
        let outs = net.clients[1].s.handover(c, net.now).unwrap();
        net.handle(1, outs);
        assert!(net.settle(120_000).is_some());
        for cl in &net.clients[1..] {
            assert_eq!(cl.s.effective_host(), Some(c));
            let title = cl.s.board().settings().title.clone().map(|r| r.val);
            assert_eq!(title.as_deref(), Some("B's board"));
        }
        assert!(net.clients[2].s.status().host);
        assert!(!net.clients[1].s.status().host);
        // The original host comes back with the same certificate: the stored host (C) stays.
        net.online(0);
        assert!(net.settle(120_000).is_some());
        assert_eq!(net.clients[0].s.effective_host(), Some(c));
    }

    #[test]
    fn hide_is_local_and_remove_is_for_everyone() {
        let mut net = start_net(61, CALM, 3);
        net.run(2_000);
        for c in 0..3 {
            for _ in 0..3 {
                let e = stroke(&mut net.rng, net.clients[c].s.board());
                net.edit(c, vec![e]).unwrap();
            }
        }
        assert!(net.settle(60_000).is_some());
        let b = author_key(1);
        net.act(0, |s, now| s.set_hidden(b, true, now));
        let by_b = |s: &Session| {
            s.visible_objects()
                .iter()
                .filter(|o| o.id.author == b)
                .count()
        };
        assert_eq!(by_b(&net.clients[0].s), 0);
        assert_eq!(by_b(&net.clients[2].s), 3);
        assert!(net.converged(), "hiding must not change the shared board");
        net.act(0, |s, now| s.set_hidden(b, false, now));
        assert_eq!(by_b(&net.clients[0].s), 3);
        // Only the host removes for everyone.
        assert_eq!(
            net.clients[2].s.remove_author(b, net.now).err(),
            Some(LocalError::NotAllowed)
        );
        let outs = net.clients[0].s.remove_author(b, net.now).unwrap();
        net.handle(0, outs);
        assert!(net.settle(60_000).is_some());
        for c in &net.clients {
            assert_eq!(by_b(&c.s), 0);
            assert_eq!(c.s.visible_objects().len(), 6);
        }
    }

    #[test]
    fn presenter_view_reaches_followers_only() {
        let mut net = start_net(71, CALM, 3);
        net.run(2_000);
        let a = author_key(0);
        let outs = net.clients[0].s.set_presenting(true, net.now).unwrap();
        net.handle(0, outs);
        net.run(2_000);
        assert_eq!(net.clients[1].s.status().presenting, Some(a));
        net.act(1, |s, now| s.set_following(true, now));
        net.clients[1].last_ui.clear();
        net.clients[2].last_ui.clear();
        net.act(0, |s, now| s.local_view(400, 800, 1_500, now));
        net.run(1_000);
        let views = |c: &Client| {
            c.last_ui
                .iter()
                .filter(|u| matches!(u, Ui::View { author, .. } if *author == a))
                .count()
        };
        assert_eq!(views(&net.clients[1]), 1);
        assert_eq!(views(&net.clients[2]), 0);
        let outs = net.clients[0].s.set_presenting(false, net.now).unwrap();
        net.handle(0, outs);
        net.run(2_000);
        assert_eq!(net.clients[1].s.status().presenting, None);
    }

    #[test]
    fn without_relay_nothing_is_sent_until_it_returns() {
        let cfg = Config {
            board: BOARD,
            session: 100,
            info: info(0),
            joined_at_ms: 0,
            relay: false,
        };
        let mut s = Session::new(cfg, Board::new(author_key(0)), 0);
        let list = vec![PeerInfo {
            session: 100,
            name: "User 0".into(),
            key: Some(format!("{:064x}", 1)),
        }];
        let sent = |outs: Vec<Outgoing>| -> Vec<Msg> {
            outs.into_iter()
                .filter_map(|o| match o {
                    Outgoing::RelaySend { payload, .. } => {
                        Some(codec::decode_relay(&payload).unwrap())
                    }
                    Outgoing::Ui(_) => None,
                })
                .collect()
        };
        let mut now = 0;
        let mut all = sent(s.start(now));
        all.extend(sent(s.on_presence(&list, now)));
        let outs = s.init_board("Plan", now).unwrap();
        all.extend(sent(outs));
        let mut rng = Rng(8);
        let e = stroke(&mut rng, s.board());
        all.extend(sent(s.local_edit(vec![e], now).unwrap()));
        for _ in 0..100 {
            now += 50;
            all.extend(sent(s.tick(now)));
        }
        assert!(
            all.is_empty(),
            "sent {} messages without a relay",
            all.len()
        );
        now += 50;
        let mut back = sent(s.set_relay_available(true, now));
        for _ in 0..100 {
            now += 50;
            back.extend(sent(s.tick(now)));
        }
        assert!(back.iter().any(|m| matches!(m, Msg::Hello(_))));
        let ops: usize = back
            .iter()
            .map(|m| match m {
                Msg::Ops(o) => o.ops.len(),
                _ => 0,
            })
            .sum();
        assert!(ops >= 1, "queued ops were not sent: {back:?}");
    }

    #[test]
    fn author_keys_are_bound_to_sessions() {
        let mut net = start_net(91, CALM, 3);
        net.run(3_000);
        // C replays an OPS message claiming to be B.
        let mut fake = Board::new(author_key(1));
        let e = stroke(&mut Rng(3), &fake);
        let ops = fake.local(vec![e]).unwrap();
        let (payload, _) = codec::pack_ops(&BOARD, author_key(1), &ops).unwrap();
        let from = net.peer_info(2);
        let dropped = net.clients[0].s.dropped();
        let outs = net.clients[0].s.on_relay_message(&from, &payload, net.now);
        net.handle(0, outs);
        assert_eq!(net.clients[0].s.dropped(), dropped + 1);
        assert_eq!(net.clients[0].s.board().visible_objects().len(), 0);
    }

    #[test]
    fn flood_is_dropped_with_bounded_memory() {
        let mut net = start_net(101, CALM, 2);
        net.run(2_000);
        let now = net.now;
        let s = &mut net.clients[0].s;
        // One session floods ops from its own author key.
        let evil = PeerInfo {
            session: 666,
            name: "x".into(),
            key: None,
        };
        let mut fake = Board::new(0xbad);
        let mut rng = Rng(5);
        let hello = Msg::Hello(Hello {
            board: BOARD,
            author: 0xbad,
            info: AuthorInfo {
                name: "x".into(),
                cert_key: None,
                shape: 0,
                color: 0,
            },
            vv: Vv::new(),
            host: false,
            joined_at_ms: 0,
            direct: false,
        });
        s.on_relay_message(&evil, &codec::encode_relay(&hello).unwrap(), now);
        for i in 0..90 {
            let edits: Vec<Edit> = (0..50).map(|_| stroke(&mut rng, &fake)).collect();
            let ops = fake.local(edits).unwrap();
            let (payload, n) = codec::pack_ops(&BOARD, 0xbad, &ops).unwrap();
            assert_eq!(n, ops.len());
            s.on_relay_message(&evil, &payload, now + i);
        }
        assert!(s.board().live_count() <= FLOOD_OPS as usize);
        assert!(s.dropped() > 3_000);
        // Many author keys from one session, unsolicited snapshot parts, many sessions.
        for k in 0..5_000u32 {
            let mut h = match &hello {
                Msg::Hello(h) => h.clone(),
                _ => unreachable!(),
            };
            h.author = 0x1_0000 + k;
            let p = codec::encode_relay(&Msg::Hello(h)).unwrap();
            let sender = PeerInfo {
                session: 1_000 + k,
                name: "x".into(),
                key: None,
            };
            s.on_relay_message(&sender, &p, now + 300);
            s.on_relay_message(&evil, &p, now + 300);
            let snap = Msg::Snap(Snap {
                board: BOARD,
                part: (k % 7) as u16 + 1,
                of: 9_000,
                vv: None,
                data: vec![0; 1_000],
            });
            s.on_relay_message(&sender, &codec::encode_relay(&snap).unwrap(), now + 300);
        }
        assert!(s.peers.len() <= MAX_PEERS);
        assert!(s.bound.len() <= MAX_BOUND);
        assert!(s.inbound.len() <= MAX_SNAP_INBOUND);
        let bytes: usize = s.inbound.values().map(|e| e.bytes).sum();
        assert!(bytes <= MAX_SNAP_INBOUND * MAX_SNAP_BYTES);
        assert!(s.gaps.len() <= MAX_GAPS);
        assert!(s.relay_ctrl.len() <= MAX_CTRL && s.snap_out.len() <= MAX_SNAP_OUT);
    }
}
