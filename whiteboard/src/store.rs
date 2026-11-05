//! WB6: saving boards on this device.
//!
//! A board is stored as one board file (the same bytes as the `.mcwb` export) split into chunks
//! under `board/<hex id>/<n>`, plus a small index value `boards` that lists every saved board.
//! The host store allows 64 KiB per value, so chunks are at most [`CHUNK_SIZE`] bytes.
//!
//! Board file layout (integers are LEB128 varints unless noted):
//! `"MCWB"`, version byte, board id (16 bytes), saved-at ms, title (length + UTF-8), version
//! vector (count, then author key as 4 little-endian bytes + seq, keys ascending), state delta
//! (length + the codec's deflated `delta_since(empty)`), then an FNV-1a 64 checksum (8 bytes,
//! little-endian) over everything before it. Decoding is strict and never panics.
//!
//! Crash safety: chunks are written first and the index last. A save that stops half-way leaves
//! the index pointing at a mix of old and new chunks, which the size and checksum checks reject,
//! so a half-written board is never read as valid.
//!
//! Everything here is pure except for the [`Storage`] trait, which the plugin glue implements
//! with the SDK storage calls (tests use a `HashMap`).

use crate::codec::{self, BoardId};
use crate::model::{AuthorKey, Board, MAX_TITLE_CHARS, MAX_VV_ENTRIES, Vv, Write};

pub const MAGIC: &[u8; 4] = b"MCWB";
pub const VERSION: u8 = 1;
/// Host values are capped at 64 KiB; the rest is headroom.
pub const CHUNK_SIZE: usize = 60 * 1024;
/// Storage quota from the manifest (1024 KiB).
pub const QUOTA_BYTES: usize = 1024 * 1024;
pub const INDEX_KEY: &str = "boards";
/// Larger than any board that fits the quota, so imported board files from bigger stores still
/// load, but bounded so a hostile file cannot exhaust memory.
pub const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_BOARDS: usize = 256;
/// Wait this long after the last change before saving.
pub const SAVE_DEBOUNCE_MS: u64 = 2_000;
/// Save at the latest this long after the first unsaved change, even while people keep drawing.
pub const SAVE_MAX_DELAY_MS: u64 = 30_000;

const INDEX_VERSION: u8 = 1;
const CHECKSUM_LEN: usize = 8;
/// Title bytes allowed in files: 64 characters of up to 4 UTF-8 bytes each.
const MAX_TITLE_BYTES: usize = MAX_TITLE_CHARS * 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// The host store failed; the text comes from the host.
    Storage(String),
    /// Saving would exceed the storage quota.
    Quota { needed: usize, quota: usize },
    /// The board file is larger than [`MAX_FILE_BYTES`] or the index is full.
    TooLarge,
    /// The board state could not be encoded.
    Encode,
    /// Stored or imported bytes are not a valid board file or index.
    Corrupt(&'static str),
    /// No saved board with this id.
    NotFound,
}

/// Key-value storage as the host offers it.
pub trait Storage {
    fn get(&mut self, key: &str) -> Result<Option<Vec<u8>>, String>;
    fn set(&mut self, key: &str, value: &[u8]) -> Result<(), String>;
    fn delete(&mut self, key: &str) -> Result<(), String>;
    fn list_keys(&mut self, prefix: &str) -> Result<Vec<String>, String>;
}

/// A board as saved on this device or exported as a board file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardFile {
    pub id: BoardId,
    pub title: String,
    pub saved_at_ms: u64,
    pub vv: Vv,
    pub writes: Vec<Write>,
}

impl BoardFile {
    /// Captures the full state of `board`.
    pub fn from_board(id: BoardId, board: &Board, saved_at_ms: u64) -> BoardFile {
        let title = board
            .settings()
            .title
            .as_ref()
            .map_or_else(String::new, |r| r.val.clone());
        BoardFile {
            id,
            title,
            saved_at_ms,
            vv: board.vv().clone(),
            writes: board.delta_since(&Vv::new()),
        }
    }

    /// A replica for a new session with author key `me`, plus how many writes the model rejected.
    pub fn into_board(self, me: AuthorKey) -> (Board, usize) {
        let mut board = Board::new(me);
        let rejected = board.merge_delta(self.writes, &self.vv);
        (board, rejected)
    }
}

