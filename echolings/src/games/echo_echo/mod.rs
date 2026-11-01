//! Echo Echo: the pets hum a short tune and everyone repeats it. A memory game that works
//! without voice, drawn with native components.
//!
//! The starter sends an invite with the tune to the channel; everyone who joins listens, repeats
//! it on four pads and sends their score to the channel. After [`GAME_SECS`] every client shows
//! the scores it received. Two messages per player per game, so no game burst is needed.
//!
//! Wire format, version 1 (integers big-endian), everything received is untrusted:
//! - kind 3 invite: `version u8 · game id u32 · tune length u8 · notes u8…` (notes below
//!   [`PADS`], length [`MIN_TUNE`]…[`MAX_TUNE`])
//! - kind 4 score: `version u8 · game id u32 · correct notes u8`

mod view;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

pub use view::GameView;

pub const INVITE: u8 = 3;
pub const SCORE: u8 = 4;

pub const PADS: u8 = 4;
pub const TUNE_LEN: usize = 5;
pub const MIN_TUNE: usize = 3;
pub const MAX_TUNE: usize = 8;
/// How long each note of the tune is shown.
pub const STEP_MS: u32 = 700;
/// From the invite until the scores are shown.
pub const GAME_SECS: u64 = 45;
pub const GAME_MS: u32 = 45_000;

const VERSION: u8 = 1;

/// `ui-action` names of the game card.
pub mod action {
    pub const JOIN: &str = "echo-join";
    pub const CLOSE: &str = "echo-close";
    const PAD_PREFIX: &str = "echo-pad:";

    pub fn pad(pad: u8) -> String {
        format!("{PAD_PREFIX}{pad}")
    }

