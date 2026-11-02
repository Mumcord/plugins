//! The fit history: every fit seen in the chat or opened, newest first, kept in plugin storage
//! so it survives a restart. Pure logic; reading and writing storage goes through [`Store`].

use serde::{Deserialize, Serialize};

use crate::eft::{self, Fit};

/// Entries kept; the oldest goes when a new fit arrives. 50 fits of at most 4 KiB stay below the
/// storage quota.
pub const MAX_ENTRIES: usize = 50;
/// Storage key of the index: hashes, senders and times, newest first.
pub const INDEX_KEY: &str = "history";
/// Upper bounds of one panel page. A slot tree may be at most 16 KiB encoded and every Copy
/// fitting button carries its fit, so a page holds as many fits as fit in this many bytes.
const PAGE_TEXT_BYTES: usize = 10 * 1024;
const PAGE_ROWS: usize = 12;

/// Where the history lives; the plugin's key-value storage in Mumcord, a map in tests.
pub trait Store {
    fn load(&mut self, key: &str) -> Option<Vec<u8>>;
    fn save(&mut self, key: &str, value: &[u8]);
    fn remove(&mut self, key: &str);
}

/// One fit in the history.
#[derive(Debug, Clone)]
pub struct Entry {
    pub hash: u64,
    pub sender: String,
    /// When it was seen, in seconds since 1970 (UTC).
    pub time: u64,
    /// The fit as pasted, for Open and Copy fitting.
    pub text: String,
    pub fit: Fit,
}

/// What the index stores per entry; ship and fit name come from parsing the stored text again.
#[derive(Serialize, Deserialize)]
struct Stored {
    hash: String,
    sender: String,
    time: u64,
}

/// Storage key of one fit's text.
fn fit_key(hash: u64) -> String {
    format!("fit/{hash:016x}")
}

/// Identifies a fit regardless of trailing spaces and line endings, so the same fit pasted
/// twice is listed once. FNV-1a: stable across versions, unlike the standard library's hasher.
pub fn hash(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for line in text.trim().lines() {
        for byte in line.trim_end().bytes().chain(*b"\n") {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

#[derive(Default)]
pub struct History {
    /// Newest first.
    entries: Vec<Entry>,
}

impl History {
    /// Reads the history from storage. Anything unreadable is skipped, never fatal.
    pub fn load(store: &mut impl Store) -> Self {
        let index: Vec<Stored> = store
            .load(INDEX_KEY)
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut entries = Vec::new();
        for stored in index.into_iter().take(MAX_ENTRIES) {
            let Ok(hash) = u64::from_str_radix(&stored.hash, 16) else {
                continue;
            };
            let Some(text) = store
                .load(&fit_key(hash))
                .and_then(|bytes| String::from_utf8(bytes).ok())
            else {
                continue;
            };
            let Some(fit) = eft::parse(&text) else {
                continue;
            };
            if entries.iter().any(|e: &Entry| e.hash == hash) {
                continue;
            }
            entries.push(Entry {
                hash,
                sender: stored.sender,
                time: stored.time,
                text,
                fit,
            });
        }
        Self { entries }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn get(&self, hash: u64) -> Option<&Entry> {
        self.entries.iter().find(|e| e.hash == hash)
    }

    /// Puts a fit at the top. The same fit seen again moves up with its new sender and time.
    pub fn add(&mut self, store: &mut impl Store, sender: &str, time: u64, text: &str, fit: &Fit) {
        let hash = hash(text);
        let known = self.entries.iter().position(|e| e.hash == hash);
        if let Some(i) = known {
            self.entries.remove(i);
        } else {
            store.save(&fit_key(hash), text.as_bytes());
        }
        self.entries.insert(
            0,
            Entry {
                hash,
                sender: sender.to_owned(),
                time,
                text: text.to_owned(),
                fit: fit.clone(),
            },
        );
        while self.entries.len() > MAX_ENTRIES {
            if let Some(old) = self.entries.pop() {
                store.remove(&fit_key(old.hash));
            }
        }
        self.save_index(store);
    }

    fn save_index(&self, store: &mut impl Store) {
        let index: Vec<Stored> = self
            .entries
            .iter()
            .map(|e| Stored {
                hash: format!("{:016x}", e.hash),
                sender: e.sender.clone(),
                time: e.time,
            })
            .collect();
        // Serialising plain strings and numbers cannot fail.
        if let Ok(bytes) = serde_json::to_vec(&index) {
            store.save(INDEX_KEY, &bytes);
        }
    }

    /// The end (exclusive) of the page that starts at `start`: as many entries as fit the page
    /// budget, and always at least one.
    pub fn page_end(&self, start: usize) -> usize {
        let mut bytes = 0;
        let mut end = start;
        for entry in self.entries.iter().skip(start).take(PAGE_ROWS) {
            bytes += entry.text.len();
            if end > start && bytes > PAGE_TEXT_BYTES {
                break;
            }
            end += 1;
        }
        end
    }
}

/// "14:32" for today, "2026-10-30 14:32" for an earlier day, in the user's local time.
pub fn format_time(time: u64, now: u64, utc_offset_minutes: i32) -> String {
    let offset = i64::from(utc_offset_minutes) * 60;
    let local = |t: u64| i64::try_from(t).unwrap_or(i64::MAX).saturating_add(offset);
    let (when, today) = (local(time), local(now));
    let (day, seconds) = (when.div_euclid(86_400), when.rem_euclid(86_400));
    let clock = format!("{:02}:{:02}", seconds / 3600, seconds % 3600 / 60);
    if day == today.div_euclid(86_400) {
        return clock;
    }
    let (y, m, d) = civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02} {clock}")
}

/// Year, month and day of a day number since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}