/// One saved board in the index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    pub id: BoardId,
    pub title: String,
    pub saved_at_ms: u64,
    /// Size of the board file in bytes.
    pub size: u32,
    pub chunks: u32,
}

// ---- byte helpers ------------------------------------------------------------------------------

fn put_uv(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    put_uv(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

fn fnv64(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

type RResult<T> = Result<T, StoreError>;

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Reader<'a> {
        Reader { buf, pos: 0 }
    }

    fn bytes(&mut self, n: usize) -> RResult<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.buf.len())
            .ok_or(StoreError::Corrupt("truncated"))?;
        let out = &self.buf[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn u8(&mut self) -> RResult<u8> {
        Ok(self.bytes(1)?[0])
    }

    /// Canonical varints only: no overlong encodings, no overflow.
    fn uv(&mut self) -> RResult<u64> {
        let mut v: u64 = 0;
        for i in 0..10 {
            let b = self.u8()?;
            let bits = u64::from(b & 0x7f);
            if i == 9 && bits > 1 {
                return Err(StoreError::Corrupt("varint overflow"));
            }
            v |= bits << (7 * i);
            if b & 0x80 == 0 {
                if i > 0 && b == 0 {
                    return Err(StoreError::Corrupt("overlong varint"));
                }
                return Ok(v);
            }
        }
        Err(StoreError::Corrupt("varint overflow"))
    }

    fn u32v(&mut self) -> RResult<u32> {
        u32::try_from(self.uv()?).map_err(|_| StoreError::Corrupt("value out of range"))
    }

    fn len(&mut self, max: usize) -> RResult<usize> {
        usize::try_from(self.uv()?)
            .ok()
            .filter(|&n| n <= max)
            .ok_or(StoreError::Corrupt("length out of range"))
    }

    fn title(&mut self) -> RResult<String> {
        let n = self.len(MAX_TITLE_BYTES)?;
        let s =
            std::str::from_utf8(self.bytes(n)?).map_err(|_| StoreError::Corrupt("bad UTF-8"))?;
        if s.chars().count() > MAX_TITLE_CHARS {
            return Err(StoreError::Corrupt("title too long"));
        }
        Ok(s.to_string())
    }

    fn id(&mut self) -> RResult<BoardId> {
        let mut id = [0u8; 16];
        id.copy_from_slice(self.bytes(16)?);
        Ok(id)
    }

    fn end(&self) -> RResult<()> {
        if self.pos == self.buf.len() {
            Ok(())
        } else {
            Err(StoreError::Corrupt("trailing bytes"))
        }
    }
}

// ---- board file --------------------------------------------------------------------------------

fn title_ok(title: &str) -> bool {
    title.len() <= MAX_TITLE_BYTES && title.chars().count() <= MAX_TITLE_CHARS
}

/// The board file bytes (also the `.mcwb` export).
pub fn encode_board(file: &BoardFile) -> Result<Vec<u8>, StoreError> {
    if !title_ok(&file.title) || file.vv.len() > MAX_VV_ENTRIES {
        return Err(StoreError::Encode);
    }
    let delta = codec::encode_delta(&file.writes).map_err(|_| StoreError::Encode)?;
    let mut out = Vec::with_capacity(delta.len() + 64);
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.extend_from_slice(&file.id);
    put_uv(&mut out, file.saved_at_ms);
    put_str(&mut out, &file.title);
    put_uv(&mut out, file.vv.len() as u64);
    for (&author, &seq) in &file.vv {
        out.extend_from_slice(&author.to_le_bytes());
        put_uv(&mut out, u64::from(seq));
    }
    put_uv(&mut out, delta.len() as u64);
    out.extend_from_slice(&delta);
    let sum = fnv64(&out);
    out.extend_from_slice(&sum.to_le_bytes());
    if out.len() > MAX_FILE_BYTES {
        return Err(StoreError::TooLarge);
    }
    Ok(out)
}

/// Parses a board file. Rejects anything malformed, truncated, extended or with a bad checksum.
pub fn decode_board(bytes: &[u8]) -> Result<BoardFile, StoreError> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(StoreError::TooLarge);
    }
    if bytes.len() < MAGIC.len() + 1 + CHECKSUM_LEN || &bytes[..4] != MAGIC {
        return Err(StoreError::Corrupt("not a board file"));
    }
    if bytes[4] != VERSION {
        return Err(StoreError::Corrupt("unknown version"));
    }
    let (body, sum) = bytes.split_at(bytes.len() - CHECKSUM_LEN);
    let mut sum_bytes = [0u8; CHECKSUM_LEN];
    sum_bytes.copy_from_slice(sum);
    if fnv64(body) != u64::from_le_bytes(sum_bytes) {
        return Err(StoreError::Corrupt("checksum mismatch"));
    }
    let mut r = Reader::new(&body[5..]);
    let id = r.id()?;
    let saved_at_ms = r.uv()?;
    let title = r.title()?;
    let n = r.len(MAX_VV_ENTRIES)?;
    let mut vv = Vv::new();
    let mut last: Option<AuthorKey> = None;
    for _ in 0..n {
        let mut key = [0u8; 4];
        key.copy_from_slice(r.bytes(4)?);
        let author = AuthorKey::from_le_bytes(key);
        if last.is_some_and(|l| author <= l) {
            return Err(StoreError::Corrupt("version vector not sorted"));
        }
        last = Some(author);
        vv.insert(author, r.u32v()?);
    }
    let len = r.len(MAX_FILE_BYTES)?;
    let delta = r.bytes(len)?;
    r.end()?;
    let writes = codec::decode_delta(delta).map_err(|_| StoreError::Corrupt("bad board state"))?;
    Ok(BoardFile {
        id,
        title,
        saved_at_ms,
        vv,
        writes,
    })
}

