//! WB2: the wire codec for whiteboard messages.
//!
//! Peers transport (Mumble plugin data): the message kind travels separately and the payload is
//! `[FORMAT][body]`, at most 997 bytes. Relay transport: the payload is `[FORMAT][kind][body]`,
//! at most 60 KiB. Integers are LEB128 varints (signed ones zigzag), strings are a varint byte
//! length plus UTF-8, author keys are 4 fixed bytes (they are random, so a varint would only
//! grow them) and board ids 16 bytes. Decoding is strict: overlong varints, out-of-range values,
//! over-long strings, unknown tags and trailing bytes are all errors, and nothing panics.
//!
//! Decisions where the contract leaves a gap:
//! - The id of a Create and of a Clear is implicit (author, seq) of its op, see `model.rs`.
//! - OPS layout: board id, author, then a mode byte (0 raw, 1 deflated) and the rest of the body
//!   (cursor, op count, ops), so the board id stays readable without inflating.
//! - A single op larger than a payload cannot be sent: [`pack_ops`] reports `TooLarge` and the
//!   sync layer must send it over the relay or as a snapshot. Likewise a version vector that does
//!   not fit HELLO or SNAP part 0 (about 130 authors on peers) makes encoding fail.
//! - SNAP data is the rest of the payload; the parts concatenated form one deflated delta
//!   ([`encode_delta`] / [`decode_delta`]).
//! - LIVE has no "done" flag; a chunk without points can mark the end of a live stroke.
//! - Live, laser and cursor coordinates are quarter pixels like everything else.

use miniz_oxide::deflate::compress_to_vec;
use miniz_oxide::inflate::decompress_to_vec_with_limit;

use crate::model::{
    AuthorInfo, AuthorKey, BoardField, Body, Field, Fields, Geom, Host, Kind, MAX_ALLOWED_PER_OP,
    MAX_ALT_CHARS, MAX_COORD, MAX_KEY_CHARS, MAX_NAME_CHARS, MAX_POINTS, MAX_TEXT_CHARS,
    MAX_TITLE_CHARS, MAX_VV_ENTRIES, MAX_Z_LEN, ObjId, Op, Policy, Stamp, Vv, Write,
};

pub const FORMAT: u8 = 1;
pub const MAX_PEERS_PAYLOAD: usize = 997;
pub const MAX_RELAY_PAYLOAD: usize = 60 * 1024;
pub const KIND_HELLO: u8 = 0x01;
pub const KIND_OPS: u8 = 0x02;
pub const KIND_NEED: u8 = 0x03;
pub const KIND_DIGEST: u8 = 0x04;
pub const KIND_SNAP: u8 = 0x05;
pub const KIND_BOARD: u8 = 0x06;
pub const KIND_CURSOR: u8 = 0x10;
pub const KIND_LIVE: u8 = 0x11;
pub const KIND_LASER: u8 = 0x12;
pub const KIND_VIEW: u8 = 0x13;
pub const MAX_LIVE_POINTS: usize = 512;
pub const MAX_LASER_POINTS: usize = 256;
const MAX_OPS_PER_MSG: usize = 4_096;
const MAX_DELTA_WRITES: usize = 1 << 20;
/// Inflated OPS bodies stay far below this; it only bounds a hostile "deflate bomb".
const MAX_OPS_INFLATED: usize = 1 << 20;
/// A full delta of a 4 MiB board, with room for per-register stamps.
const MAX_DELTA_INFLATED: usize = 16 << 20;
const DEFLATE_LEVEL: u8 = 9;

pub type BoardId = [u8; 16];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    Peers,
    Relay,
}

impl Transport {
    pub fn max_payload(self) -> usize {
        match self {
            Transport::Peers => MAX_PEERS_PAYLOAD,
            Transport::Relay => MAX_RELAY_PAYLOAD,
        }
    }

