//! Snack Race: the pets race for a snack. A quick tap-timing game drawn in the plugin's web
//! view (`web/race.html`); the invite, lobby and results are a native card like Echo Echo's.
//!
//! The lobby works like Echo Echo's: the starter sends an invite, everyone who joins within
//! [`LOBBY_SECS`] waits, and the host's start message starts the race on every joined client.
//! Each player then runs on their own clock for at most [`RACE_MS`]: a marker sweeps over a
//! timing bar and every tap while it is on the snack makes the pet hop once; a miss makes the
//! pet stumble for a moment. The first [`GOAL`] hops reach the snack. Because every client
//! times only its own run, message delays never decide who wins.
//!
//! While racing, a client shares its hops a few times per second inside a game burst so the
//! other pets move on everyone's track; without a burst only the final score goes out.
//! [`RESULTS_SECS`] after the start every client shows the scores it received.
//!
//! Wire format, version 1 (integers big-endian), everything received is untrusted:
//! - kind 6 invite: `version u8 · race id u32 · lobby secs u8` (0 = it already started)
//! - kind 7 start: `version u8 · race id u32`, from the host when the lobby closes
//! - kind 8 progress: `version u8 · race id u32 · hops u8`
//! - kind 9 score: `version u8 · race id u32 · hops u8 · finish ms u16` (ms only counts when
//!   hops reached [`GOAL`])

pub mod page;
mod view;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

pub use view::RaceView;

pub const INVITE: u8 = 6;
pub const START: u8 = 7;
pub const PROGRESS: u8 = 8;
pub const SCORE: u8 = 9;

/// Hops from the start line to the snack.
pub const GOAL: u8 = 10;
/// How long one player's run may take.
pub const RACE_MS: u32 = 15_000;
/// A finish faster than this is impossible with the page's stumble and sweep timing, so a
/// score claiming it is a forgery and dropped.
pub const MIN_FINISH_MS: u16 = 2_000;
/// From the start until the results are shown: the run plus time for the scores to arrive.
pub const RESULTS_SECS: u64 = 20;
pub const RESULTS_MS: u32 = 20_000;
pub const LOBBY_SECS: u64 = 30;
pub const LOBBY_MS: u32 = 30_000;
/// How long a joined player waits past the lobby for a start that may never come.
pub const NO_START_GRACE_SECS: u64 = 15;
/// Progress goes out at most this often while racing.
pub const PROGRESS_MS: u32 = 350;
/// Where the snack zone sits on the timing bar, in percent; the page keeps it inside the bar.
const ZONE_MIN: u8 = 15;
const ZONE_MAX: u8 = 85;

const VERSION: u8 = 1;

/// `ui-action` names of the race card and the panel.
pub mod action {
    pub const START: &str = "race";
    pub const JOIN: &str = "race-join";
    pub const OPEN: &str = "race-open";
    pub const START_NOW: &str = "race-start";
    pub const CLOSE: &str = "race-close";
}

fn header(id: u32) -> Vec<u8> {
    let mut out = vec![VERSION];
    out.extend_from_slice(&id.to_be_bytes());
    out
}