/// Splits a board file into values that fit the host store.
pub fn chunks(bytes: &[u8]) -> Vec<&[u8]> {
    bytes.chunks(CHUNK_SIZE).collect()
}

fn chunk_count(size: usize) -> usize {
    size.div_ceil(CHUNK_SIZE)
}

pub fn hex_id(id: &BoardId) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn parse_hex_id(s: &str) -> Option<BoardId> {
    if s.len() != 32 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut id = [0u8; 16];
    for (i, b) in id.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(id)
}

fn chunk_prefix(id: &BoardId) -> String {
    format!("board/{}/", hex_id(id))
}

pub fn chunk_key(id: &BoardId, n: usize) -> String {
    format!("{}{n}", chunk_prefix(id))
}

// ---- index -------------------------------------------------------------------------------------

pub fn encode_index(entries: &[IndexEntry]) -> Result<Vec<u8>, StoreError> {
    if entries.len() > MAX_BOARDS {
        return Err(StoreError::TooLarge);
    }
    let mut out = vec![INDEX_VERSION];
    put_uv(&mut out, entries.len() as u64);
    for e in entries {
        if !title_ok(&e.title) {
            return Err(StoreError::Encode);
        }
        out.extend_from_slice(&e.id);
        put_str(&mut out, &e.title);
        put_uv(&mut out, e.saved_at_ms);
        put_uv(&mut out, u64::from(e.size));
        put_uv(&mut out, u64::from(e.chunks));
    }
    Ok(out)
}

pub fn decode_index(bytes: &[u8]) -> Result<Vec<IndexEntry>, StoreError> {
    let mut r = Reader::new(bytes);
    if r.u8()? != INDEX_VERSION {
        return Err(StoreError::Corrupt("unknown index version"));
    }
    let n = r.len(MAX_BOARDS)?;
    let mut out: Vec<IndexEntry> = Vec::with_capacity(n);
    for _ in 0..n {
        let e = IndexEntry {
            id: r.id()?,
            title: r.title()?,
            saved_at_ms: r.uv()?,
            size: r.u32v()?,
            chunks: r.u32v()?,
        };
        let size = e.size as usize;
        if size > MAX_FILE_BYTES || e.chunks as usize != chunk_count(size) {
            return Err(StoreError::Corrupt("bad index entry"));
        }
        if out.iter().any(|o| o.id == e.id) {
            return Err(StoreError::Corrupt("duplicate index entry"));
        }
        out.push(e);
    }
    r.end()?;
    Ok(out)
}

/// Bytes the saved boards and the index take in the store.
pub fn used_bytes(entries: &[IndexEntry]) -> usize {
    let index = encode_index(entries).map_or(0, |b| b.len());
    index + entries.iter().map(|e| e.size as usize).sum::<usize>()
}

fn storage<T>(r: Result<T, String>) -> Result<T, StoreError> {
    r.map_err(StoreError::Storage)
}

