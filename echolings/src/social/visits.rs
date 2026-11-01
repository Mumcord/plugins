//! Visits: one pat per pet per day, enforced on both sides because the sender is untrusted.
//!
//! Visitors are identified by their peer key, which is stable across sessions. People without a
//! certificate have no key; they are tracked by session for the rest of the day.

use std::collections::BTreeSet;

use mumcord_plugin_sdk::{
    log,
    storage::{self, Scope},
};
use serde::{Deserialize, Serialize};

/// Bounds the stored ledger; nobody gets hundreds of honest pats a day.
const MAX_ENTRIES: usize = 200;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    pub day: i64,
    pub given: BTreeSet<String>,
    pub received: BTreeSet<String>,
}

/// The ledger id of a peer.
pub fn visitor_id(key: Option<&str>, session: u32) -> String {
    match key {
        Some(key) => key.to_owned(),
        None => format!("session:{session}"),
    }
}

impl Ledger {
    fn roll(&mut self, day: i64) {
        if day != self.day {
            *self = Ledger {
                day,
                ..Ledger::default()
            };
        }
    }

    pub fn has_given(&self, id: &str, day: i64) -> bool {
        self.day == day && self.given.contains(id)
    }

    /// Records a pat the user gives; false if they already patted this pet today.
    pub fn give(&mut self, id: &str, day: i64) -> bool {
        self.roll(day);
        record(&mut self.given, id)
    }

    /// Records a pat the user's pet gets; false if this visitor already patted today.
    pub fn receive(&mut self, id: &str, day: i64) -> bool {
        self.roll(day);
        record(&mut self.received, id)
    }
}

fn record(set: &mut BTreeSet<String>, id: &str) -> bool {
    if set.len() >= MAX_ENTRIES || set.contains(id) {
        return false;
    }
    set.insert(id.to_owned());
    true
}

const STORAGE_KEY: &str = "visits";
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Stored {
    v: u32,
    ledger: Ledger,
}

/// The ledger from plugin storage (scope identity); empty if missing or unreadable, which at
/// worst allows one extra pat today.
pub fn load() -> Ledger {
    match storage::get_json::<Stored>(Scope::Identity, STORAGE_KEY) {
        Ok(Some(stored)) if stored.v == VERSION => stored.ledger,
        Ok(_) => Ledger::default(),
        Err(e) => {
            log::warn(&format!("could not read the visits: {e:?}"));
            Ledger::default()
        }
    }
}

pub fn save(ledger: &Ledger) {
    let stored = Stored {
        v: VERSION,
        ledger: ledger.clone(),
    };
    if let Err(e) = storage::set_json(Scope::Identity, STORAGE_KEY, &stored) {
        log::warn(&format!("could not save the visits: {e:?}"));
    }
}
