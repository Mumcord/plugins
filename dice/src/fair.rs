//! Commit–witness–reveal maths (FR-6.82, issue #285). Every client runs exactly this code, so
//! everyone computes the same dice from the same messages.

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

use crate::notation::{Mode, Notation};

/// Domain separation tag for every hash in the dice protocol.
pub const TAG: &[u8] = b"mcdice1";
pub const SECRET_BYTES: usize = 16;

pub type Secret = [u8; SECRET_BYTES];
pub type Hash = [u8; 32];

/// `C = SHA-256("mcdice1" ‖ roll id ‖ notation ‖ R)`; fixes notation and secret before the
/// roller sees any witness value.
pub fn commitment(roll_id: u32, canonical: &str, secret: &Secret) -> Hash {
    Sha256::new()
        .chain_update(TAG)
        .chain_update(roll_id.to_be_bytes())
        .chain_update(canonical.as_bytes())
        .chain_update(secret)
        .finalize()
        .into()
}

/// `S = SHA-256("mcdice1" ‖ roll id ‖ R ‖ W1 ‖ W2)`, witnesses in reveal order.
pub fn seed(roll_id: u32, secret: &Secret, witnesses: &[Secret]) -> Hash {
    let mut hasher = Sha256::new()
        .chain_update(TAG)
        .chain_update(roll_id.to_be_bytes())
        .chain_update(secret);
    for witness in witnesses {
        hasher.update(witness);
    }
    hasher.finalize().into()
}

/// Deterministic number stream: blocks of `HMAC-SHA256(S, counter)` with a big-endian u32
/// counter from 0, read as big-endian u32 values.
pub struct Stream {
    seed: Hash,
    counter: u32,
    block: Hash,
    used: usize,
}

impl Stream {
    pub fn new(seed: Hash) -> Self {
        Self {
            seed,
            counter: 0,
            block: [0; 32],
            used: 32,
        }
    }

    fn next_u32(&mut self) -> u32 {
        if self.used == self.block.len() {
            // HMAC accepts keys of any length, so this cannot fail.
            let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&self.seed)
                .unwrap_or_else(|_| unreachable!("HMAC takes any key length"));
            mac.update(&self.counter.to_be_bytes());
            self.block = mac.finalize().into_bytes().into();
            self.counter = self.counter.wrapping_add(1);
            self.used = 0;
        }
        let bytes = [
            self.block[self.used],
            self.block[self.used + 1],
            self.block[self.used + 2],
            self.block[self.used + 3],
        ];
        self.used += 4;
        u32::from_be_bytes(bytes)
    }

    /// One die from 1 to `sides`. Values at the top of the u32 range that would make some faces
    /// more likely are thrown away (rejection sampling, no modulo bias).
    pub fn die(&mut self, sides: u16) -> u16 {
        let sides_wide = u64::from(sides.max(1));
        let accepted = (1u64 << 32) / sides_wide * sides_wide;
        loop {
            let value = u64::from(self.next_u32());
            if value < accepted {
                // value % sides < sides ≤ u16::MAX.
                return 1 + u16::try_from(value % sides_wide).unwrap_or(0);
            }
        }
    }
}

/// The dice of a finished roll, as every client computes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Every die thrown, in stream order.
    pub dice: Vec<u16>,
    /// Index into `dice` of the die that does not count (advantage, disadvantage, drop).
    pub dropped: Option<usize>,
    pub total: i32,
}

pub fn roll(notation: &Notation, seed: Hash) -> Outcome {
    let mut stream = Stream::new(seed);
    let dice: Vec<u16> = (0..notation.dice_thrown())
        .map(|_| stream.die(notation.sides))
        .collect();
    // Ties drop the later die, so the result is the same on every client.
    let lowest = || {
        dice.iter()
            .enumerate()
            .rev()
            .min_by_key(|(_, value)| **value)
            .map(|(i, _)| i)
    };
    // `max_by_key` already returns the last of equal elements, `min_by_key` the first.
    let highest = || {
        dice.iter()
            .enumerate()
            .max_by_key(|(_, value)| **value)
            .map(|(i, _)| i)
    };
    let dropped = match notation.mode {
        Mode::Plain => None,
        Mode::Advantage | Mode::DropLowest => lowest(),
        Mode::Disadvantage | Mode::DropHighest => highest(),
    };
    let kept: i32 = dice
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != dropped)
        .map(|(_, value)| i32::from(*value))
        .sum();
    Outcome {
        dice,
        dropped,
        total: kept + i32::from(notation.modifier),
    }
}
