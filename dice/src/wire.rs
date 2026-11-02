//! Binary messages (FR-6.82). All integers are big-endian. Every message is far below the
//! 997-byte payload limit (the largest, a reveal with two witnesses, is 53 bytes).
//!
//! - kind 1 commit, roller → channel: roll id u32, commitment 32 B, canonical notation (rest).
//! - kind 2 witness, witness → channel: roller session u32, roll id u32, witness value 16 B.
//! - kind 3 reveal, roller → channel: roll id u32, secret 16 B, count u8 (0–2), witness values
//!   16 B each.

use crate::fair::{Hash, SECRET_BYTES, Secret};
use crate::notation::MAX_CANONICAL_BYTES;

pub const KIND_COMMIT: u8 = 1;
pub const KIND_WITNESS: u8 = 2;
pub const KIND_REVEAL: u8 = 3;

/// At most two witness values count for a roll.
pub const MAX_WITNESSES: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Commit {
        roll_id: u32,
        commitment: Hash,
        notation: String,
    },
    Witness {
        roller: u32,
        roll_id: u32,
        value: Secret,
    },
    Reveal {
        roll_id: u32,
        secret: Secret,
        witnesses: Vec<Secret>,
    },
}

impl Message {
    pub fn kind(&self) -> u8 {
        match self {
            Message::Commit { .. } => KIND_COMMIT,
            Message::Witness { .. } => KIND_WITNESS,
            Message::Reveal { .. } => KIND_REVEAL,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            Message::Commit {
                roll_id,
                commitment,
                notation,
            } => {
                out.extend_from_slice(&roll_id.to_be_bytes());
                out.extend_from_slice(commitment);
                out.extend_from_slice(notation.as_bytes());
            }
            Message::Witness {
                roller,
                roll_id,
                value,
            } => {
                out.extend_from_slice(&roller.to_be_bytes());
                out.extend_from_slice(&roll_id.to_be_bytes());
                out.extend_from_slice(value);
            }
            Message::Reveal {
                roll_id,
                secret,
                witnesses,
            } => {
                out.extend_from_slice(&roll_id.to_be_bytes());
                out.extend_from_slice(secret);
                // Never more than MAX_WITNESSES, so this fits a byte.
                out.push(u8::try_from(witnesses.len()).unwrap_or(u8::MAX));
                for witness in witnesses {
                    out.extend_from_slice(witness);
                }
            }
        }
        out
    }

    /// Strict: anything with the wrong length or content is `None`.
    pub fn decode(kind: u8, bytes: &[u8]) -> Option<Message> {
        let mut reader = Reader(bytes);
        let message = match kind {
            KIND_COMMIT => {
                let roll_id = reader.u32()?;
                let commitment = reader.array::<32>()?;
                let rest = reader.rest();
                if rest.is_empty() || rest.len() > MAX_CANONICAL_BYTES {
                    return None;
                }
                Message::Commit {
                    roll_id,
                    commitment,
                    notation: String::from_utf8(rest.to_vec()).ok()?,
                }
            }
            KIND_WITNESS => Message::Witness {
                roller: reader.u32()?,
                roll_id: reader.u32()?,
                value: reader.array::<SECRET_BYTES>()?,
            },
            KIND_REVEAL => {
                let roll_id = reader.u32()?;
                let secret = reader.array::<SECRET_BYTES>()?;
                let count = usize::from(reader.u8()?);
                if count > MAX_WITNESSES {
                    return None;
                }
                let witnesses = (0..count)
                    .map(|_| reader.array::<SECRET_BYTES>())
                    .collect::<Option<Vec<_>>>()?;
                Message::Reveal {
                    roll_id,
                    secret,
                    witnesses,
                }
            }
            _ => return None,
        };
        reader.rest().is_empty().then_some(message)
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let (head, tail) = self.0.split_at_checked(n)?;
        self.0 = tail;
        Some(head)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    fn u32(&mut self) -> Option<u32> {
        self.array::<4>().map(u32::from_be_bytes)
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    fn rest(&mut self) -> &[u8] {
        std::mem::take(&mut self.0)
    }
}