/// The saved boards, newest first. A missing index means no boards.
pub fn read_index(st: &mut impl Storage) -> Result<Vec<IndexEntry>, StoreError> {
    let mut entries = match storage(st.get(INDEX_KEY))? {
        Some(bytes) => decode_index(&bytes)?,
        None => Vec::new(),
    };
    entries.sort_by_key(|e| std::cmp::Reverse(e.saved_at_ms));
    Ok(entries)
}

/// Deletes chunk keys of `id` from `keep` on (or all of them for `keep == 0`).
fn delete_chunks_from(st: &mut impl Storage, id: &BoardId, keep: usize) -> Result<(), StoreError> {
    let prefix = chunk_prefix(id);
    for key in storage(st.list_keys(&prefix))? {
        let stale = key
            .strip_prefix(&prefix)
            .and_then(|n| n.parse::<usize>().ok())
            .is_none_or(|n| n >= keep);
        if stale {
            storage(st.delete(&key))?;
        }
    }
    Ok(())
}

/// Saves a board: chunks first, then the index, then removes chunks the new save no longer uses.
pub fn save_board(st: &mut impl Storage, file: &BoardFile) -> Result<IndexEntry, StoreError> {
    let bytes = encode_board(file)?;
    let mut entries = read_index(st)?;
    entries.retain(|e| e.id != file.id);
    let entry = IndexEntry {
        id: file.id,
        title: file.title.clone(),
        saved_at_ms: file.saved_at_ms,
        size: u32::try_from(bytes.len()).map_err(|_| StoreError::TooLarge)?,
        chunks: u32::try_from(chunk_count(bytes.len())).map_err(|_| StoreError::TooLarge)?,
    };
    entries.push(entry.clone());
    let needed = used_bytes(&entries);
    if needed > QUOTA_BYTES {
        return Err(StoreError::Quota {
            needed,
            quota: QUOTA_BYTES,
        });
    }
    let index = encode_index(&entries)?;
    for (n, chunk) in chunks(&bytes).into_iter().enumerate() {
        storage(st.set(&chunk_key(&file.id, n), chunk))?;
    }
    storage(st.set(INDEX_KEY, &index))?;
    delete_chunks_from(st, &file.id, entry.chunks as usize)?;
    Ok(entry)
}

/// Loads a saved board. Missing, short or mismatching chunks are rejected.
pub fn load_board(st: &mut impl Storage, id: &BoardId) -> Result<BoardFile, StoreError> {
    let entry = read_index(st)?
        .into_iter()
        .find(|e| e.id == *id)
        .ok_or(StoreError::NotFound)?;
    let size = entry.size as usize;
    let mut bytes = Vec::with_capacity(size);
    for n in 0..entry.chunks as usize {
        let chunk =
            storage(st.get(&chunk_key(id, n)))?.ok_or(StoreError::Corrupt("missing chunk"))?;
        let expected = CHUNK_SIZE.min(size - bytes.len().min(size));
        if chunk.len() != expected {
            return Err(StoreError::Corrupt("chunk size mismatch"));
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.len() != size {
        return Err(StoreError::Corrupt("board size mismatch"));
    }
    let file = decode_board(&bytes)?;
    if file.id != *id {
        return Err(StoreError::Corrupt("board id mismatch"));
    }
    Ok(file)
}

/// Deletes a saved board on this device (only when the person asked for it).
pub fn delete_board(st: &mut impl Storage, id: &BoardId) -> Result<(), StoreError> {
    let mut entries = read_index(st)?;
    let before = entries.len();
    entries.retain(|e| e.id != *id);
    if entries.len() != before {
        storage(st.set(INDEX_KEY, &encode_index(&entries)?))?;
    }
    delete_chunks_from(st, id, 0)
}

/// Removes chunks that belong to no indexed board (left behind by a first save that stopped
/// before the index was written). Returns how many keys were deleted.
pub fn remove_orphans(st: &mut impl Storage) -> Result<usize, StoreError> {
    let entries = read_index(st)?;
    let mut removed = 0;
    for key in storage(st.list_keys("board/"))? {
        let id = key
            .strip_prefix("board/")
            .and_then(|rest| rest.split('/').next())
            .and_then(parse_hex_id);
        if id.is_none_or(|id| entries.iter().all(|e| e.id != id)) {
            storage(st.delete(&key))?;
            removed += 1;
        }
    }
    Ok(removed)
}

// ---- time helpers ------------------------------------------------------------------------------

/// How long ago a board was saved, for "saved 2 days ago". The page turns it into words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ago {
    JustNow,
    Minutes(u64),
    Hours(u64),
    Days(u64),
}

pub fn saved_ago(now_ms: u64, saved_at_ms: u64) -> Ago {
    // A clock that went backwards counts as "just now" rather than a negative age.
    let secs = now_ms.saturating_sub(saved_at_ms) / 1000;
    match secs {
        0..60 => Ago::JustNow,
        60..3_600 => Ago::Minutes(secs / 60),
        3_600..86_400 => Ago::Hours(secs / 3_600),
        _ => Ago::Days(secs / 86_400),
    }
}

/// Autosave timing: save [`SAVE_DEBOUNCE_MS`] after the last change, but no later than
/// [`SAVE_MAX_DELAY_MS`] after the first unsaved one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Autosave {
    first: Option<u64>,
    last: Option<u64>,
}

