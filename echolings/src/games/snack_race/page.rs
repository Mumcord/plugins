//! The bridge between the plugin and the race page (`web/race.html`).
//!
//! The page sends plain strings: `ready` once it listens, `start` for Start now, and
//! `{"hop":n,"ms":m}` for each hop of the user's pet. The plugin answers with the whole state
//! as JSON (see [`PageState`]) whenever something changed. The page does no i18n of its own:
//! every text comes from the plugin, already in the user's language.

use serde::{Deserialize, Serialize};

use super::{GOAL, Phase, RACE_MS, Race, course};
use crate::storage::AnimationLevel;
use crate::ui::{Motion, i18n::I18n};

/// Snack Race needs the web view; without it the race is hidden (FR-6.83).
pub const PERMISSION: &str = "ui.webview";
/// The race page below `web/`.
pub const ENTRY: &str = "race.html";
/// The page's first message; everything posted before it was lost.
pub const READY: &str = "ready";
/// Start now, from the host's page.
pub const START: &str = "start";
/// At most this many pets on the track besides the user's, so a big channel stays readable.
const MAX_RUNNERS: usize = 11;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FromPage {
    Ready,
    Start,
    Hop { hops: u8, ms: u32 },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HopMessage {
    hop: u8,
    ms: u32,
}

/// The page's message, or None for anything else (the page is ours, but stay strict).
pub fn parse(message: &str) -> Option<FromPage> {
    match message {
        READY => Some(FromPage::Ready),
        START => Some(FromPage::Start),
        _ => {
            let hop: HopMessage = serde_json::from_str(message).ok()?;
            (hop.hop <= GOAL).then_some(FromPage::Hop {
                hops: hop.hop,
                ms: hop.ms,
            })
        }
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct PageRunner {
    pub name: String,
    /// Picture below `web/`, e.g. `pets/fizzle-buddy-ember-small.svg`; None draws a plain pet.
    pub pet: Option<String>,
    pub hops: u8,
    pub me: bool,
}

/// The page's texts in the user's language.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Strings {
    pub lang: String,
    pub title: String,
    pub status: String,
    pub hop: String,
    pub hint: String,
    pub starts_in: String,
    pub start_now: String,
    /// "{{n}} of {{goal}} hops": the page fills in `n`.
    pub hops: String,
    pub stumble: String,
    pub time_left: String,
    pub results: String,
    pub track: String,
    pub bar: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PageState {
    pub id: u32,
    /// `waiting`, `racing`, `finished` or `over`.
    pub phase: &'static str,
    pub host: bool,
    /// Seconds until the lobby closes, while waiting.
    pub starts_in: Option<u64>,
    /// Time left of the user's run, while racing.
    pub left_ms: u32,
    pub goal: u8,
    pub race_ms: u32,
    /// Snack zone per hop, in percent of the timing bar.
    pub course: Vec<u8>,
    /// `full`, `subtle` or `still`.
    pub motion: &'static str,
    pub dark: bool,
    pub runners: Vec<PageRunner>,
    /// Result lines, best first, once the race is over.
    pub results: Vec<String>,
    pub strings: Strings,
}

/// Everything the page needs from the plugin besides the race.
pub struct Look<'a> {
    pub t: &'a I18n,
    pub language: &'a str,
    pub motion: Motion,
    pub dark: bool,
    /// The user's own pet: name and picture.
    pub me: PageRunner,
    /// Pictures of other pets, by session.
    pub pet_of: &'a dyn Fn(u32) -> Option<String>,
}

fn motion_name(motion: Motion) -> &'static str {
    match motion.level {
        _ if motion.reduced => "still",
        AnimationLevel::Still => "still",
        AnimationLevel::Subtle => "subtle",
        AnimationLevel::Full => "full",
    }
}

/// A picture path under `assets/` as the page sees it under `web/`.
pub fn web_picture(asset: &str) -> Option<String> {
    asset.strip_prefix("assets/").map(str::to_owned)
}

pub fn state(
    race: &Race,
    look: Look<'_>,
    now: u64,
    status: String,
    results: Vec<String>,
) -> String {
    let t = look.t;
    let phase = match race.phase {
        Phase::Invited | Phase::Waiting => "waiting",
        Phase::Racing => "racing",
        Phase::Finished => "finished",
        Phase::Over => "over",
    };
    let left_ms = race.started_at.map_or(RACE_MS, |at| {
        let spent = now.saturating_sub(at).saturating_mul(1000);
        u32::try_from(u64::from(RACE_MS).saturating_sub(spent)).unwrap_or(0)
    });
    let mut others: Vec<(&u32, &super::Runner)> = race.runners.iter().collect();
    others.sort_by_key(|(_, r)| std::cmp::Reverse(r.hops));
    let mut runners = vec![PageRunner {
        hops: race.hops,
        ..look.me
    }];
    runners.extend(
        others
            .into_iter()
            .take(MAX_RUNNERS)
            .map(|(session, r)| PageRunner {
                name: r.name.clone(),
                pet: (look.pet_of)(*session),
                hops: r.hops,
                me: false,
            }),
    );
    let state = PageState {
        id: race.id,
        phase,
        host: race.is_host(),
        starts_in: race.starts_at.map(|at| at.saturating_sub(now)),
        left_ms,
        goal: GOAL,
        race_ms: RACE_MS,
        course: course(race.id),
        motion: motion_name(look.motion),
        dark: look.dark,
        runners,
        results,
        strings: Strings {
            lang: look.language.to_owned(),
            title: t.t("race.title"),
            status,
            hop: t.t("race.hop"),
            hint: t.t("race.hint"),
            starts_in: t.t("race.startsIn"),
            start_now: t.t("race.startNow"),
            hops: t.f("race.hops", &[("goal", &GOAL.to_string())]),
            stumble: t.t("race.stumble"),
            time_left: t.t("race.timeLeft"),
            results: t.t("race.results"),
            track: t.t("race.track"),
            bar: t.t("race.bar"),
        },
    };
    serde_json::to_string(&state).unwrap_or_default()
}