/// The race id after the version byte, and what follows it.
fn split_header(bytes: &[u8]) -> Option<(u32, &[u8])> {
    let (&version, rest) = bytes.split_first()?;
    if version < VERSION {
        return None;
    }
    let id = u32::from_be_bytes(rest.get(..4)?.try_into().ok()?);
    Some((id, &rest[4..]))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Invite {
    pub id: u32,
    /// Seconds until the race starts; 0 = it already started.
    pub lobby: u8,
}

impl Invite {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = header(self.id);
        out.push(self.lobby);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Invite> {
        let (id, rest) = split_header(bytes)?;
        // A peer cannot make others wait longer than our own lobby.
        let lobby = (*rest.first()?).min(u8::try_from(LOBBY_SECS).unwrap_or(u8::MAX));
        Some(Invite { id, lobby })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Start {
    pub id: u32,
}

impl Start {
    pub fn encode(&self) -> Vec<u8> {
        header(self.id)
    }

    pub fn decode(bytes: &[u8]) -> Option<Start> {
        split_header(bytes).map(|(id, _)| Start { id })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub id: u32,
    pub hops: u8,
}

impl Progress {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = header(self.id);
        out.push(self.hops);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Progress> {
        let (id, rest) = split_header(bytes)?;
        let hops = *rest.first()?;
        (hops <= GOAL).then_some(Progress { id, hops })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Score {
    pub id: u32,
    pub hops: u8,
    /// Finish time in ms; only meaningful when `hops` is [`GOAL`].
    pub ms: u16,
}

impl Score {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = header(self.id);
        out.push(self.hops);
        out.extend_from_slice(&self.ms.to_be_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Score> {
        let (id, rest) = split_header(bytes)?;
        let hops = *rest.first()?;
        let ms = u16::from_be_bytes(rest.get(1..3)?.try_into().ok()?);
        let max_ms = u16::try_from(RACE_MS).unwrap_or(u16::MAX);
        let valid = hops < GOAL || (MIN_FINISH_MS..=max_ms).contains(&ms);
        (hops <= GOAL && valid).then_some(Score { id, hops, ms })
    }

    /// Sort key: more hops first, then the faster finish.
    fn rank(&self) -> (std::cmp::Reverse<u8>, u16) {
        let ms = if self.hops == GOAL { self.ms } else { u16::MAX };
        (std::cmp::Reverse(self.hops), ms)
    }
}

/// Where the snack zone sits on the timing bar for each hop, in percent. Everyone in a race
/// gets the same course from its id, so nobody has an easier bar.
pub fn course(id: u32) -> Vec<u8> {
    // splitmix64, so neighbouring ids give unrelated courses.
    let mut state = u64::from(id);
    (0..GOAL)
        .map(|_| {
            state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^= z >> 31;
            let span = u64::from(ZONE_MAX - ZONE_MIN) + 1;
            ZONE_MIN + u8::try_from(z % span).unwrap_or(0)
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Someone else started it; the user has not joined.
    Invited,
    /// In the lobby: the user hosts or joined and waits for the start.
    Waiting,
    /// The user's pet is running.
    Racing,
    /// The user's run is over; waiting for the results.
    Finished,
    /// Results are shown.
    Over,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runner {
    /// Display name from the server, already made safe.
    pub name: String,
    pub hops: u8,
    /// The final score once it arrived.
    pub score: Option<Score>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Race {
    pub id: u32,
    pub phase: Phase,
    /// Unix seconds when the lobby closes; None once the race started.
    pub starts_at: Option<u64>,
    /// Unix seconds when the results show (or a race that never started is given up).
    pub ends_at: u64,
    /// Unix seconds of the start, once it started.
    pub started_at: Option<u64>,
    /// Who started it: None = the user, else the session and display name.
    pub host: Option<(u32, String)>,
    /// Other pets on the track, by session.
    pub runners: BTreeMap<u32, Runner>,
    /// The user's hops so far.
    pub hops: u8,
    /// The user's final score once the run is over.
    pub mine: Option<Score>,
}

impl Race {
    /// The user starts a race: the lobby opens and the user waits with the others.
    pub fn open(id: u32, now: u64) -> Race {
        Race {
            id,
            phase: Phase::Waiting,
            starts_at: Some(now + LOBBY_SECS),
            ends_at: now + LOBBY_SECS + RESULTS_SECS,
            started_at: None,
            host: None,
            runners: BTreeMap::new(),
            hops: 0,
            mine: None,
        }
    }

    pub fn invited(invite: Invite, host: u32, host_name: String, now: u64) -> Race {
        let lobby = u64::from(invite.lobby);
        let (starts_at, started_at, ends_at) = if lobby == 0 {
            (None, Some(now), now + RESULTS_SECS)
        } else {
            (Some(now + lobby), None, now + lobby + NO_START_GRACE_SECS)
        };
        Race {
            id: invite.id,
            phase: Phase::Invited,
            starts_at,
            ends_at,
            started_at,
            host: Some((host, host_name)),
            runners: BTreeMap::new(),
            hops: 0,
            mine: None,
        }
    }

    pub fn card_id(&self) -> String {
        format!("snack-race-{}", self.id)
    }

    pub fn is_over(&self) -> bool {
        self.phase == Phase::Over
    }

    pub fn is_host(&self) -> bool {
        self.host.is_none()
    }

    /// True if the user joined now: into the lobby, or straight into a race that started.
    pub fn join(&mut self) -> bool {
        if self.phase != Phase::Invited {
            return false;
        }
        self.phase = if self.starts_at.is_some() {
            Phase::Waiting
        } else {
            Phase::Racing
        };
        true
    }

    /// The lobby closes and the race starts for the host and whoever joined. False if it
    /// already started or is over.
    pub fn begin(&mut self, now: u64) -> bool {
        if self.starts_at.is_none() || self.is_over() {
            return false;
        }
        self.starts_at = None;
        self.started_at = Some(now);
        self.ends_at = now + RESULTS_SECS;
        if self.phase == Phase::Waiting {
            self.phase = Phase::Racing;
        }
        true
    }

    /// The host's start message arrived; one from anyone else or for another race is ignored.
    pub fn start_received(&mut self, session: u32, start: Start, now: u64) -> bool {
        let from_host = self.host.as_ref().is_some_and(|(host, _)| *host == session);
        from_host && start.id == self.id && self.begin(now)
    }

    /// The race ended without a start: the host left before the lobby closed.
    pub fn never_started(&self) -> bool {
        self.is_over() && self.starts_at.is_some()
    }

    /// The page reported the user's hop count. Only one hop more than before counts, so a
    /// repeated or skipped report cannot jump the pet ahead. Returns the score when this hop
    /// reached the snack.
    pub fn hop(&mut self, hops: u8, ms: u32, now: u64) -> Option<Score> {
        if self.phase != Phase::Racing || hops != self.hops + 1 {
            return None;
        }
        self.hops = hops;
        (hops == GOAL).then(|| self.finish(ms, now))
    }

    /// The user's run is over: the snack was reached (`hop`) or the time ran out. A finish time
    /// below what the clock allows (the page was reopened mid-race) is raised to it.
    pub fn finish(&mut self, ms: u32, now: u64) -> Score {
        let elapsed = self
            .started_at
            .map_or(0, |at| now.saturating_sub(at).saturating_sub(1) * 1000);
        let ms = u64::from(ms)
            .max(elapsed)
            .clamp(u64::from(MIN_FINISH_MS), u64::from(RACE_MS));
        let score = Score {
            id: self.id,
            hops: self.hops,
            ms: u16::try_from(ms).unwrap_or(u16::MAX),
        };
        self.phase = Phase::Finished;
        self.mine = Some(score);
        score
    }

    /// The race timer ran out; returns the score if the user was still running.
    pub fn time_up(&mut self, now: u64) -> Option<Score> {
        (self.phase == Phase::Racing).then(|| self.finish(RACE_MS, now))
    }

    /// Someone's hops while racing; ignored for other races, after their score or the end.
    pub fn progress_received(&mut self, session: u32, name: String, progress: Progress) -> bool {
        if progress.id != self.id || self.is_over() || self.starts_at.is_some() {
            return false;
        }
        let runner = self.runners.entry(session).or_insert(Runner {
            name,
            hops: 0,
            score: None,
        });
        if runner.score.is_some() || progress.hops <= runner.hops {
            return false;
        }
        runner.hops = progress.hops;
        true
    }

    /// Records someone's final score; the first one per session counts.
    pub fn score_received(&mut self, session: u32, name: String, score: Score) -> bool {
        if score.id != self.id || self.is_over() {
            return false;
        }
        let runner = self.runners.entry(session).or_insert(Runner {
            name,
            hops: 0,
            score: None,
        });
        if runner.score.is_some() {
            return false;
        }
        runner.hops = score.hops;
        runner.score = Some(score);
        true
    }

    pub fn end(&mut self) {
        self.phase = Phase::Over;
    }

    /// Final scores, best first; None as name = the user. Pets that never sent a score are
    /// left out: their run may have ended without one ever arriving.
    pub fn results(&self) -> Vec<(Option<&str>, Score)> {
        let mut results: Vec<(Option<&str>, Score)> = self
            .runners
            .values()
            .filter_map(|r| r.score.map(|s| (Some(r.name.as_str()), s)))
            .chain(self.mine.map(|m| (None, m)))
            .collect();
        results.sort_by_key(|(_, score)| score.rank());
        results
    }

    /// Whether the user takes part (hosts or joined).
    pub fn playing(&self) -> bool {
        self.phase != Phase::Invited
    }
}
