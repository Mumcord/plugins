//! Commit–reveal for the figure draw, so the Weaver cannot pick an easy figure and Threaders'
//! clients never hold the figure before dawn.
//!
//! The Weaver commits to `SHA-256("skyloom1" ‖ secret ‖ round)` before anyone sends their
//! witness nonce. The figure comes from an HMAC stream keyed with the secret over every witness
//! nonce, with rejection sampling, so neither side alone controls it. At dawn the Weaver reveals
//! the secret and every client checks the seal and draws the figure itself.

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

use crate::deck;
use crate::wire::{Nonce, Pos, Secret};

const DOMAIN: &[u8] = b"skyloom1";

/// The seal the Weaver publishes before weaving.
pub fn commit(secret: &Secret, round: u8) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(DOMAIN);
    hash.update(secret);
    hash.update([round]);
    hash.finalize().into()
}

/// Bytes of the HMAC stream for this round's draw, block by block.
struct Stream {
    key: Secret,
    message: Vec<u8>,
    block: u32,
    buffer: Vec<u8>,
}

impl Stream {
    fn next(&mut self) -> u8 {
        if self.buffer.is_empty() {
            let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&self.key)
                .expect("HMAC takes keys of any length");
            mac.update(DOMAIN);
            mac.update(&self.block.to_be_bytes());
            mac.update(&self.message);
            self.block += 1;
            let mut bytes: Vec<u8> = mac.finalize().into_bytes().to_vec();
            bytes.reverse();
            self.buffer = bytes;
        }
        self.buffer.pop().unwrap_or(0)
    }
}

/// The deck index of this round's figure. `witnesses` are (roster slot, nonce) pairs in slot
/// order; `used` marks figures already woven this night, which are skipped while others are
/// left.
pub fn figure(secret: &Secret, round: u8, witnesses: &[(u8, Nonce)], used: u64) -> usize {
    let mut message = vec![round];
    for (slot, nonce) in witnesses {
        message.push(*slot);
        message.extend_from_slice(nonce);
    }
    let mut free: Vec<usize> = (0..deck::SIZE).filter(|i| used & (1 << i) == 0).collect();
    if free.is_empty() {
        free = (0..deck::SIZE).collect();
    }
    let mut stream = Stream {
        key: *secret,
        message,
        block: 0,
        buffer: Vec::new(),
    };
    // Rejection sampling keeps every free figure equally likely.
    let n = free.len();
    let limit = 256 - 256 % n;
    loop {
        let b = usize::from(stream.next());
        if b < limit {
            return free[b % n];
        }
    }
}

/// A short digest of a client's round result, compared between clients for "Verified".
pub fn result_hash(round: u8, figure: usize, stars: &[(u32, Pos)], resonance: u8) -> [u8; 8] {
    let mut hash = Sha256::new();
    hash.update(DOMAIN);
    hash.update([round, figure as u8, resonance]);
    for (session, (x, y)) in stars {
        hash.update(session.to_be_bytes());
        hash.update(x.to_be_bytes());
        hash.update(y.to_be_bytes());
    }
    let digest: [u8; 32] = hash.finalize().into();
    let mut out = [0; 8];
    out.copy_from_slice(&digest[..8]);
    out
}

/// A public, deterministic number for this night and round (twist choice, drift direction).
/// Not secret: everyone may know it in advance.
pub fn public_draw(night: u32, round: u8, what: &[u8]) -> u32 {
    let mut hash = Sha256::new();
    hash.update(DOMAIN);
    hash.update(night.to_be_bytes());
    hash.update([round]);
    hash.update(what);
    let digest: [u8; 32] = hash.finalize().into();
    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
}