impl Autosave {
    pub fn changed(&mut self, now_ms: u64) {
        self.first.get_or_insert(now_ms);
        self.last = Some(now_ms);
    }

    pub fn is_dirty(&self) -> bool {
        self.first.is_some()
    }

    pub fn should_save(&self, now_ms: u64) -> bool {
        match (self.first, self.last) {
            (Some(first), Some(last)) => {
                now_ms.saturating_sub(last) >= SAVE_DEBOUNCE_MS
                    || now_ms.saturating_sub(first) >= SAVE_MAX_DELAY_MS
            }
            _ => false,
        }
    }

    pub fn saved(&mut self) {
        *self = Autosave::default();
    }

    /// A failed save stays dirty and is retried one debounce later instead of on every tick.
    pub fn failed(&mut self, now_ms: u64) {
        self.first = Some(now_ms);
        self.last = Some(now_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Edit, Fields, Geom, Kind};
    use std::collections::HashMap;

    #[derive(Default)]
    struct Mem(HashMap<String, Vec<u8>>);

    impl Storage for Mem {
        fn get(&mut self, key: &str) -> Result<Option<Vec<u8>>, String> {
            Ok(self.0.get(key).cloned())
        }
        fn set(&mut self, key: &str, value: &[u8]) -> Result<(), String> {
            assert!(value.len() <= 64 * 1024, "value over the host limit");
            self.0.insert(key.to_string(), value.to_vec());
            Ok(())
        }
        fn delete(&mut self, key: &str) -> Result<(), String> {
            self.0.remove(key);
            Ok(())
        }
        fn list_keys(&mut self, prefix: &str) -> Result<Vec<String>, String> {
            Ok(self
                .0
                .keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect())
        }
    }

    const ID: BoardId = [7; 16];

    fn note(text: &str, x: i32) -> Edit {
        Edit::Create {
            kind: Kind::Note,
            base: vec![],
            fields: Fields {
                geom: Some(Geom {
                    x,
                    y: 0,
                    w: 600,
                    h: 400,
                }),
                text: Some(text.into()),
                fill: Some(1),
                z: Some("n".into()),
                ..Fields::default()
            },
        }
    }

    /// A board with `notes` notes and one stroke; random-ish text keeps deflate honest.
    fn board(notes: usize) -> Board {
        let mut b = Board::new(1);
        b.local_board(vec![crate::model::BoardField::Title("Launch plan".into())])
            .unwrap();
        let mut seed: u64 = 42;
        for i in 0..notes {
            let text: String = (0..500)
                .map(|_| {
                    seed = seed
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1_442_695_040_888_963_407);
                    char::from(b'a' + ((seed >> 33) % 26) as u8)
                })
                .collect();
            b.local(vec![note(&text, i as i32 * 700)]).unwrap();
        }
        b.local(vec![Edit::Create {
            kind: Kind::Stroke,
            base: vec![(0, 0), (40, 80), (90, 10)],
            fields: Fields {
                geom: Some(Geom {
                    x: 0,
                    y: 0,
                    w: 90,
                    h: 80,
                }),
                z: Some("s".into()),
                ..Fields::default()
            },
        }])
        .unwrap();
        b
    }

