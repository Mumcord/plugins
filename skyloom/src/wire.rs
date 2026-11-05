//! The messages between Skyloom clients. Each one is a Bovnar document (sent through the SDK's
//! `bovnar` module) with a `type` field naming the message and the fields of that message. They
//! stay small, so they fit Mumble plugin data (997 bytes) and the relay alike. A later layout
//! raises [`VERSION`]; clients ignore documents of another version, and fields they do not know.
//!
//! The sender is the Mumble session the server attributes the message to; no payload ever names
//! its own sender.

use mumcord_plugin_sdk::bovnar;
use serde::{Deserialize, Serialize};

/// Layout version of every message this version sends and accepts.
pub const VERSION: u32 = 1;
/// The `kind` byte for Mumble plugin data.
pub const KIND: u8 = 1;
/// Loom size in loom units (16:9). Star positions are clamped to it.
pub const LOOM_W: u16 = 1600;
pub const LOOM_H: u16 = 900;
/// Most players in a night, so slot masks fit a u16.
pub const MAX_PLAYERS: usize = 12;
/// Most stars one player holds (a lone Threader with two players).
pub const MAX_STARS: usize = 4;

pub type Secret = [u8; 32];
pub type Nonce = [u8; 16];
pub type Pos = (u16, u16);

/// Night settings, chosen by the Lamplighter in the lobby.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Weave time: 60, 90 or 120 seconds.
    pub weave_secs: u8,
    /// "Mixed" skies: twists from round 3. False is "Calm".
    pub mixed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            weave_secs: 90,
            mixed: true,
        }
    }
}

impl Settings {
    fn valid(self) -> bool {
        matches!(self.weave_secs, 60 | 90 | 120)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    /// The night's players in join order (the first is the Lamplighter) and its settings. Sent
    /// by the Lamplighter whenever they change; `rev` counts the changes.
    Roster {
        night: u32,
        rev: u8,
        round: u8,
        settings: Settings,
        /// Roster slots of players who have already woven this night.
        weaved: u16,
        players: Vec<u32>,
    },
    /// Asks the Lamplighter to be seated (`play`) or announces a spectator.
    Join { play: bool },
    /// Leaves the night.
    Leave,
    /// The next Weaver starts round `round` with roster revision `rev`.
    Ready { round: u8, rev: u8 },
    /// The Weaver's commitment to its secret, and the figures used so far this night.
    Seal {
        round: u8,
        #[serde(with = "hex")]
        commit: [u8; 32],
        used: u64,
    },
    /// A Threader's randomness for the figure draw.
    Witness {
        round: u8,
        #[serde(with = "hex")]
        nonce: Nonce,
    },
    /// Weaving starts; `witnesses` is the roster slots whose witness counts.
    Weave { round: u8, witnesses: u16 },
    /// The sender's own stars, in star order.
    Stars { round: u8, stars: Vec<Pos> },
    /// The sender's final stars at the deadline.
    Lock { round: u8, stars: Vec<Pos> },
    /// The Weaver plucks a star so everyone sees which one is meant.
    Pluck { round: u8, target: u32, star: u8 },
    /// The Weaver ties off early with `spare` seconds left.
    TieOff { round: u8, spare: u8 },
    /// The Weaver's secret, and the roster slots whose locks are scored.
    Reveal {
        round: u8,
        #[serde(with = "hex")]
        secret: Secret,
        locks: u16,
    },
    /// A digest of the sender's computed round result, to compare.
    Result {
        round: u8,
        #[serde(with = "hex")]
        hash: [u8; 8],
    },
    /// Sent directly to the second Weaver in a Twin Weavers round.
    Share {
        round: u8,
        #[serde(with = "hex")]
        secret: Secret,
    },
    /// The Lamplighter ends the night: everyone sees the tapestry.
    End { night: u32 },
}

impl bovnar::Message for Message {
    fn version() -> u32 {
        VERSION
    }
}

impl Message {
    /// The Mumble plugin data destination of a message for the whole channel or one player.
    pub fn peers(to: mumcord_plugin_sdk::peers::Target) -> bovnar::Destination {
        bovnar::Destination::Peers { to, kind: KIND }
    }

    /// Parses an untrusted document. Errs for bytes that are no Bovnar document, another
    /// layout version, an unknown type, or values out of range.
    pub fn decode(bytes: &[u8]) -> Result<Message, bovnar::Error> {
        let received = bovnar::decode::<Message>(bytes)?;
        if received.version != VERSION || !received.message.valid() {
            return Err(bovnar::Error::WrongType);
        }
        Ok(received.message)
    }

    fn valid(&self) -> bool {
        let on_loom = |stars: &[Pos]| {
            !stars.is_empty()
                && stars.len() <= MAX_STARS
                && stars.iter().all(|&(x, y)| x <= LOOM_W && y <= LOOM_H)
        };
        match self {
            Message::Roster {
                settings, players, ..
            } => settings.valid() && !players.is_empty() && players.len() <= MAX_PLAYERS,
            Message::Stars { stars, .. } | Message::Lock { stars, .. } => on_loom(stars),
            _ => true,
        }
    }
}

/// Fixed-size byte arrays travel as hex text, which stays compact in a Bovnar document.
mod hex {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer, const N: usize>(
        bytes: &[u8; N],
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let mut text = String::with_capacity(N * 2);
        for b in bytes {
            text.push_str(&format!("{b:02x}"));
        }
        s.serialize_str(&text)
    }

    pub fn deserialize<'de, D: Deserializer<'de>, const N: usize>(
        d: D,
    ) -> Result<[u8; N], D::Error> {
        let text = String::deserialize(d)?;
        if text.len() != N * 2 || !text.is_ascii() {
            return Err(D::Error::custom("wrong hex length"));
        }
        let mut out = [0u8; N];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).map_err(D::Error::custom)?;
        }
        Ok(out)
    }
}
