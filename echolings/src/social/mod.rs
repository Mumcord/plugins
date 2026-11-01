//! Other people's pets: the pet card exchange, visits and reactions to what happens in the
//! channel. Pure state, so it runs in host tests; `lib.rs` does the host calls.
//!
//! Message kinds of this plugin (byte 2 of the wire envelope): 1 pet card, 2 pat (direct),
//! 3 and 4 Echo Echo (see `games::echo_echo`).

pub mod card;
pub mod throttle;
mod view;
pub mod visits;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use crate::pet::Reaction;
use card::Card;
use throttle::CardThrottle;
use visits::{Ledger, visitor_id};

pub use view::OtherView;

pub const CARD: u8 = 1;
pub const PAT: u8 = 2;
/// The pat payload: only a version byte, so a later version can add a message.
pub const PAT_PAYLOAD: [u8; 1] = [1];

/// After the user moves, the host replays `joined` for everyone already in the new channel;
/// those are not real arrivals and get no wave.
pub const JOIN_QUIET_SECS: u64 = 3;

/// The profile card's pat button, `visit-pat:<session>`.
pub mod action {
    const PAT_PREFIX: &str = "visit-pat:";

    pub fn pat(session: u32) -> String {
        format!("{PAT_PREFIX}{session}")
    }

    pub fn parse_pat(action: &str) -> Option<u32> {
        action.strip_prefix(PAT_PREFIX)?.parse().ok()
    }
}

/// Things in the channel the user's pet reacts to. Computed locally from events the client
/// already has; no traffic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Happening {
    /// Someone joined the user's channel.
    Joined,
    ScreenShare,
    Mentioned,
}

/// What happened when the user patted someone's pet; shown on that profile card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatNote {
    Given,
    AlreadyToday,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtherPet {
    pub card: Card,
    pub key: Option<String>,
    pub talking: bool,
}

#[derive(Debug, Default)]
pub struct Social {
    /// Pets of others in the user's channel, by session.
    pub pets: BTreeMap<u32, OtherPet>,
    pub throttle: CardThrottle,
    pub ledger: Ledger,
    /// Unix seconds until which joins are replays (see [`JOIN_QUIET_SECS`]).
    pub quiet_until: u64,
    /// The other user whose profile card is open, if any.
    pub open_card: Option<u32>,
    pub pat_note: Option<(u32, PatNote)>,
}

impl Social {
    /// The user connected, disconnected or changed channel. Returns the sessions whose badges
    /// must be cleared.
    pub fn moved(&mut self, now: u64) -> Vec<u32> {
        self.throttle.invalidate();
        self.quiet_until = now + JOIN_QUIET_SECS;
        self.open_card = None;
        self.pat_note = None;
        std::mem::take(&mut self.pets).into_keys().collect()
    }

    /// Someone joined: they don't have the user's card yet.
    pub fn joined(&mut self) {
        self.throttle.invalidate();
    }

    /// True if the session had a pet (its badge must be cleared).
    pub fn left(&mut self, session: u32) -> bool {
        if self.open_card == Some(session) {
            self.open_card = None;
        }
        self.pets.remove(&session).is_some()
    }

    /// Stores a received card; false if it was invalid or unchanged.
    pub fn card_received(&mut self, session: u32, key: Option<String>, bytes: &[u8]) -> bool {
        let Some(card) = Card::decode(bytes) else {
            return false;
        };
        let talking = self.pets.get(&session).is_some_and(|p| p.talking);
        let pet = OtherPet { card, key, talking };
        if self.pets.get(&session) == Some(&pet) {
            return false;
        }
        self.pets.insert(session, pet);
        true
    }

    /// True if the session has a pet whose talking state changed (its badge must be redrawn).
    pub fn talking(&mut self, session: u32, talking: bool) -> bool {
        match self.pets.get_mut(&session) {
            Some(pet) if pet.talking != talking => {
                pet.talking = talking;
                true
            }
            _ => false,
        }
    }

    pub fn can_pat(&self, session: u32, day: i64) -> bool {
        self.pets.get(&session).is_some_and(|pet| {
            !self
                .ledger
                .has_given(&visitor_id(pet.key.as_deref(), session), day)
        })
    }

    /// Records a pat the user is about to give; false if there is no such pet or it was
    /// already patted today.
    pub fn give_pat(&mut self, session: u32, day: i64) -> bool {
        let Some(pet) = self.pets.get(&session) else {
            return false;
        };
        let id = visitor_id(pet.key.as_deref(), session);
        self.ledger.give(&id, day)
    }

    /// Checks an incoming pat; true if it counts (a valid pat, first one from this visitor
    /// today).
    pub fn pat_received(
        &mut self,
        session: u32,
        key: Option<&str>,
        payload: &[u8],
        day: i64,
    ) -> bool {
        payload.first().is_some_and(|v| *v >= 1)
            && self.ledger.receive(&visitor_id(key, session), day)
    }
}

/// The pet's reaction to a happening, or None. Sleep and Do Not Disturb are the caller's check.
pub fn reaction(happening: Happening, now: u64, quiet_until: u64) -> Option<Reaction> {
    match happening {
        Happening::Joined if now < quiet_until => None,
        Happening::Joined => Some(Reaction::Wave),
        Happening::ScreenShare => Some(Reaction::Sparkle),
        Happening::Mentioned => Some(Reaction::Hop),
    }
}

/// A server display name made safe for a notification: no control characters, at most 32
/// characters.
pub fn display_name(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .take(32)
        .collect::<String>()
        .trim()
        .to_owned()
}