    #[test]
    fn board_file_round_trips() {
        let b = board(3);
        let file = BoardFile::from_board(ID, &b, 1_700_000_000_000);
        assert_eq!(file.title, "Launch plan");
        let bytes = encode_board(&file).unwrap();
        assert_eq!(&bytes[..4], MAGIC);
        let back = decode_board(&bytes).unwrap();
        assert_eq!(back, file);
        let (restored, rejected) = back.into_board(99);
        assert_eq!(rejected, 0);
        assert_eq!(restored.state_hash(), b.state_hash());
        assert_eq!(restored.vv(), b.vv());
    }

    #[test]
    fn empty_board_round_trips() {
        let file = BoardFile::from_board(ID, &Board::new(5), 0);
        let back = decode_board(&encode_board(&file).unwrap()).unwrap();
        assert_eq!(back, file);
    }

    #[test]
    fn damaged_files_are_rejected() {
        let bytes = encode_board(&BoardFile::from_board(ID, &board(2), 5)).unwrap();
        for n in 0..bytes.len() {
            assert!(decode_board(&bytes[..n]).is_err(), "truncated to {n}");
        }
        for i in 0..bytes.len() {
            let mut bad = bytes.clone();
            bad[i] ^= 0x10;
            assert!(decode_board(&bad).is_err(), "flipped byte {i}");
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(decode_board(&longer).is_err());
        assert!(decode_board(b"").is_err());
        assert!(decode_board(&[0xff; 64]).is_err());
    }

    #[test]
    fn hex_ids_and_keys() {
        let id: BoardId = core::array::from_fn(|i| i as u8 * 17);
        let hex = hex_id(&id);
        assert_eq!(hex.len(), 32);
        assert_eq!(parse_hex_id(&hex), Some(id));
        assert_eq!(parse_hex_id("zz"), None);
        assert_eq!(parse_hex_id(&"é".repeat(16)), None);
        assert_eq!(chunk_key(&id, 3), format!("board/{hex}/3"));
    }

    #[test]
    fn index_round_trips_and_is_strict() {
        let entries = vec![IndexEntry {
            id: ID,
            title: "Plan <b>".into(),
            saved_at_ms: 9,
            size: CHUNK_SIZE as u32 + 1,
            chunks: 2,
        }];
        let bytes = encode_index(&entries).unwrap();
        assert_eq!(decode_index(&bytes).unwrap(), entries);
        for n in 0..bytes.len() {
            assert!(decode_index(&bytes[..n]).is_err());
        }
        let mut wrong = entries.clone();
        wrong[0].chunks = 5;
        assert!(decode_index(&encode_index(&wrong).unwrap()).is_err());
        let mut dup = entries.clone();
        dup.push(entries[0].clone());
        assert!(decode_index(&encode_index(&dup).unwrap()).is_err());
    }

    #[test]
    fn a_150_kib_board_saves_in_chunks_and_loads() {
        let b = board(600);
        let file = BoardFile::from_board(ID, &b, 1_000);
        let bytes = encode_board(&file).unwrap();
        assert!(bytes.len() >= 150 * 1024, "only {} bytes", bytes.len());
        let parts = chunks(&bytes);
        assert!(parts.len() >= 3);
        assert!(parts.iter().all(|c| c.len() <= CHUNK_SIZE));

        let mut st = Mem::default();
        let entry = save_board(&mut st, &file).unwrap();
        assert_eq!(entry.chunks as usize, parts.len());
        assert!(used_bytes(&read_index(&mut st).unwrap()) > bytes.len());
        let loaded = load_board(&mut st, &ID).unwrap();
        let (restored, _) = loaded.into_board(2);
        assert_eq!(restored.state_hash(), b.state_hash());
    }

    #[test]
    fn smaller_save_removes_stale_chunks() {
        let mut st = Mem::default();
        save_board(&mut st, &BoardFile::from_board(ID, &board(600), 1)).unwrap();
        let entry = save_board(&mut st, &BoardFile::from_board(ID, &board(1), 2)).unwrap();
        assert_eq!(entry.chunks, 1);
        let keys = st.list_keys("board/").unwrap();
        assert_eq!(keys, vec![chunk_key(&ID, 0)]);
        assert_eq!(read_index(&mut st).unwrap().len(), 1);
        assert_eq!(load_board(&mut st, &ID).unwrap().saved_at_ms, 2);
    }

    #[test]
    fn half_written_or_damaged_chunks_are_rejected() {
        let mut st = Mem::default();
        save_board(&mut st, &BoardFile::from_board(ID, &board(600), 1)).unwrap();
        // A newer save that stopped after its first chunk: the index still describes the old one.
        let newer = encode_board(&BoardFile::from_board(ID, &board(599), 2)).unwrap();
        st.set(&chunk_key(&ID, 0), chunks(&newer)[0]).unwrap();
        assert!(matches!(
            load_board(&mut st, &ID),
            Err(StoreError::Corrupt(_))
        ));

        let mut st = Mem::default();
        save_board(&mut st, &BoardFile::from_board(ID, &board(600), 1)).unwrap();
        let mut last = st.get(&chunk_key(&ID, 2)).unwrap().unwrap();
        last.pop();
        st.set(&chunk_key(&ID, 2), &last).unwrap();
        assert!(matches!(
            load_board(&mut st, &ID),
            Err(StoreError::Corrupt(_))
        ));
        st.delete(&chunk_key(&ID, 1)).unwrap();
        assert!(matches!(
            load_board(&mut st, &ID),
            Err(StoreError::Corrupt(_))
        ));
        assert_eq!(load_board(&mut st, &[1; 16]), Err(StoreError::NotFound));
    }

    #[test]
    fn quota_is_enforced() {
        let mut st = Mem::default();
        let mut saved = 0;
        let mut quota_hit = false;
        for i in 0..20u8 {
            let file = BoardFile::from_board([i; 16], &board(600), u64::from(i));
            match save_board(&mut st, &file) {
                Ok(_) => saved += 1,
                Err(StoreError::Quota { needed, quota }) => {
                    assert!(needed > quota);
                    quota_hit = true;
                    break;
                }
                Err(e) => panic!("{e:?}"),
            }
        }
        assert!(quota_hit && saved > 0);
        assert!(used_bytes(&read_index(&mut st).unwrap()) <= QUOTA_BYTES);
        // Re-saving an existing board does not count it twice.
        let again = BoardFile::from_board([0; 16], &board(600), 99);
        save_board(&mut st, &again).unwrap();
    }

    #[test]
    fn delete_and_orphans() {
        let mut st = Mem::default();
        save_board(&mut st, &BoardFile::from_board(ID, &board(600), 1)).unwrap();
        save_board(&mut st, &BoardFile::from_board([2; 16], &board(1), 5)).unwrap();
        let list = read_index(&mut st).unwrap();
        assert_eq!(list[0].id, [2; 16], "newest first");
        delete_board(&mut st, &ID).unwrap();
        assert_eq!(read_index(&mut st).unwrap().len(), 1);
        assert_eq!(st.list_keys(&chunk_prefix(&ID)).unwrap().len(), 0);

        st.set(&chunk_key(&[9; 16], 0), b"x").unwrap();
        st.set("board/junk", b"x").unwrap();
        assert_eq!(remove_orphans(&mut st).unwrap(), 2);
        assert!(load_board(&mut st, &[2; 16]).is_ok());
    }

    #[test]
    fn corrupt_index_is_reported() {
        let mut st = Mem::default();
        st.set(INDEX_KEY, &[9, 9, 9]).unwrap();
        assert!(matches!(read_index(&mut st), Err(StoreError::Corrupt(_))));
    }

    #[test]
    fn saved_ago_buckets() {
        assert_eq!(saved_ago(10_000, 20_000), Ago::JustNow);
        assert_eq!(saved_ago(59_999, 0), Ago::JustNow);
        assert_eq!(saved_ago(60_000, 0), Ago::Minutes(1));
        assert_eq!(saved_ago(3 * 3_600_000, 0), Ago::Hours(3));
        assert_eq!(saved_ago(2 * 86_400_000 + 5, 0), Ago::Days(2));
    }

    #[test]
    fn autosave_debounces_with_a_max_delay() {
        let mut a = Autosave::default();
        assert!(!a.should_save(100_000));
        a.changed(0);
        assert!(a.is_dirty());
        assert!(!a.should_save(1_999));
        assert!(a.should_save(2_000));
        // Constant drawing: saved after 30 s anyway.
        for t in (0..30_000).step_by(500) {
            a.changed(t);
            assert!(!a.should_save(t));
        }
        assert!(a.should_save(30_000));
        a.saved();
        assert!(!a.is_dirty() && !a.should_save(99_999));
        a.failed(50_000);
        assert!(!a.should_save(51_000));
        assert!(a.should_save(52_000));
    }
}