    fn header_len(self) -> usize {
        match self {
            Transport::Peers => 1,
            Transport::Relay => 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hello {
    pub board: BoardId,
    pub author: AuthorKey,
    pub info: AuthorInfo,
    pub vv: Vv,
    pub host: bool,
    /// When this participant joined; the longest-present peer answers first.
    pub joined_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpsMsg {
    pub board: BoardId,
    /// Author of every op in `ops`.
    pub author: AuthorKey,
    pub ops: Vec<Op>,
    /// The author's last cursor position (quarter pixels).
    pub cursor: Option<(i32, i32)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Need {
    pub board: BoardId,
    pub author: AuthorKey,
    pub from_seq: u32,
    pub to_seq: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Digest {
    pub board: BoardId,
    pub vv_hash: u64,
    pub total_ops: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snap {
    pub board: BoardId,
    pub part: u16,
    pub of: u16,
    /// Sender's version vector, in part 0 only.
    pub vv: Option<Vv>,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardMsg {
    pub board: BoardId,
    pub title: String,
    pub host: AuthorKey,
    pub started_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub author: AuthorKey,
    pub x: i32,
    pub y: i32,
    /// 0 none, 1 drawing, 2 typing, 3 highlighting.
    pub status: u8,
    pub tool: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Live {
    pub author: AuthorKey,
    pub sid: u16,
    pub seq: u16,
    pub points: Vec<(i32, i32)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Laser {
    pub author: AuthorKey,
    pub points: Vec<(i32, i32)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub author: AuthorKey,
    pub x: i32,
    pub y: i32,
    pub zoom_permille: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    Hello(Hello),
    Ops(OpsMsg),
    Need(Need),
    Digest(Digest),
    Snap(Snap),
    Board(BoardMsg),
    Cursor(Cursor),
    Live(Live),
    Laser(Laser),
    View(View),
}

impl Msg {
    pub fn kind(&self) -> u8 {
        match self {
            Msg::Hello(_) => KIND_HELLO,
            Msg::Ops(_) => KIND_OPS,
            Msg::Need(_) => KIND_NEED,
            Msg::Digest(_) => KIND_DIGEST,
            Msg::Snap(_) => KIND_SNAP,
            Msg::Board(_) => KIND_BOARD,
            Msg::Cursor(_) => KIND_CURSOR,
            Msg::Live(_) => KIND_LIVE,
            Msg::Laser(_) => KIND_LASER,
            Msg::View(_) => KIND_VIEW,
        }
    }
}

/// Cursor and live strokes are too chatty for the peers channel. Laser is allowed there: without
/// a relay it is sent during a game burst.
fn relay_only(kind: u8) -> bool {
    matches!(kind, KIND_CURSOR | KIND_LIVE)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeError {
    /// The message does not fit the transport's payload limit.
    TooLarge,
    /// Ops of another author, or a Create/Clear whose id is not (author, seq).
    BadOp,
    /// The message kind is not allowed on this transport.
    WrongTransport,
    /// More entries than a decoder accepts.
    TooMany,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    Truncated,
    BadVarint,
    BadValue,
    BadUtf8,
    TooLong,
    UnknownFormat,
    UnknownKind,
    WrongTransport,
    TrailingBytes,
    TooLarge,
    Inflate,
}

// ---- primitives --------------------------------------------------------------------------------

struct W(Vec<u8>);

impl W {
    fn new() -> W {
        W(Vec::new())
    }

    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }

    fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }

    fn uv(&mut self, mut v: u64) {
        while v >= 0x80 {
            self.0.push((v as u8) | 0x80);
            v >>= 7;
        }
        self.0.push(v as u8);
    }

    fn iv(&mut self, v: i64) {
        self.uv(((v << 1) ^ (v >> 63)) as u64);
    }

    fn key(&mut self, k: AuthorKey) {
        self.0.extend_from_slice(&k.to_le_bytes());
    }

    fn str(&mut self, s: &str) {
        self.uv(s.len() as u64);
        self.0.extend_from_slice(s.as_bytes());
    }

    fn opt_str(&mut self, s: &Option<String>) {
        match s {
            Some(s) => {
                self.u8(1);
                self.str(s);
            }
            None => self.u8(0),
        }
    }

    fn board(&mut self, b: &BoardId) {
        self.0.extend_from_slice(b);
    }

    fn count(&mut self, n: usize, max: usize) -> Result<(), EncodeError> {
        if n > max {
            return Err(EncodeError::TooMany);
        }
        self.uv(n as u64);
        Ok(())
    }

    /// First point absolute, the rest as deltas.
    fn points(&mut self, pts: &[(i32, i32)], max: usize) -> Result<(), EncodeError> {
        self.count(pts.len(), max)?;
        let mut prev = (0i64, 0i64);
        for &(x, y) in pts {
            let (x, y) = (i64::from(x), i64::from(y));
            self.iv(x - prev.0);
            self.iv(y - prev.1);
            prev = (x, y);
        }
        Ok(())
    }

    fn vv(&mut self, vv: &Vv) -> Result<(), EncodeError> {
        self.count(vv.len(), MAX_VV_ENTRIES)?;
        for (&a, &s) in vv {
            self.key(a);
            self.uv(u64::from(s));
        }
        Ok(())
    }

    fn author_info(&mut self, a: &AuthorInfo) {
        self.str(&a.name);
        self.opt_str(&a.cert_key);
        self.u8(a.shape);
        self.u8(a.color);
    }

    fn field_value(&mut self, f: &Field) {
        match f {
            Field::Geom(g) => {
                for v in [g.x, g.y, g.w, g.h] {
                    self.iv(i64::from(v));
                }
            }
            Field::Ink(v) | Field::Width(v) | Field::Fill(v) => self.u8(*v),
            Field::Text(s) | Field::Z(s) | Field::Alt(s) => self.str(s),
            Field::Deleted(b) => self.bool(*b),
        }
    }

    /// A presence mask with one bit per field tag, then the values in tag order.
    fn fields(&mut self, f: &Fields) {
        let list = f.to_list();
        self.u8(list.iter().fold(0, |m, f| m | (1 << f.tag())));
        for f in &list {
            self.field_value(f);
        }
    }

    fn board_field_value(&mut self, f: &BoardField) {
        match f {
            BoardField::Title(t) => self.str(t),
            BoardField::Host(h) => {
                self.key(h.author);
                self.opt_str(&h.cert_key);
            }
            BoardField::Policy(p) => self.u8(p.to_u8()),
            BoardField::Allowed(k, v) => {
                self.str(k);
                self.bool(*v);
            }
            BoardField::LaserForViewers(v) => self.bool(*v),
            BoardField::Presenter(p) => match p {
                Some(a) => {
                    self.u8(1);
                    self.key(*a);
                }
                None => self.u8(0),
            },
        }
    }

    fn stamp(&mut self, s: Stamp) {
        self.uv(s.lamport);
        self.key(s.author);
        self.uv(u64::from(s.seq));
    }
}

struct R<'a> {
    buf: &'a [u8],
    pos: usize,
}

type DResult<T> = Result<T, DecodeError>;

impl<'a> R<'a> {
    fn new(buf: &'a [u8]) -> R<'a> {
        R { buf, pos: 0 }
    }

    fn take(&mut self, n: usize) -> DResult<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(DecodeError::Truncated)?;
        let out = self.buf.get(self.pos..end).ok_or(DecodeError::Truncated)?;
        self.pos = end;
        Ok(out)
    }

    fn rest(&mut self) -> &'a [u8] {
        let out = self.buf.get(self.pos..).unwrap_or(&[]);
        self.pos = self.buf.len();
        out
    }

    fn end(&self) -> DResult<()> {
        if self.pos == self.buf.len() {
            Ok(())
        } else {
            Err(DecodeError::TrailingBytes)
        }
    }

    fn u8(&mut self) -> DResult<u8> {
        Ok(self.take(1)?[0])
    }

    fn bool(&mut self) -> DResult<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DecodeError::BadValue),
        }
    }

    /// Canonical LEB128: at most 10 bytes, no overflow, no redundant trailing zero byte.
    fn uv(&mut self) -> DResult<u64> {
        let mut v = 0u64;
        for i in 0..10 {
            let b = self.u8()?;
            let part = u64::from(b & 0x7f);
            if i == 9 && b > 1 {
                return Err(DecodeError::BadVarint);
            }
            v |= part << (7 * i);
            if b & 0x80 == 0 {
                if i > 0 && b == 0 {
                    return Err(DecodeError::BadVarint);
                }
                return Ok(v);
            }
        }
        Err(DecodeError::BadVarint)
    }

    fn u32v(&mut self) -> DResult<u32> {
        u32::try_from(self.uv()?).map_err(|_| DecodeError::BadValue)
    }

    fn u16v(&mut self) -> DResult<u16> {
        u16::try_from(self.uv()?).map_err(|_| DecodeError::BadValue)
    }

    fn iv(&mut self) -> DResult<i64> {
        let u = self.uv()?;
        Ok(((u >> 1) as i64) ^ -((u & 1) as i64))
    }

    fn coord(&mut self) -> DResult<i32> {
        coord(self.iv()?)
    }

    fn key(&mut self) -> DResult<AuthorKey> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn board(&mut self) -> DResult<BoardId> {
        let mut out = [0u8; 16];
        out.copy_from_slice(self.take(16)?);
        Ok(out)
    }

    /// A string of at most `max_chars` characters (and so at most 4 bytes per character).
    fn str(&mut self, max_chars: usize) -> DResult<String> {
        let len = usize::try_from(self.uv()?).map_err(|_| DecodeError::TooLong)?;
        if len > max_chars.saturating_mul(4) {
            return Err(DecodeError::TooLong);
        }
        let s = std::str::from_utf8(self.take(len)?).map_err(|_| DecodeError::BadUtf8)?;
        if s.chars().nth(max_chars).is_some() {
            return Err(DecodeError::TooLong);
        }
        Ok(s.to_string())
    }

    fn opt_str(&mut self, max_chars: usize) -> DResult<Option<String>> {
        Ok(if self.bool()? {
            Some(self.str(max_chars)?)
        } else {
            None
        })
    }

    fn count(&mut self, max: usize) -> DResult<usize> {
        let n = usize::try_from(self.uv()?).map_err(|_| DecodeError::TooLong)?;
        if n > max {
            return Err(DecodeError::TooLong);
        }
        Ok(n)
    }

    fn points(&mut self, max: usize) -> DResult<Vec<(i32, i32)>> {
        let n = self.count(max)?;
        // Every point needs at least two bytes, so a short buffer cannot claim many points.
        if n > self.buf.len().saturating_sub(self.pos) / 2 {
            return Err(DecodeError::Truncated);
        }
        let mut out = Vec::with_capacity(n);
        let mut prev = (0i64, 0i64);
        for _ in 0..n {
            let x = prev.0 + self.iv()?.clamp(-(1 << 40), 1 << 40);
            let y = prev.1 + self.iv()?.clamp(-(1 << 40), 1 << 40);
            out.push((coord(x)?, coord(y)?));
            prev = (x, y);
        }
        Ok(out)
    }

    fn vv(&mut self) -> DResult<Vv> {
        let n = self.count(MAX_VV_ENTRIES)?;
        let mut vv = Vv::new();
        let mut last = None;
        for _ in 0..n {
            let a = self.key()?;
            // Strictly ascending keys: one canonical encoding, no duplicates.
            if last.is_some_and(|l| a <= l) {
                return Err(DecodeError::BadValue);
            }
            last = Some(a);
            let s = self.u32v()?;
            if s == 0 {
                return Err(DecodeError::BadValue);
            }
            vv.insert(a, s);
        }
        Ok(vv)
    }

    fn author_info(&mut self) -> DResult<AuthorInfo> {
        Ok(AuthorInfo {
            name: self.str(MAX_NAME_CHARS)?,
            cert_key: self.opt_str(MAX_KEY_CHARS)?,
            shape: self.u8()?,
            color: self.u8()?,
        })
    }

    fn field_value(&mut self, tag: u8) -> DResult<Field> {
        Ok(match tag {
            0 => Field::Geom(Geom {
                x: self.coord()?,
                y: self.coord()?,
                w: self.coord()?,
                h: self.coord()?,
            }),
            1 => Field::Ink(self.u8()?),
            2 => Field::Width(self.u8()?),
            3 => Field::Fill(self.u8()?),
            4 => Field::Text(self.str(MAX_TEXT_CHARS)?),
            5 => Field::Z(self.str(MAX_Z_LEN)?),
            6 => Field::Alt(self.str(MAX_ALT_CHARS)?),
            7 => Field::Deleted(self.bool()?),
            _ => return Err(DecodeError::UnknownKind),
        })
    }

    fn fields(&mut self) -> DResult<Fields> {
        let mask = self.u8()?;
        let mut out = Fields::default();
        for tag in 0..8 {
            if mask & (1 << tag) != 0 {
                out.set(self.field_value(tag)?);
            }
        }
        Ok(out)
    }

    fn board_field_value(&mut self, tag: u8) -> DResult<BoardField> {
        Ok(match tag {
            0 => BoardField::Title(self.str(MAX_TITLE_CHARS)?),
            1 => BoardField::Host(Host {
                author: self.key()?,
                cert_key: self.opt_str(MAX_KEY_CHARS)?,
            }),
            2 => BoardField::Policy(Policy::from_u8(self.u8()?).ok_or(DecodeError::BadValue)?),
            3 => BoardField::Allowed(self.str(MAX_KEY_CHARS)?, self.bool()?),
            4 => BoardField::LaserForViewers(self.bool()?),
            5 => BoardField::Presenter(if self.bool()? {
                Some(self.key()?)
            } else {
                None
            }),
            _ => return Err(DecodeError::UnknownKind),
        })
    }

    fn kind(&mut self) -> DResult<Kind> {
        Kind::from_u8(self.u8()?).ok_or(DecodeError::BadValue)
    }

    fn stamp(&mut self) -> DResult<Stamp> {
        Ok(Stamp {
            lamport: self.uv()?,
            author: self.key()?,
            seq: self.u32v()?,
        })
    }
}

fn coord(v: i64) -> DResult<i32> {
    if (-i64::from(MAX_COORD)..=i64::from(MAX_COORD)).contains(&v) {
        Ok(v as i32)
    } else {
        Err(DecodeError::BadValue)
    }
}

fn varint_len(v: u64) -> usize {
    let mut n = 1;
    let mut v = v >> 7;
    while v > 0 {
        n += 1;
        v >>= 7;
    }
    n
}

// ---- ops ---------------------------------------------------------------------------------------

const OP_CREATE: u8 = 0;
const OP_SET: u8 = 1;
const OP_CLEAR: u8 = 2;
const OP_REVOKE: u8 = 3;
const OP_BOARD: u8 = 4;
const OP_AUTHOR: u8 = 5;

fn board_fields(w: &mut W, fields: &[BoardField]) -> Result<(), EncodeError> {
    w.count(fields.len(), MAX_ALLOWED_PER_OP + 6)?;
    for f in fields {
        w.u8(f.tag());
        w.board_field_value(f);
    }
    Ok(())
}

/// One op; seq and lamport are coded relative to the previous op of the batch.
fn write_op(w: &mut W, op: &Op, prev: Option<&Op>) -> Result<(), EncodeError> {
    let own_id = ObjId {
        author: op.author,
        counter: op.seq,
    };
    match prev {
        Some(p) => {
            w.iv(i64::from(op.seq) - i64::from(p.seq));
            let diff = i128::from(op.lamport) - i128::from(p.lamport);
            w.iv(i64::try_from(diff).map_err(|_| EncodeError::BadOp)?);
        }
        None => {
            w.uv(u64::from(op.seq));
            w.uv(op.lamport);
        }
    }
    match &op.body {
        Body::Create {
            id,
            kind,
            base,
            fields,
        } => {
            if *id != own_id {
                return Err(EncodeError::BadOp);
            }
            w.u8(OP_CREATE);
            w.u8(kind.to_u8());
            w.points(base, MAX_POINTS)?;
            w.fields(fields);
        }
        Body::Set { id, fields } => {
            w.u8(OP_SET);
            w.key(id.author);
            w.uv(u64::from(id.counter));
            w.fields(fields);
        }
        Body::Clear { clear_id, vv } => {
            if *clear_id != own_id {
                return Err(EncodeError::BadOp);
            }
            w.u8(OP_CLEAR);
            w.vv(vv)?;
        }
        Body::ClearRevoke { clear_id } => {
            w.u8(OP_REVOKE);
            w.key(clear_id.author);
            w.uv(u64::from(clear_id.counter));
        }
        Body::Board { fields } => {
            w.u8(OP_BOARD);
            board_fields(w, fields)?;
        }
        Body::Author(info) => {
            w.u8(OP_AUTHOR);
            w.author_info(info);
        }
    }
    Ok(())
}

fn read_op(r: &mut R, author: AuthorKey, prev: Option<(u32, u64)>) -> DResult<Op> {
    let (seq, lamport) = match prev {
        Some((ps, pl)) => {
            let seq = i64::from(ps) + r.iv()?;
            let lamport = i128::from(pl) + i128::from(r.iv()?);
            (
                u32::try_from(seq).map_err(|_| DecodeError::BadValue)?,
                u64::try_from(lamport).map_err(|_| DecodeError::BadValue)?,
            )
        }
        None => (r.u32v()?, r.uv()?),
    };
    let own_id = ObjId {
        author,
        counter: seq,
    };
    let body = match r.u8()? {
        OP_CREATE => Body::Create {
            id: own_id,
            kind: r.kind()?,
            base: r.points(MAX_POINTS)?,
            fields: r.fields()?,
        },
        OP_SET => Body::Set {
            id: ObjId {
                author: r.key()?,
                counter: r.u32v()?,
            },
            fields: r.fields()?,
        },
        OP_CLEAR => Body::Clear {
            clear_id: own_id,
            vv: r.vv()?,
        },
        OP_REVOKE => Body::ClearRevoke {
            clear_id: ObjId {
                author: r.key()?,
                counter: r.u32v()?,
            },
        },
        OP_BOARD => {
            let n = r.count(MAX_ALLOWED_PER_OP + 6)?;
            let mut fields = Vec::new();
            for _ in 0..n {
                let tag = r.u8()?;
                fields.push(r.board_field_value(tag)?);
            }
            Body::Board { fields }
        }
        OP_AUTHOR => Body::Author(r.author_info()?),
        _ => return Err(DecodeError::UnknownKind),
    };
    Ok(Op {
        author,
        seq,
        lamport,
        body,
    })
}

fn cursor_part(w: &mut W, cursor: Option<(i32, i32)>) {
    match cursor {
        Some((x, y)) => {
            w.u8(1);
            w.iv(i64::from(x));
            w.iv(i64::from(y));
        }
        None => w.u8(0),
    }
}

/// Encodes each op of a batch on its own (deltas against the previous op).
fn op_chunks(author: AuthorKey, ops: &[Op]) -> Result<Vec<Vec<u8>>, EncodeError> {
    let mut out = Vec::with_capacity(ops.len());
    for (i, op) in ops.iter().enumerate() {
        if op.author != author {
            return Err(EncodeError::BadOp);
        }
        let mut w = W::new();
        write_op(&mut w, op, i.checked_sub(1).map(|p| &ops[p]))?;
        out.push(w.0);
    }
    Ok(out)
}

/// OPS body from pre-encoded op chunks; deflated when that is smaller.
fn ops_body(board: &BoardId, author: AuthorKey, cursor: &[u8], chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut inner = W::new();
    inner.0.extend_from_slice(cursor);
    inner.uv(chunks.len() as u64);
    for c in chunks {
        inner.0.extend_from_slice(c);
    }
    let mut w = W::new();
    w.board(board);
    w.key(author);
    let packed = compress_to_vec(&inner.0, DEFLATE_LEVEL);
    if packed.len() < inner.0.len() {
        w.u8(1);
        w.0.extend_from_slice(&packed);
    } else {
        w.u8(0);
        w.0.extend_from_slice(&inner.0);
    }
    w.0
}

fn read_ops(r: &mut R) -> DResult<OpsMsg> {
    let board = r.board()?;
    let author = r.key()?;
    let inflated;
    let inner = match r.u8()? {
        0 => r.rest(),
        1 => {
            inflated = decompress_to_vec_with_limit(r.rest(), MAX_OPS_INFLATED)
                .map_err(|_| DecodeError::Inflate)?;
            &inflated[..]
        }
        _ => return Err(DecodeError::BadValue),
    };
    let mut r = R::new(inner);
    let cursor = if r.bool()? {
        Some((r.coord()?, r.coord()?))
    } else {
        None
    };
    let n = r.count(MAX_OPS_PER_MSG)?;
    let mut ops: Vec<Op> = Vec::new();
    for _ in 0..n {
        let prev = ops.last().map(|p| (p.seq, p.lamport));
        ops.push(read_op(&mut r, author, prev)?);
    }
    r.end()?;
    Ok(OpsMsg {
        board,
        author,
        ops,
        cursor,
    })
}

/// Packs as many leading ops as fit into one OPS payload for `transport`. Returns the payload
/// (framed for the transport, kind [`KIND_OPS`]) and how many ops it holds. `TooLarge` means
/// the first op alone does not fit.
pub fn pack_ops(
    board: &BoardId,
    author: AuthorKey,
    ops: &[Op],
    cursor: Option<(i32, i32)>,
    transport: Transport,
) -> Result<(Vec<u8>, usize), EncodeError> {
    let chunks = op_chunks(author, ops)?;
    let mut cur = W::new();
    cursor_part(&mut cur, cursor);
    let room = transport.max_payload() - transport.header_len();
    // Raw sizes add up, so the raw fit is found by counting; deflate only extends it.
    let fixed = 16 + 4 + 1 + cur.0.len();
    let mut raw_fit = 0;
    let mut sum = 0;
    for (i, c) in chunks.iter().enumerate() {
        sum += c.len();
        if fixed + varint_len(i as u64 + 1) + sum > room {
            break;
        }
        raw_fit = i + 1;
    }
    let fits = |n: usize| ops_body(board, author, &cur.0, &chunks[..n]).len() <= room;
    let (mut lo, mut hi) = (raw_fit, chunks.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if fits(mid) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    if lo == 0 {
        return Err(EncodeError::TooLarge);
    }
    let body = ops_body(board, author, &cur.0, &chunks[..lo]);
    Ok((frame(transport, KIND_OPS, &body), lo))
}

// ---- deltas and snapshots ----------------------------------------------------------------------

const W_OBJECT: u8 = 0;
const W_FIELD: u8 = 1;
const W_CLEAR: u8 = 2;
const W_REVOKE: u8 = 3;
const W_BOARD: u8 = 4;
const W_AUTHOR: u8 = 5;

/// A state delta as one deflated byte string (the SNAP data of all parts together).
pub fn encode_delta(writes: &[Write]) -> Result<Vec<u8>, EncodeError> {
    let mut w = W::new();
    w.count(writes.len(), MAX_DELTA_WRITES)?;
    for x in writes {
        match x {
            Write::Object { at, kind, base } => {
                w.u8(W_OBJECT);
                w.stamp(*at);
                w.u8(kind.to_u8());
                w.points(base, MAX_POINTS)?;
            }
            Write::Field { id, at, field } => {
                w.u8(W_FIELD);
                w.stamp(*at);
                w.key(id.author);
                w.uv(u64::from(id.counter));
                w.u8(field.tag());
                w.field_value(field);
            }
            Write::Clear { at, vv } => {
                w.u8(W_CLEAR);
                w.stamp(*at);
                w.vv(vv)?;
            }
            Write::Revoke { clear_id, at } => {
                w.u8(W_REVOKE);
                w.stamp(*at);
                w.key(clear_id.author);
                w.uv(u64::from(clear_id.counter));
            }
            Write::Board { at, field } => {
                w.u8(W_BOARD);
                w.stamp(*at);
                w.u8(field.tag());
                w.board_field_value(field);
            }
            Write::Author { at, info } => {
                w.u8(W_AUTHOR);
                w.stamp(*at);
                w.author_info(info);
            }
        }
    }
    Ok(compress_to_vec(&w.0, DEFLATE_LEVEL))
}

pub fn decode_delta(data: &[u8]) -> DResult<Vec<Write>> {
    let raw =
        decompress_to_vec_with_limit(data, MAX_DELTA_INFLATED).map_err(|_| DecodeError::Inflate)?;
    let mut r = R::new(&raw);
    let n = r.count(MAX_DELTA_WRITES)?;
    let mut out = Vec::new();
    for _ in 0..n {
        let tag = r.u8()?;
        let at = r.stamp()?;
        out.push(match tag {
            W_OBJECT => Write::Object {
                at,
                kind: r.kind()?,
                base: r.points(MAX_POINTS)?,
            },
            W_FIELD => {
                let id = ObjId {
                    author: r.key()?,
                    counter: r.u32v()?,
                };
                let ftag = r.u8()?;
                Write::Field {
                    id,
                    at,
                    field: r.field_value(ftag)?,
                }
            }
            W_CLEAR => Write::Clear { at, vv: r.vv()? },
            W_REVOKE => Write::Revoke {
                clear_id: ObjId {
                    author: r.key()?,
                    counter: r.u32v()?,
                },
                at,
            },
            W_BOARD => {
                let ftag = r.u8()?;
                Write::Board {
                    at,
                    field: r.board_field_value(ftag)?,
                }
            }
            W_AUTHOR => Write::Author {
                at,
                info: r.author_info()?,
            },
            _ => return Err(DecodeError::UnknownKind),
        });
    }
    r.end()?;
    Ok(out)
}

/// Splits a deflated delta into SNAP messages that each fit `transport`; part 0 carries `vv`.
pub fn snap_messages(
    board: &BoardId,
    vv: &Vv,
    data: &[u8],
    transport: Transport,
) -> Result<Vec<Snap>, EncodeError> {
    let mut vv_w = W::new();
    vv_w.vv(vv)?;
    // Header: format (+ kind), board id, part and count (3-byte varints at most).
    let header = transport.header_len() + 16 + 3 + 3;
    let room = transport.max_payload();
    let first = room
        .checked_sub(header + vv_w.0.len())
        .filter(|&n| n > 0)
        .ok_or(EncodeError::TooLarge)?;
    let rest_room = room - header;
    let first_len = first.min(data.len());
    let mut parts = vec![&data[..first_len]];
    parts.extend(data[first_len..].chunks(rest_room));
    let of = u16::try_from(parts.len()).map_err(|_| EncodeError::TooLarge)?;
    Ok(parts
        .into_iter()
        .enumerate()
        .map(|(i, d)| Snap {
            board: *board,
            part: i as u16,
            of,
            vv: (i == 0).then(|| vv.clone()),
            data: d.to_vec(),
        })
        .collect())
}

// ---- messages ----------------------------------------------------------------------------------

fn body(msg: &Msg) -> Result<Vec<u8>, EncodeError> {
    let mut w = W::new();
    match msg {
        Msg::Hello(h) => {
            w.board(&h.board);
            w.key(h.author);
            w.author_info(&h.info);
            w.vv(&h.vv)?;
            w.u8(u8::from(h.host));
            w.uv(h.joined_at_ms);
        }
        Msg::Ops(m) => {
            let chunks = op_chunks(m.author, &m.ops)?;
            if chunks.len() > MAX_OPS_PER_MSG {
                return Err(EncodeError::TooMany);
            }
            let mut cur = W::new();
            cursor_part(&mut cur, m.cursor);
            return Ok(ops_body(&m.board, m.author, &cur.0, &chunks));
        }
        Msg::Need(n) => {
            w.board(&n.board);
            w.key(n.author);
            w.uv(u64::from(n.from_seq));
            w.uv(u64::from(n.to_seq));
        }
        Msg::Digest(d) => {
            w.board(&d.board);
            w.0.extend_from_slice(&d.vv_hash.to_le_bytes());
            w.uv(u64::from(d.total_ops));
        }
        Msg::Snap(s) => {
            w.board(&s.board);
            w.uv(u64::from(s.part));
            w.uv(u64::from(s.of));
            if s.part == 0 {
                w.vv(s.vv.as_ref().ok_or(EncodeError::BadOp)?)?;
            }
            w.0.extend_from_slice(&s.data);
        }
        Msg::Board(b) => {
            w.board(&b.board);
            w.str(&b.title);
            w.key(b.host);
            w.uv(b.started_ms);
        }
        Msg::Cursor(c) => {
            w.key(c.author);
            w.iv(i64::from(c.x));
            w.iv(i64::from(c.y));
            w.u8(c.status);
            w.u8(c.tool);
        }
        Msg::Live(l) => {
            w.key(l.author);
            w.uv(u64::from(l.sid));
            w.uv(u64::from(l.seq));
            w.points(&l.points, MAX_LIVE_POINTS)?;
        }
        Msg::Laser(l) => {
            w.key(l.author);
            w.points(&l.points, MAX_LASER_POINTS)?;
        }
        Msg::View(v) => {
            w.key(v.author);
            w.iv(i64::from(v.x));
            w.iv(i64::from(v.y));
            w.uv(u64::from(v.zoom_permille));
        }
    }
    Ok(w.0)
}

fn frame(transport: Transport, kind: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 2);
    out.push(FORMAT);
    if transport == Transport::Relay {
        out.push(kind);
    }
    out.extend_from_slice(body);
    out
}

fn encode(msg: &Msg, transport: Transport) -> Result<Vec<u8>, EncodeError> {
    if transport == Transport::Peers && relay_only(msg.kind()) {
        return Err(EncodeError::WrongTransport);
    }
    let out = frame(transport, msg.kind(), &body(msg)?);
    if out.len() > transport.max_payload() {
        return Err(EncodeError::TooLarge);
    }
    Ok(out)
}

/// Encodes for `peers.messages`: returns the message kind and the payload.
pub fn encode_peers(msg: &Msg) -> Result<(u8, Vec<u8>), EncodeError> {
    Ok((msg.kind(), encode(msg, Transport::Peers)?))
}

/// Encodes for the relay: the payload carries the kind itself.
pub fn encode_relay(msg: &Msg) -> Result<Vec<u8>, EncodeError> {
    encode(msg, Transport::Relay)
}

fn read_body(kind: u8, r: &mut R) -> DResult<Msg> {
    let msg = match kind {
        KIND_HELLO => Msg::Hello(Hello {
            board: r.board()?,
            author: r.key()?,
            info: r.author_info()?,
            vv: r.vv()?,
            host: r.bool()?,
            joined_at_ms: r.uv()?,
        }),
        KIND_OPS => Msg::Ops(read_ops(r)?),
        KIND_NEED => {
            let n = Need {
                board: r.board()?,
                author: r.key()?,
                from_seq: r.u32v()?,
                to_seq: r.u32v()?,
            };
            if n.from_seq == 0 || n.from_seq > n.to_seq {
                return Err(DecodeError::BadValue);
            }
            Msg::Need(n)
        }
        KIND_DIGEST => Msg::Digest(Digest {
            board: r.board()?,
            vv_hash: {
                let b = r.take(8)?;
                u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
            },
            total_ops: r.u32v()?,
        }),
        KIND_SNAP => {
            let board = r.board()?;
            let part = r.u16v()?;
            let of = r.u16v()?;
            if part >= of {
                return Err(DecodeError::BadValue);
            }
            let vv = if part == 0 { Some(r.vv()?) } else { None };
            Msg::Snap(Snap {
                board,
                part,
                of,
                vv,
                data: r.rest().to_vec(),
            })
        }
        KIND_BOARD => Msg::Board(BoardMsg {
            board: r.board()?,
            title: r.str(MAX_TITLE_CHARS)?,
            host: r.key()?,
            started_ms: r.uv()?,
        }),
        KIND_CURSOR => {
            let c = Cursor {
                author: r.key()?,
                x: r.coord()?,
                y: r.coord()?,
                status: r.u8()?,
                tool: r.u8()?,
            };
            if c.status > 3 {
                return Err(DecodeError::BadValue);
            }
            Msg::Cursor(c)
        }
        KIND_LIVE => Msg::Live(Live {
            author: r.key()?,
            sid: r.u16v()?,
            seq: r.u16v()?,
            points: r.points(MAX_LIVE_POINTS)?,
        }),
        KIND_LASER => Msg::Laser(Laser {
            author: r.key()?,
            points: r.points(MAX_LASER_POINTS)?,
        }),
        KIND_VIEW => Msg::View(View {
            author: r.key()?,
            x: r.coord()?,
            y: r.coord()?,
            zoom_permille: r.u32v()?,
        }),
        _ => return Err(DecodeError::UnknownKind),
    };
    r.end()?;
    Ok(msg)
}

fn check_format(payload: &[u8], transport: Transport) -> DResult<&[u8]> {
    if payload.len() > transport.max_payload() {
        return Err(DecodeError::TooLarge);
    }
    match payload.split_first() {
        Some((&FORMAT, rest)) => Ok(rest),
        Some(_) => Err(DecodeError::UnknownFormat),
        None => Err(DecodeError::Truncated),
    }
}

/// Decodes a `peers.messages` payload of the given kind.
pub fn decode_peers(kind: u8, payload: &[u8]) -> DResult<Msg> {
    let rest = check_format(payload, Transport::Peers)?;
    if relay_only(kind) {
        return Err(DecodeError::WrongTransport);
    }
    read_body(kind, &mut R::new(rest))
}

/// Decodes a relay payload (`[FORMAT][kind][body]`).
pub fn decode_relay(payload: &[u8]) -> DResult<Msg> {
    let rest = check_format(payload, Transport::Relay)?;
    let (&kind, body) = rest.split_first().ok_or(DecodeError::Truncated)?;
    read_body(kind, &mut R::new(body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Board, Edit, Kind, prepare_stroke};
    use proptest::prelude::*;

    const BOARD: BoardId = [7; 16];

    fn rt(msg: Msg) {
        let relay = encode_relay(&msg).unwrap();
        assert_eq!(decode_relay(&relay).unwrap(), msg);
        if !relay_only(msg.kind()) {
            let (kind, peers) = encode_peers(&msg).unwrap();
            assert_eq!(decode_peers(kind, &peers).unwrap(), msg);
        }
    }

    fn info() -> AuthorInfo {
        AuthorInfo {
            name: "Ann ü".into(),
            cert_key: Some("abc".into()),
            shape: 2,
            color: 5,
        }
    }

    fn all_fields() -> Fields {
        Fields {
            geom: Some(Geom {
                x: -MAX_COORD,
                y: MAX_COORD,
                w: 3,
                h: -4,
            }),
            ink: Some(5),
            width: Some(2),
            fill: Some(4),
            text: Some("note ✓".into()),
            z: Some("mn".into()),
            alt: Some("alt".into()),
            deleted: Some(true),
        }
    }

    fn sample_ops(author: AuthorKey) -> Vec<Op> {
        let mut vv = Vv::new();
        vv.insert(5, 9);
        vv.insert(author, 2);
        let bodies = vec![
            Body::Create {
                id: ObjId { author, counter: 1 },
                kind: Kind::Stroke,
                base: vec![(0, 0), (10, -3), (MAX_COORD, MAX_COORD)],
                fields: all_fields(),
            },
            Body::Set {
                id: ObjId {
                    author: 5,
                    counter: 77,
                },
                fields: Fields {
                    deleted: Some(false),
                    ..Fields::default()
                },
            },
            Body::Clear {
                clear_id: ObjId { author, counter: 3 },
                vv,
            },
            Body::ClearRevoke {
                clear_id: ObjId { author, counter: 3 },
            },
            Body::Board {
                fields: vec![
                    BoardField::Title("T".into()),
                    BoardField::Host(Host {
                        author,
                        cert_key: None,
                    }),
                    BoardField::Policy(Policy::HostOnly),
                    BoardField::Allowed("key".into(), false),
                    BoardField::LaserForViewers(true),
                    BoardField::Presenter(None),
                    BoardField::Presenter(Some(author)),
                ],
            },
            Body::Author(info()),
        ];
        bodies
            .into_iter()
            .enumerate()
            .map(|(i, body)| Op {
                author,
                seq: i as u32 + 1,
                lamport: [5, 4, 900, 901, 2, 3][i],
                body,
            })
            .collect()
    }

    #[test]
    fn every_message_round_trips() {
        let mut vv = Vv::new();
        vv.insert(1, 2);
        vv.insert(u32::MAX, u32::MAX);
        rt(Msg::Hello(Hello {
            board: BOARD,
            author: 9,
            info: info(),
            vv: vv.clone(),
            host: true,
            joined_at_ms: 1_700_000_000_000,
        }));
        rt(Msg::Ops(OpsMsg {
            board: BOARD,
            author: 42,
            ops: sample_ops(42),
            cursor: Some((-5, 6)),
        }));
        rt(Msg::Ops(OpsMsg {
            board: BOARD,
            author: 42,
            ops: vec![],
            cursor: None,
        }));
        rt(Msg::Need(Need {
            board: BOARD,
            author: 3,
            from_seq: 4,
            to_seq: 9,
        }));
        rt(Msg::Digest(Digest {
            board: BOARD,
            vv_hash: u64::MAX - 3,
            total_ops: 77,
        }));
        rt(Msg::Snap(Snap {
            board: BOARD,
            part: 0,
            of: 2,
            vv: Some(vv),
            data: vec![1, 2, 3],
        }));
        rt(Msg::Snap(Snap {
            board: BOARD,
            part: 1,
            of: 2,
            vv: None,
            data: vec![4],
        }));
        rt(Msg::Board(BoardMsg {
            board: BOARD,
            title: "Sprint".into(),
            host: 8,
            started_ms: 12,
        }));
        rt(Msg::Cursor(Cursor {
            author: 1,
            x: -100,
            y: 200,
            status: 3,
            tool: 4,
        }));
        rt(Msg::Live(Live {
            author: 1,
            sid: 65535,
            seq: 3,
            points: vec![(1, 2), (-3, 4)],
        }));
        rt(Msg::Laser(Laser {
            author: 1,
            points: vec![(0, 0)],
        }));
        rt(Msg::View(View {
            author: 1,
            x: 1,
            y: -1,
            zoom_permille: 1500,
        }));
    }

    #[test]
    fn transport_rules() {
        let cursor = Msg::Cursor(Cursor {
            author: 1,
            x: 0,
            y: 0,
            status: 0,
            tool: 0,
        });
        assert_eq!(encode_peers(&cursor), Err(EncodeError::WrongTransport));
        let relay = encode_relay(&cursor).unwrap();
        assert_eq!(
            decode_peers(KIND_CURSOR, &relay[..1]),
            Err(DecodeError::WrongTransport)
        );
        let (kind, mut payload) = encode_peers(&Msg::Need(Need {
            board: BOARD,
            author: 1,
            from_seq: 1,
            to_seq: 1,
        }))
        .unwrap();
        assert_eq!(decode_peers(0x7f, &payload), Err(DecodeError::UnknownKind));
        payload[0] = 2;
        assert_eq!(
            decode_peers(kind, &payload),
            Err(DecodeError::UnknownFormat)
        );
        payload[0] = FORMAT;
        payload.push(0);
        assert_eq!(
            decode_peers(kind, &payload),
            Err(DecodeError::TrailingBytes)
        );
        assert_eq!(
            decode_peers(kind, &vec![FORMAT; MAX_PEERS_PAYLOAD + 1]),
            Err(DecodeError::TooLarge)
        );
    }

    #[test]
    fn strict_primitives() {
        // Overlong zero, an 11-byte varint and a u64 overflow.
        for bad in [
            &[0x80, 0x00][..],
            &[0xff; 11][..],
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02][..],
        ] {
            assert_eq!(R::new(bad).uv(), Err(DecodeError::BadVarint));
        }
        let mut w = W::new();
        w.uv(u64::MAX);
        w.iv(i64::MIN);
        w.iv(-1);
        let mut r = R::new(&w.0);
        assert_eq!(r.uv(), Ok(u64::MAX));
        assert_eq!(r.iv(), Ok(i64::MIN));
        assert_eq!(r.iv(), Ok(-1));
        // Strings over their cap and invalid UTF-8.
        let mut w = W::new();
        w.str(&"x".repeat(MAX_TITLE_CHARS + 1));
        assert_eq!(R::new(&w.0).str(MAX_TITLE_CHARS), Err(DecodeError::TooLong));
        assert_eq!(R::new(&[2, 0xc3, 0x28]).str(10), Err(DecodeError::BadUtf8));
        // Unsorted version vector.
        let mut w = W::new();
        w.uv(2);
        w.key(5);
        w.uv(1);
        w.key(4);
        w.uv(1);
        assert_eq!(R::new(&w.0).vv(), Err(DecodeError::BadValue));
    }

    #[test]
    fn encoder_refuses_inconsistent_ops() {
        let mut ops = sample_ops(1);
        ops[0].author = 2;
        assert_eq!(
            pack_ops(&BOARD, 1, &ops, None, Transport::Peers),
            Err(EncodeError::BadOp)
        );
        let mut ops = sample_ops(1);
        if let Body::Create { id, .. } = &mut ops[0].body {
            id.counter = 99;
        }
        assert_eq!(
            pack_ops(&BOARD, 1, &ops, None, Transport::Peers),
            Err(EncodeError::BadOp)
        );
    }

    fn pen_stroke(n: usize, phase: f64) -> Vec<(f64, f64)> {
        // Handwriting-like loops: a slow drift with a wobble, sampled every ~2 px.
        (0..n)
            .map(|i| {
                let t = i as f64 / 10.0 + phase;
                (
                    300.0 + i as f64 * 1.2 + 12.0 * t.cos(),
                    200.0 + 25.0 * (t * 0.7).sin(),
                )
            })
            .collect()
    }

    fn stroke_ops(board: &mut Board, count: usize) -> Vec<Op> {
        let mut out = Vec::new();
        for i in 0..count {
            let (geom, base) = prepare_stroke(&pen_stroke(200, i as f64)).unwrap();
            out.extend(
                board
                    .local(vec![Edit::Create {
                        kind: Kind::Stroke,
                        base,
                        fields: Fields {
                            geom: Some(geom),
                            ink: Some(1),
                            width: Some(1),
                            z: Some("n".into()),
                            ..Fields::default()
                        },
                    }])
                    .unwrap(),
            );
        }
        out
    }

    #[test]
    fn typical_pen_stroke_is_small() {
        let mut board = Board::new(0xdead_beef);
        let ops = stroke_ops(&mut board, 1);
        let (payload, n) =
            pack_ops(&BOARD, board.me(), &ops, Some((10, 10)), Transport::Peers).unwrap();
        assert_eq!(n, 1);
        // The stroke itself (what each extra stroke adds to a batch) stays within 120 bytes;
        // the message header (board id, author, cursor) is shared by every op of a batch.
        let op_bytes = op_chunks(board.me(), &ops).unwrap()[0].len();
        assert!(op_bytes <= 120, "{op_bytes} bytes per stroke");
        assert!(payload.len() <= op_bytes + 30, "{} bytes", payload.len());
    }

    #[test]
    fn packer_fills_messages() {
        let mut board = Board::new(3);
        let ops = stroke_ops(&mut board, 40);
        let mut rest = &ops[..];
        let mut got = Vec::new();
        let mut messages = 0;
        while !rest.is_empty() {
            let (payload, n) = pack_ops(&BOARD, 3, rest, None, Transport::Peers).unwrap();
            assert!(payload.len() <= MAX_PEERS_PAYLOAD);
            let Msg::Ops(m) = decode_peers(KIND_OPS, &payload).unwrap() else {
                panic!("not ops");
            };
            got.extend(m.ops);
            rest = &rest[n..];
            messages += 1;
        }
        assert_eq!(got, ops);
        assert!(messages <= 8, "{messages} messages for 40 strokes");
        let (relay, n) = pack_ops(&BOARD, 3, &ops, None, Transport::Relay).unwrap();
        assert_eq!(n, ops.len());
        assert!(relay.len() <= MAX_RELAY_PAYLOAD);
        assert!(matches!(decode_relay(&relay), Ok(Msg::Ops(m)) if m.ops == ops));
    }

    #[test]
    fn oversized_op_is_reported() {
        let op = Op {
            author: 1,
            seq: 1,
            lamport: 1,
            body: Body::Create {
                id: ObjId {
                    author: 1,
                    counter: 1,
                },
                kind: Kind::Stroke,
                base: {
                    // Pseudo-random points do not deflate, so the op stays large.
                    let mut seed = 0x2545_f491_u32;
                    let mut next = move || {
                        seed ^= seed << 13;
                        seed ^= seed >> 17;
                        seed ^= seed << 5;
                        (seed % 1_000_000) as i32
                    };
                    (0..MAX_POINTS).map(|_| (next(), next())).collect()
                },
                fields: Fields::default(),
            },
        };
        assert_eq!(
            pack_ops(&BOARD, 1, &[op], None, Transport::Peers),
            Err(EncodeError::TooLarge)
        );
    }

    #[test]
    fn snapshot_round_trip() {
        let mut board = Board::new(3);
        stroke_ops(&mut board, 60);
        board.local_clear().unwrap();
        let data = encode_delta(&board.delta_since(&Vv::new())).unwrap();
        for transport in [Transport::Peers, Transport::Relay] {
            let snaps = snap_messages(&BOARD, board.vv(), &data, transport).unwrap();
            let mut joined = Vec::new();
            let mut vv = None;
            for s in snaps {
                let payload = match transport {
                    Transport::Peers => encode_peers(&Msg::Snap(s)).unwrap().1,
                    Transport::Relay => encode_relay(&Msg::Snap(s)).unwrap(),
                };
                assert!(payload.len() <= transport.max_payload());
                let msg = match transport {
                    Transport::Peers => decode_peers(KIND_SNAP, &payload),
                    Transport::Relay => decode_relay(&payload),
                };
                let Ok(Msg::Snap(s)) = msg else {
                    panic!("not a snap");
                };
                vv = vv.or(s.vv);
                joined.extend(s.data);
            }
            let mut copy = Board::new(4);
            let writes = decode_delta(&joined).unwrap();
            assert_eq!(copy.merge_delta(writes, &vv.unwrap()), 0);
            assert_eq!(copy.state_hash(), board.state_hash());
        }
    }

    // ---- property tests ------------------------------------------------------------------------

    fn arb_str(max: usize) -> impl Strategy<Value = String> {
        prop::collection::vec(any::<char>(), 0..=max.min(40)).prop_map(|v| v.into_iter().collect())
    }

    fn arb_coord() -> impl Strategy<Value = i32> {
        prop_oneof![-1000i32..1000, -MAX_COORD..=MAX_COORD]
    }

    fn arb_fields() -> impl Strategy<Value = Fields> {
        (
            prop::option::of((arb_coord(), arb_coord(), arb_coord(), arb_coord())),
            prop::option::of(any::<u8>()),
            prop::option::of(any::<u8>()),
            prop::option::of(any::<u8>()),
            prop::option::of(arb_str(MAX_TEXT_CHARS)),
            prop::option::of("[a-z]{1,32}"),
            prop::option::of(arb_str(MAX_ALT_CHARS)),
            prop::option::of(any::<bool>()),
        )
            .prop_map(|(g, ink, width, fill, text, z, alt, deleted)| Fields {
                geom: g.map(|(x, y, w, h)| Geom { x, y, w, h }),
                ink,
                width,
                fill,
                text,
                z,
                alt,
                deleted,
            })
    }

    fn arb_vv() -> impl Strategy<Value = Vv> {
        prop::collection::btree_map(any::<u32>(), 1u32.., 0..6)
    }

    fn arb_info() -> impl Strategy<Value = AuthorInfo> {
        (
            arb_str(MAX_NAME_CHARS),
            prop::option::of(arb_str(MAX_KEY_CHARS)),
            any::<u8>(),
            any::<u8>(),
        )
            .prop_map(|(name, cert_key, shape, color)| AuthorInfo {
                name,
                cert_key,
                shape,
                color,
            })
    }

    fn arb_board_field() -> impl Strategy<Value = BoardField> {
        prop_oneof![
            arb_str(MAX_TITLE_CHARS).prop_map(BoardField::Title),
            (any::<u32>(), prop::option::of(arb_str(MAX_KEY_CHARS)))
                .prop_map(|(author, cert_key)| BoardField::Host(Host { author, cert_key })),
            (0u8..3).prop_map(|p| BoardField::Policy(Policy::from_u8(p).unwrap())),
            (arb_str(MAX_KEY_CHARS), any::<bool>()).prop_map(|(k, v)| BoardField::Allowed(k, v)),
            any::<bool>().prop_map(BoardField::LaserForViewers),
            prop::option::of(any::<u32>()).prop_map(BoardField::Presenter),
        ]
    }

    fn arb_points(max: usize) -> impl Strategy<Value = Vec<(i32, i32)>> {
        prop::collection::vec((arb_coord(), arb_coord()), 0..max)
    }

    /// A body for op (author, seq).
    fn arb_body(author: AuthorKey, seq: u32) -> impl Strategy<Value = Body> {
        let own = ObjId {
            author,
            counter: seq,
        };
        prop_oneof![
            (0u8..10, arb_points(40), arb_fields()).prop_map(move |(k, base, fields)| {
                Body::Create {
                    id: own,
                    kind: Kind::from_u8(k).unwrap(),
                    base,
                    fields,
                }
            }),
            (any::<u32>(), any::<u32>(), arb_fields()).prop_map(|(a, c, fields)| Body::Set {
                id: ObjId {
                    author: a,
                    counter: c
                },
                fields
            }),
            arb_vv().prop_map(move |vv| Body::Clear { clear_id: own, vv }),
            (any::<u32>(), any::<u32>()).prop_map(|(a, c)| Body::ClearRevoke {
                clear_id: ObjId {
                    author: a,
                    counter: c
                }
            }),
            prop::collection::vec(arb_board_field(), 0..5)
                .prop_map(|fields| Body::Board { fields }),
            arb_info().prop_map(Body::Author),
        ]
    }

    fn arb_ops() -> impl Strategy<Value = (AuthorKey, Vec<Op>)> {
        (
            any::<u32>(),
            prop::collection::vec((1u32.., 1u64..(1 << 60)), 0..8),
        )
            .prop_flat_map(|(author, clocks)| {
                let bodies: Vec<_> = clocks
                    .iter()
                    .map(|&(seq, _)| arb_body(author, seq))
                    .collect();
                (Just(author), Just(clocks), bodies)
            })
            .prop_map(|(author, clocks, bodies)| {
                let ops = clocks
                    .into_iter()
                    .zip(bodies)
                    .map(|((seq, lamport), body)| Op {
                        author,
                        seq,
                        lamport,
                        body,
                    })
                    .collect();
                (author, ops)
            })
    }

    fn arb_stamp() -> impl Strategy<Value = Stamp> {
        (any::<u64>(), any::<u32>(), any::<u32>()).prop_map(|(lamport, author, seq)| Stamp {
            lamport,
            author,
            seq,
        })
    }

    fn arb_write() -> impl Strategy<Value = Write> {
        let field = arb_fields().prop_filter_map("empty", |f| f.to_list().into_iter().next());
        prop_oneof![
            (arb_stamp(), 0u8..10, arb_points(30)).prop_map(|(at, k, base)| Write::Object {
                at,
                kind: Kind::from_u8(k).unwrap(),
                base
            }),
            (any::<u32>(), any::<u32>(), arb_stamp(), field).prop_map(|(a, c, at, field)| {
                Write::Field {
                    id: ObjId {
                        author: a,
                        counter: c,
                    },
                    at,
                    field,
                }
            }),
            (arb_stamp(), arb_vv()).prop_map(|(at, vv)| Write::Clear { at, vv }),
            (any::<u32>(), any::<u32>(), arb_stamp()).prop_map(|(a, c, at)| Write::Revoke {
                clear_id: ObjId {
                    author: a,
                    counter: c
                },
                at
            }),
            (arb_stamp(), arb_board_field()).prop_map(|(at, field)| Write::Board { at, field }),
            (arb_stamp(), arb_info()).prop_map(|(at, info)| Write::Author { at, info }),
        ]
    }

    fn arb_msg() -> impl Strategy<Value = Msg> {
        let board = any::<[u8; 16]>();
        prop_oneof![
            (
                board,
                any::<u32>(),
                arb_info(),
                arb_vv(),
                any::<bool>(),
                any::<u64>()
            )
                .prop_map(|(board, author, info, vv, host, joined_at_ms)| Msg::Hello(
                    Hello {
                        board,
                        author,
                        info,
                        vv,
                        host,
                        joined_at_ms
                    }
                )),
            (
                board,
                arb_ops(),
                prop::option::of((arb_coord(), arb_coord()))
            )
                .prop_map(|(board, (author, ops), cursor)| Msg::Ops(OpsMsg {
                    board,
                    author,
                    ops,
                    cursor
                })),
            (board, any::<u32>(), 1u32.., any::<u32>()).prop_map(|(board, author, a, b)| {
                Msg::Need(Need {
                    board,
                    author,
                    from_seq: a.min(b.max(1)),
                    to_seq: a.max(b),
                })
            }),
            (board, any::<u64>(), any::<u32>()).prop_map(|(board, vv_hash, total_ops)| {
                Msg::Digest(Digest {
                    board,
                    vv_hash,
                    total_ops,
                })
            }),
            (
                board,
                1u16..,
                arb_vv(),
                prop::collection::vec(any::<u8>(), 0..100)
            )
                .prop_flat_map(|(board, of, vv, data)| (0..of).prop_map(move |part| {
                    Msg::Snap(Snap {
                        board,
                        part,
                        of,
                        vv: (part == 0).then(|| vv.clone()),
                        data: data.clone(),
                    })
                })),
            (board, arb_str(MAX_TITLE_CHARS), any::<u32>(), any::<u64>()).prop_map(
                |(board, title, host, started_ms)| Msg::Board(BoardMsg {
                    board,
                    title,
                    host,
                    started_ms
                })
            ),
            (any::<u32>(), arb_coord(), arb_coord(), 0u8..4, any::<u8>()).prop_map(
                |(author, x, y, status, tool)| Msg::Cursor(Cursor {
                    author,
                    x,
                    y,
                    status,
                    tool
                })
            ),
            (any::<u32>(), any::<u16>(), any::<u16>(), arb_points(60)).prop_map(
                |(author, sid, seq, points)| Msg::Live(Live {
                    author,
                    sid,
                    seq,
                    points
                })
            ),
            (any::<u32>(), arb_points(60))
                .prop_map(|(author, points)| Msg::Laser(Laser { author, points })),
            (any::<u32>(), arb_coord(), arb_coord(), any::<u32>()).prop_map(
                |(author, x, y, zoom_permille)| Msg::View(View {
                    author,
                    x,
                    y,
                    zoom_permille
                })
            ),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn encode_decode_round_trip(msg in arb_msg()) {
            let relay = encode_relay(&msg).unwrap();
            prop_assert_eq!(decode_relay(&relay).unwrap(), msg.clone());
            if !relay_only(msg.kind()) {
                match encode_peers(&msg) {
                    Ok((kind, payload)) => {
                        prop_assert_eq!(decode_peers(kind, &payload).unwrap(), msg);
                    }
                    Err(e) => prop_assert_eq!(e, EncodeError::TooLarge),
                }
            }
        }

        #[test]
        fn delta_codec_round_trip(writes in prop::collection::vec(arb_write(), 0..40)) {
            let data = encode_delta(&writes).unwrap();
            prop_assert_eq!(decode_delta(&data).unwrap(), writes);
        }

        #[test]
        fn decode_never_panics_on_random_bytes(
            kind in any::<u8>(),
            bytes in prop::collection::vec(any::<u8>(), 0..1200),
        ) {
            let _ = decode_peers(kind, &bytes);
            let _ = decode_relay(&bytes);
            let _ = decode_delta(&bytes);
            let mut framed = vec![FORMAT, kind];
            framed.extend_from_slice(&bytes);
            let _ = decode_relay(&framed);
            let _ = decode_peers(kind, &framed[1..]);
        }

        #[test]
        fn decode_never_panics_on_mutated_messages(
            msg in arb_msg(),
            flips in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..6),
            cut in any::<prop::sample::Index>(),
        ) {
            let mut payload = encode_relay(&msg).unwrap();
            for (at, v) in &flips {
                let i = at.index(payload.len());
                payload[i] ^= v;
            }
            let _ = decode_relay(&payload);
            let _ = decode_relay(&payload[..cut.index(payload.len() + 1)]);
            if payload.len() > 2 {
                let _ = decode_peers(payload[1], &[&payload[..1], &payload[2..]].concat());
            }
        }

        #[test]
        fn decode_never_panics_on_mutated_deltas(
            writes in prop::collection::vec(arb_write(), 1..10),
            flips in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..6),
        ) {
            let mut raw = decompress_to_vec_with_limit(&encode_delta(&writes).unwrap(), 1 << 20)
                .unwrap();
            for (at, v) in &flips {
                let i = at.index(raw.len());
                raw[i] ^= v;
            }
            let _ = decode_delta(&compress_to_vec(&raw, 1));
        }
    }
}