    pub fn parse_pad(action: &str) -> Option<u8> {
        action
            .strip_prefix(PAD_PREFIX)?
            .parse()
            .ok()
            .filter(|p| *p < super::PADS)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    pub id: u32,
    pub tune: Vec<u8>,
}

impl Invite {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![VERSION];
        out.extend_from_slice(&self.id.to_be_bytes());
        out.push(u8::try_from(self.tune.len()).unwrap_or(0));
        out.extend_from_slice(&self.tune);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Invite> {
        let (&version, rest) = bytes.split_first()?;
        if version < VERSION || rest.len() < 5 {
            return None;
        }
        let id = u32::from_be_bytes(rest[..4].try_into().ok()?);
        let len = usize::from(rest[4]);
        let tune = rest.get(5..5 + len)?.to_vec();
        let valid = (MIN_TUNE..=MAX_TUNE).contains(&len) && tune.iter().all(|n| *n < PADS);
        valid.then_some(Invite { id, tune })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Score {
    pub id: u32,
    pub correct: u8,
}

impl Score {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![VERSION];
        out.extend_from_slice(&self.id.to_be_bytes());
        out.push(self.correct);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Score> {
        let (&version, rest) = bytes.split_first()?;
        if version < VERSION || rest.len() < 5 {
            return None;
        }
        Some(Score {
            id: u32::from_be_bytes(rest[..4].try_into().ok()?),
            correct: rest[4],
        })
    }
}

/// A tune of [`TUNE_LEN`] notes from a seed. Not secret: anyone could read the invite anyway.
pub fn tune(seed: u64) -> Vec<u8> {
    // splitmix64, so neighbouring seeds (e.g. consecutive seconds) give unrelated tunes.
    let mut state = seed;
    (0..TUNE_LEN)
        .map(|_| {
            state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^= z >> 31;
            u8::try_from(z % u64::from(PADS)).unwrap_or(0)
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Someone else started it; the user has not joined.
    Invited,
    /// The tune plays; `step` is the note shown now.
    Listen { step: usize },
    /// The user repeats; `entered` notes are right so far.
    Repeat { entered: usize },
    /// The user is done and waits for the others.
    Finished { correct: u8 },
    /// Scores are shown.
    Over,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player {
    /// Display name from the server, already made safe.
    pub name: String,
    pub correct: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    pub id: u32,
    pub tune: Vec<u8>,
    pub phase: Phase,
    /// Unix seconds when the scores are shown.
    pub ends_at: u64,
    /// Who started it: None = the user, else the session and display name.
    pub host: Option<(u32, String)>,
    /// Scores of others, by session; the first score per session counts.
    pub scores: BTreeMap<u32, Player>,
    /// The user's own score once finished.
    pub mine: Option<u8>,
}

impl Game {
    /// The user starts a game: the tune plays right away.
    pub fn start(id: u32, tune: Vec<u8>, now: u64) -> Game {
        Game {
            id,
            tune,
            phase: Phase::Listen { step: 0 },
            ends_at: now + GAME_SECS,
            host: None,
            scores: BTreeMap::new(),
            mine: None,
        }
    }

    pub fn invited(invite: Invite, host: u32, host_name: String, now: u64) -> Game {
        Game {
            id: invite.id,
            tune: invite.tune,
            phase: Phase::Invited,
            ends_at: now + GAME_SECS,
            host: Some((host, host_name)),
            scores: BTreeMap::new(),
            mine: None,
        }
    }

    pub fn card_id(&self) -> String {
        format!("echo-echo-{}", self.id)
    }

    pub fn is_over(&self) -> bool {
        self.phase == Phase::Over
    }

    /// True if the user joined now.
    pub fn join(&mut self) -> bool {
        if self.phase != Phase::Invited {
            return false;
        }
        self.phase = Phase::Listen { step: 0 };
        true
    }

    /// The note shown right now, while the tune plays.
    pub fn playing(&self) -> Option<u8> {
        match self.phase {
            Phase::Listen { step } => self.tune.get(step).copied(),
            _ => None,
        }
    }

    /// Moves the tune on by one note; false once the tune is over and it is the user's turn.
    pub fn step(&mut self) -> bool {
        let Phase::Listen { step } = self.phase else {
            return false;
        };
        if step + 1 < self.tune.len() {
            self.phase = Phase::Listen { step: step + 1 };
            true
        } else {
            self.phase = Phase::Repeat { entered: 0 };
            false
        }
    }

    /// The user pressed a pad. Returns the score when this press finished the user's turn: the
    /// whole tune, or the first wrong note.
    pub fn press(&mut self, pad: u8) -> Option<u8> {
        let Phase::Repeat { entered } = self.phase else {
            return None;
        };
        let correct = if self.tune.get(entered) == Some(&pad) {
            entered + 1
        } else {
            entered
        };
        if correct == entered || correct == self.tune.len() {
            let score = u8::try_from(correct).unwrap_or(u8::MAX);
            self.phase = Phase::Finished { correct: score };
            self.mine = Some(score);
            Some(score)
        } else {
            self.phase = Phase::Repeat { entered: correct };
            None
        }
    }

    /// Records someone's score for this game; scores for other games, after the end or
    /// repeated ones are ignored. Returns true if it was recorded.
    pub fn score_received(&mut self, session: u32, name: String, score: Score) -> bool {
        if score.id != self.id || self.is_over() || self.scores.contains_key(&session) {
            return false;
        }
        let max = u8::try_from(self.tune.len()).unwrap_or(u8::MAX);
        self.scores.insert(
            session,
            Player {
                name,
                correct: score.correct.min(max),
            },
        );
        true
    }

    pub fn end(&mut self) {
        self.phase = Phase::Over;
    }

    /// Scores, best first; None as name = the user.
    pub fn results(&self) -> Vec<(Option<&str>, u8)> {
        let mut results: Vec<(Option<&str>, u8)> = self
            .scores
            .values()
            .map(|p| (Some(p.name.as_str()), p.correct))
            .chain(self.mine.map(|m| (None, m)))
            .collect();
        results.sort_by_key(|r| std::cmp::Reverse(r.1));
        results
    }
}
