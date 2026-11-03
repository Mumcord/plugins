//! The messages between Skyloom clients. Binary and tiny (at most 64 bytes), so they fit Mumble
//! plugin data and the relay alike. Byte 0 is the format version, byte 1 the message type; a
//! later version can change the layout behind a new format byte, and older clients ignore it.
//!
//! The sender is the Mumble session the server attributes the message to; no payload ever names
//! its own sender.

/// Format of every message this version sends and accepts.
pub const FORMAT: u8 = 1;
/// The `kind` byte for Mumble plugin data.
pub const KIND: u8 = 1;
/// Largest encoded message.
pub const MAX_LEN: usize = 64;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
        commit: [u8; 32],
        used: u64,
    },
    /// A Threader's randomness for the figure draw.
    Witness { round: u8, nonce: Nonce },
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
        secret: Secret,
        locks: u16,
    },
    /// A digest of the sender's computed round result, to compare.
    Result { round: u8, hash: [u8; 8] },
    /// Sent directly to the second Weaver in a Twin Weavers round.
    Share { round: u8, secret: Secret },
    /// The Lamplighter ends the night: everyone sees the tapestry.
    End { night: u32 },
}

fn put_stars(out: &mut Vec<u8>, stars: &[Pos]) {
    out.push(stars.len() as u8);
    for &(x, y) in stars {
        out.extend_from_slice(&x.to_be_bytes());
        out.extend_from_slice(&y.to_be_bytes());
    }
}

impl Message {
    fn tag(&self) -> u8 {
        match self {
            Message::Roster { .. } => 1,
            Message::Join { .. } => 2,
            Message::Leave => 3,
            Message::Ready { .. } => 4,
            Message::Seal { .. } => 5,
            Message::Witness { .. } => 6,
            Message::Weave { .. } => 7,
            Message::Stars { .. } => 8,
            Message::Lock { .. } => 9,
            Message::Pluck { .. } => 10,
            Message::TieOff { .. } => 11,
            Message::Reveal { .. } => 12,
            Message::Result { .. } => 13,
            Message::Share { .. } => 14,
            Message::End { .. } => 15,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![FORMAT, self.tag()];
        match self {
            Message::Roster {
                night,
                rev,
                round,
                settings,
                weaved,
                players,
            } => {
                out.extend_from_slice(&night.to_be_bytes());
                out.extend_from_slice(&[
                    *rev,
                    *round,
                    settings.weave_secs,
                    u8::from(settings.mixed),
                ]);
                out.extend_from_slice(&weaved.to_be_bytes());
                out.push(players.len() as u8);
                for p in players {
                    out.extend_from_slice(&p.to_be_bytes());
                }
            }
            Message::Join { play } => out.push(u8::from(*play)),
            Message::Leave => {}
            Message::Ready { round, rev } => out.extend_from_slice(&[*round, *rev]),
            Message::Seal {
                round,
                commit,
                used,
            } => {
                out.push(*round);
                out.extend_from_slice(commit);
                out.extend_from_slice(&used.to_be_bytes());
            }
            Message::Witness { round, nonce } => {
                out.push(*round);
                out.extend_from_slice(nonce);
            }
            Message::Weave { round, witnesses } => {
                out.push(*round);
                out.extend_from_slice(&witnesses.to_be_bytes());
            }
            Message::Stars { round, stars } | Message::Lock { round, stars } => {
                out.push(*round);
                put_stars(&mut out, stars);
            }
            Message::Pluck {
                round,
                target,
                star,
            } => {
                out.push(*round);
                out.extend_from_slice(&target.to_be_bytes());
                out.push(*star);
            }
            Message::TieOff { round, spare } => out.extend_from_slice(&[*round, *spare]),
            Message::Reveal {
                round,
                secret,
                locks,
            } => {
                out.push(*round);
                out.extend_from_slice(secret);
                out.extend_from_slice(&locks.to_be_bytes());
            }
            Message::Result { round, hash } => {
                out.push(*round);
                out.extend_from_slice(hash);
            }
            Message::Share { round, secret } => {
                out.push(*round);
                out.extend_from_slice(secret);
            }
            Message::End { night } => out.extend_from_slice(&night.to_be_bytes()),
        }
        out
    }

    /// Parses an untrusted message. None for another format, an unknown type, a wrong length
    /// or values out of range.
    pub fn decode(bytes: &[u8]) -> Option<Message> {
        if bytes.len() > MAX_LEN {
            return None;
        }
        let (&format, rest) = bytes.split_first()?;
        if format != FORMAT {
            return None;
        }
        let (&tag, body) = rest.split_first()?;
        let mut r = Reader(body);
        let message = match tag {
            1 => {
                let night = r.u32()?;
                let rev = r.u8()?;
                let round = r.u8()?;
                let settings = Settings {
                    weave_secs: r.u8()?,
                    mixed: r.bool()?,
                };
                let weaved = r.u16()?;
                let count = usize::from(r.u8()?);
                if !settings.valid() || count == 0 || count > MAX_PLAYERS {
                    return None;
                }
                let players = (0..count).map(|_| r.u32()).collect::<Option<Vec<_>>>()?;
                Message::Roster {
                    night,
                    rev,
                    round,
                    settings,
                    weaved,
                    players,
                }
            }
            2 => Message::Join { play: r.bool()? },
            3 => Message::Leave,
            4 => Message::Ready {
                round: r.u8()?,
                rev: r.u8()?,
            },
            5 => Message::Seal {
                round: r.u8()?,
                commit: r.array()?,
                used: u64::from_be_bytes(r.array()?),
            },
            6 => Message::Witness {
                round: r.u8()?,
                nonce: r.array()?,
            },
            7 => Message::Weave {
                round: r.u8()?,
                witnesses: r.u16()?,
            },
            8 | 9 => {
                let round = r.u8()?;
                let stars = r.stars()?;
                if tag == 8 {
                    Message::Stars { round, stars }
                } else {
                    Message::Lock { round, stars }
                }
            }
            10 => Message::Pluck {
                round: r.u8()?,
                target: r.u32()?,
                star: r.u8()?,
            },
            11 => Message::TieOff {
                round: r.u8()?,
                spare: r.u8()?,
            },
            12 => Message::Reveal {
                round: r.u8()?,
                secret: r.array()?,
                locks: r.u16()?,
            },
            13 => Message::Result {
                round: r.u8()?,
                hash: r.array()?,
            },
            14 => Message::Share {
                round: r.u8()?,
                secret: r.array()?,
            },
            15 => Message::End { night: r.u32()? },
            _ => return None,
        };
        r.0.is_empty().then_some(message)
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, tail) = self.0.split_first_chunk::<N>()?;
        self.0 = tail;
        Some(*head)
    }
    fn u8(&mut self) -> Option<u8> {
        self.array::<1>().map(|a| a[0])
    }
    fn bool(&mut self) -> Option<bool> {
        match self.u8()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
    fn u16(&mut self) -> Option<u16> {
        self.array().map(u16::from_be_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.array().map(u32::from_be_bytes)
    }
    fn stars(&mut self) -> Option<Vec<Pos>> {
        let count = usize::from(self.u8()?);
        if count == 0 || count > MAX_STARS {
            return None;
        }
        (0..count)
            .map(|_| {
                let x = self.u16()?;
                let y = self.u16()?;
                (x <= LOOM_W && y <= LOOM_H).then_some((x, y))
            })
            .collect()
    }
}
