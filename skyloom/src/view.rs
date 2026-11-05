//! What the plugin shows: the JSON it posts to its web page, the intents it accepts back, the
//! chat cards and the plain "[Skyloom]" lines for standard Mumble clients.

use mumcord_plugin_sdk::ui::{self, El};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::{App, Phase, Place, Record, Round, Verify};
use crate::deck;
use crate::rules::Twist;
use crate::score;
use crate::text::{t, t_with};

/// Longest night name and spectator guess, in characters.
pub const MAX_NAME: usize = 40;
pub const MAX_GUESS: usize = 32;

/// What the page may ask for. Everything is checked again by the app.
#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    Ready,
    Move { star: u8, x: i32, y: i32 },
    Settle { star: u8 },
    Pluck { target: u32, star: u8 },
    TieOff,
    Begin,
    WeaveReady,
    Settings { weave_secs: u8, mixed: bool },
    Seat { play: bool },
    Tapestry,
    PostSummary,
    AnotherNight,
    Name(String),
    Guess(String),
    Volume(u8),
    Mute(bool),
    CloseNight,
    Place(Place),
    Rules,
}

#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "kebab-case")]
enum RawIntent {
    Ready,
    Move { i: u8, x: i32, y: i32 },
    Settle { i: u8 },
    Pluck { s: u32, i: u8 },
    TieOff,
    Begin,
    WeaveReady,
    Settings { weave: u8, mixed: bool },
    Seat { play: bool },
    Tapestry,
    PostSummary,
    AnotherNight,
    Name { text: String },
    Guess { text: String },
    Volume { v: u8 },
    Mute { on: bool },
    CloseNight,
    Place { to: String },
    Rules,
}

/// Keeps at most `max` characters and drops control characters.
fn clean(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

impl Intent {
    /// Parses a message from the page; None for anything unexpected.
    pub fn parse(message: &str) -> Option<Intent> {
        if message.len() > 512 {
            return None;
        }
        Some(match serde_json::from_str::<RawIntent>(message).ok()? {
            RawIntent::Ready => Intent::Ready,
            RawIntent::Move { i, x, y } => Intent::Move { star: i, x, y },
            RawIntent::Settle { i } => Intent::Settle { star: i },
            RawIntent::Pluck { s, i } => Intent::Pluck { target: s, star: i },
            RawIntent::TieOff => Intent::TieOff,
            RawIntent::Begin => Intent::Begin,
            RawIntent::WeaveReady => Intent::WeaveReady,
            RawIntent::Settings { weave, mixed } => Intent::Settings {
                weave_secs: weave,
                mixed,
            },
            RawIntent::Seat { play } => Intent::Seat { play },
            RawIntent::Tapestry => Intent::Tapestry,
            RawIntent::PostSummary => Intent::PostSummary,
            RawIntent::AnotherNight => Intent::AnotherNight,
            RawIntent::Name { text } => Intent::Name(clean(&text, MAX_NAME)),
            RawIntent::Guess { text } => Intent::Guess(clean(&text, MAX_GUESS)),
            RawIntent::Volume { v } => Intent::Volume(v.min(100)),
            RawIntent::Mute { on } => Intent::Mute(on),
            RawIntent::CloseNight => Intent::CloseNight,
            RawIntent::Place { to } => Intent::Place(match to.as_str() {
                "stage" => Place::Stage,
                "side-panel" => Place::SidePanel,
                "dialog" => Place::Dialog,
                _ => return None,
            }),
            RawIntent::Rules => Intent::Rules,
        })
    }
}

/// The page's translated texts: every `page.*` key.
pub fn strings() -> String {
    let en: serde_json::Map<String, Value> =
        serde_json::from_str(include_str!("../i18n/en.json")).unwrap_or_default();
    let texts: serde_json::Map<String, Value> = en
        .keys()
        .filter_map(|k| {
            k.strip_prefix("page.")
                .map(|short| (short.to_string(), json!(t(k))))
        })
        .collect();
    json!({ "t": "strings", "s": texts }).to_string()
}

pub fn figure_name(index: usize) -> String {
    deck::get(index)
        .map(|f| t(&format!("figure.{}", f.id)))
        .unwrap_or_default()
}

/// The English figure name for the plain chat lines, which every reader shares: "two-moths" →
/// "Two Moths", others get "The": "heron" → "The Heron".
pub fn english_name(index: usize) -> String {
    let Some(figure) = deck::get(index) else {
        return String::new();
    };
    let words: Vec<String> = figure
        .id
        .split('-')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .collect();
    let numbered = matches!(words.first().map(String::as_str), Some("Two" | "Three"));
    if numbered {
        words.join(" ")
    } else {
        format!("The {}", words.join(" "))
    }
}

fn place_key(place: Option<Place>) -> &'static str {
    match place {
        Some(Place::SidePanel) => "side-panel",
        Some(Place::Dialog) => "dialog",
        _ => "stage",
    }
}

fn phase_key(phase: Phase) -> &'static str {
    match phase {
        Phase::Seal => "seal",
        Phase::Weave => "weave",
        Phase::Dawn => "dawn",
        Phase::Reveal => "reveal",
        Phase::Void => "void",
    }
}

fn verify_key(verify: &Verify) -> &'static str {
    match verify {
        Verify::Pending => "pending",
        Verify::Verified => "verified",
        Verify::SealBroken => "seal-broken",
        Verify::Differs => "differs",
        Verify::Missing => "missing",
    }
}

fn round_pts(points: &[(f64, f64)]) -> Vec<[i32; 2]> {
    points
        .iter()
        .map(|p| [p.0.round() as i32, p.1.round() as i32])
        .collect()
}

/// The Weaver's private sky card: the figure's lines and points in grid units × 16.
fn sky_card(app: &App, r: &Round) -> Value {
    if !r.is_weaver(app.me) || r.phase == Phase::Seal {
        return Value::Null;
    }
    let Some(index) = r.known_figure() else {
        return Value::Null;
    };
    let Some(figure) = deck::get(index) else {
        return Value::Null;
    };
    let n =
        (r.threaders.len() - r.left.iter().filter(|l| r.is_threader(**l)).count()) * r.stars_each;
    let mirror = r.twist == Twist::Mirror;
    let flip = |p: (i32, i32)| if mirror { crate::rules::mirror(p) } else { p };
    let mut points: Vec<(i32, i32)> = figure.sample(n.max(2)).into_iter().map(flip).collect();
    // Twin Weavers: each sees half of the figure's points.
    if let Some(twin) = r.twin {
        let half = points.len() / 2;
        points = if twin == app.me {
            points.split_off(half)
        } else {
            points.truncate(half);
            points
        };
    }
    let lines: Vec<Vec<[i32; 2]>> = figure
        .lines()
        .iter()
        .map(|l| {
            l.iter()
                .map(|&(x, y)| {
                    let p = flip((x * 16, y * 16));
                    [p.0, p.1]
                })
                .collect()
        })
        .collect();
    json!({
        "name": figure_name(index),
        "lines": if r.twin.is_some() { json!([]) } else { json!(lines) },
        "points": points.iter().map(|p| [p.0, p.1]).collect::<Vec<_>>(),
        "mirror": mirror,
    })
}

fn stars_json(app: &App, r: &Round) -> Vec<Value> {
    r.all_stars()
        .into_iter()
        .filter(|s| app.visible(r, s.2, s.0))
        .map(|(owner, i, (x, y))| json!([owner, i, x, y, r.locks.contains_key(&owner)]))
        .collect()
}

/// Just the stars, for frequent updates.
pub fn stars(app: &App) -> Option<String> {
    let r = app.night.as_ref()?.round.as_ref()?;
    Some(json!({ "t": "stars", "r": r.no, "s": stars_json(app, r) }).to_string())
}

pub fn talk(session: u32, on: bool) -> String {
    json!({ "t": "talk", "s": session, "on": on }).to_string()
}

pub fn pluck(by: u32, target: u32, star: u8) -> String {
    json!({ "t": "pluck", "by": by, "s": target, "i": star }).to_string()
}

pub fn say(text: &str) -> String {
    json!({ "t": "say", "text": text }).to_string()
}

fn reveal_json(app: &App, r: &Round) -> Value {
    let Some(o) = &r.outcome else {
        return Value::Null;
    };
    let truest = (0..o.stars.len()).max_by_key(|&i| o.stars[i].3);
    let loosest = (0..o.stars.len()).min_by_key(|&i| o.stars[i].3);
    // The figure part each star was matched to, for "96% on the beak".
    let parts = o
        .figure
        .and_then(deck::get)
        .map(|f| f.parts(o.stars.len()))
        .unwrap_or_default();
    let note = |star: Option<usize>, form: &str| {
        let part = parts.get(*o.order.get(star?)?)?;
        Some(crate::text::t(&format!("part.{part}.{form}")))
    };
    json!({
        "figure": o.figure.map(figure_name),
        "resonance": o.resonance,
        "bonus": o.bonus,
        "spare": o.spare,
        "woven": o.resonance >= score::WOVEN,
        "gold": o.resonance >= score::GOLD,
        "stars": o.stars.iter().map(|s| json!([s.0, s.1, s.2.0, s.2.1, s.3])).collect::<Vec<_>>(),
        "targets": round_pts(&o.targets),
        "order": o.order,
        "ghost": o.ghost.iter().map(|l| round_pts(l)).collect::<Vec<_>>(),
        "verify": verify_key(&o.verify),
        "truest": truest.map(|i| o.stars[i].0),
        "loosest": loosest.map(|i| o.stars[i].0),
        "truestAt": note(truest, "at"),
        "loosestLost": note(loosest, "lost"),
        "guess": if app.guess.is_empty() { Value::Null } else { json!(app.guess) },
    })
}

fn honours(app: &App) -> Value {
    let Some(night) = &app.night else {
        return Value::Null;
    };
    let silver = night
        .history
        .iter()
        .filter(|r| r.resonance.is_some())
        .max_by_key(|r| (r.resonance, std::cmp::Reverse(r.no)))
        .map(|r| r.weaver);
    let mut truest_counts: std::collections::BTreeMap<u32, u32> = Default::default();
    for r in &night.history {
        if let Some(t) = r.truest {
            *truest_counts.entry(t).or_default() += 1;
        }
    }
    let truest = truest_counts
        .iter()
        .max_by_key(|(s, c)| (**c, std::cmp::Reverse(**s)))
        .map(|(s, _)| *s);
    let wanderer = night
        .travel
        .iter()
        .max_by_key(|(s, d)| (**d, std::cmp::Reverse(**s)))
        .map(|(s, _)| *s);
    json!({
        "silver": silver.map(|s| app.name(s)),
        "truest": truest.map(|s| app.name(s)),
        "truestCount": truest.and_then(|s| truest_counts.get(&s)).copied().unwrap_or(0),
        "truestOf": night.history.iter().filter(|r| r.truest.is_some()).count(),
        "wanderer": wanderer.map(|s| app.name(s)),
    })
}

/// The whole view for the page.
pub fn view(app: &App) -> String {
    let me = app.me;
    let Some(night) = &app.night else {
        return json!({ "t": "view", "phase": "none", "me": me, "place": place_key(app.place) })
            .to_string();
    };
    let round = night.round.as_ref();
    let players: Vec<Value> = night
        .players
        .iter()
        .enumerate()
        .map(|(slot, &s)| {
            json!({
                "s": s,
                "name": app.name(s),
                "shape": slot % 12,
                "talking": app.talking.contains(&s),
                "weaved": night.weaved.contains(&s),
            })
        })
        .collect();
    let spectators: Vec<String> = night.spectators.iter().map(|s| app.name(*s)).collect();
    let phase = if night.ended {
        "results"
    } else {
        match round {
            Some(r) => phase_key(r.phase),
            // Joined while a round runs: watch until the next one starts.
            None if night.round_no > 0 => "between",
            None => "lobby",
        }
    };
    let role = match round {
        _ if !night.players.contains(&me) => "spectator",
        Some(r) if !r.finished() && r.is_weaver(me) => "weaver",
        Some(r) if !r.finished() && r.is_threader(me) => "threader",
        Some(r) if !r.finished() => "spectator",
        _ => "player",
    };
    let round_json = round.map(|r| {
        json!({
            "no": r.no,
            "of": night.total_rounds(),
            "weaver": r.weaver,
            "twin": r.twin,
            "twist": r.twist.key(),
            "msLeft": r.deadline.saturating_sub(app.now),
            "dur": night.settings.weave_secs,
            "stars": stars_json(app, r),
            "each": r.stars_each,
            "left": r.left.iter().collect::<Vec<_>>(),
            "card": sky_card(app, r),
            "reveal": reveal_json(app, r),
            "void": r.void_reason,
        })
    });
    let results = night.ended.then(|| {
        json!({
            "rounds": night.history.iter().map(|r| json!({
                "no": r.no,
                "weaver": app.name(r.weaver),
                "figure": r.figure.map(figure_name),
                "lines": r.figure.and_then(deck::get).map(|f| f.lines()),
                "resonance": r.resonance,
                "verified": r.verified,
            })).collect::<Vec<_>>(),
            "team": night.team(),
            "honours": honours(app),
            "name": night.sky_name,
            "summaryPosted": night.summary_posted,
        })
    });
    let past: Vec<Value> = app
        .prefs
        .skies
        .iter()
        .rev()
        .map(|s| json!({ "name": s.name, "team": s.team, "count": s.figures.len() }))
        .collect();
    json!({
        "t": "view",
        "me": me,
        "place": place_key(app.place),
        "phase": phase,
        "role": role,
        "lamplighter": night.lamplighter(),
        "players": players,
        "spectators": spectators,
        "settings": { "weave": night.settings.weave_secs, "mixed": night.settings.mixed },
        "round": round_json,
        "next": night.next_weaver(),
        "canEnd": night.lamplighter() == Some(me) && night.next_weaver().is_none() && !night.ended,
        "results": results,
        "past": past,
        "sound": { "volume": app.prefs.volume, "muted": app.prefs.muted },
        "theme": if app.dark { "dark" } else { "light" },
        "reduceMotion": app.reduce_motion,
        "live": app.live,
    })
    .to_string()
}

/// Screen reader announcements for phase changes, already translated.
pub fn announcement(app: &App, what: &str) -> Option<String> {
    let night = app.night.as_ref()?;
    let r = night.round.as_ref();
    let weaver = r.map(|r| app.name(r.weaver)).unwrap_or_default();
    Some(match what {
        "weave" => t_with(
            "say.weave",
            &[
                ("secs", &night.settings.weave_secs.to_string()),
                ("name", &weaver),
            ],
        ),
        "first-light" => t("say.first-light"),
        "dawn" => t("say.dawn"),
        "reveal" => {
            let o = r?.outcome.as_ref()?;
            let figure = o.figure.map(figure_name).unwrap_or_default();
            let mine = o.stars.iter().find(|s| s.0 == app.me).map(|s| s.3);
            let base = t_with(
                "say.reveal",
                &[("figure", &figure), ("res", &o.resonance.to_string())],
            );
            match mine {
                Some(m) => format!(
                    "{base} {}",
                    t_with("say.reveal-mine", &[("res", &m.to_string())])
                ),
                None => base,
            }
        }
        "results" => t_with(
            "say.results",
            &[("res", &night.team().unwrap_or(0).to_string())],
        ),
        _ => return None,
    })
}

// ----- Chat cards (local items for Mumcord users) -----

pub fn invite_card_id(activity: &str) -> String {
    format!("invite-{activity}")
}

pub fn round_card_id(night: u32, round: u8) -> String {
    format!("round-{night}-{round}")
}

pub fn summary_card_id(night: u32) -> String {
    format!("summary-{night}")
}

pub fn invite_card(name: &str, ended: bool) -> El {
    let column = ui::column()
        .child(ui::heading(&t("card.invite-title")))
        .child(ui::text(&t_with("card.invite", &[("name", name)])));
    if ended {
        column
    } else {
        column.child(ui::button("join", &t("card.join")))
    }
}

pub fn round_card(app: &App, record: &Record) -> El {
    let title = match record.figure {
        Some(f) => figure_name(f),
        None => t("card.clouds"),
    };
    let detail = match record.resonance {
        Some(res) => {
            let key = if res >= score::GOLD {
                "card.round-gold"
            } else if res >= score::WOVEN {
                "card.round-woven"
            } else {
                "card.round-loose"
            };
            t_with(
                key,
                &[
                    ("round", &record.no.to_string()),
                    ("res", &res.to_string()),
                    ("name", &app.name(record.weaver)),
                ],
            )
        }
        None => t_with("card.round-void", &[("round", &record.no.to_string())]),
    };
    let status = if record.verified {
        t("card.verified")
    } else {
        t("card.unverified")
    };
    ui::column()
        .child(ui::heading(&title))
        .child(ui::text(&detail))
        .child(ui::caption(&status))
}

pub fn summary_card(app: &App) -> El {
    let Some(night) = &app.night else {
        return ui::column();
    };
    let woven = night
        .history
        .iter()
        .filter(|r| r.resonance.is_some_and(|x| x >= score::WOVEN))
        .count();
    ui::column()
        .child(ui::heading(&t("card.summary-title")))
        .child(ui::text(&t_with(
            "card.summary",
            &[
                ("woven", &woven.to_string()),
                ("rounds", &night.history.len().to_string()),
                ("res", &night.team().unwrap_or(0).to_string()),
            ],
        )))
}

// ----- Plain lines for standard Mumble clients (English, shared by everyone) -----

pub fn start_line(name: &str) -> String {
    format!(
        "[Skyloom] {name} opened a night of Skyloom: one player describes a hidden \
         constellation out loud, everyone else moves a star to match it. Join from the Apps \
         menu in Mumcord, or just listen and guess along."
    )
}

pub fn round_line(app: &App, record: &Record) -> String {
    let weaver = app.name(record.weaver);
    match (record.figure, record.resonance) {
        (Some(f), Some(res)) => {
            let outcome = if res >= score::GOLD {
                "woven in gold thread"
            } else if res >= score::WOVEN {
                "woven into the sky"
            } else {
                "the threads came loose"
            };
            format!(
                "[Skyloom] Round {}: {} described by {weaver}, {res}% resonance, {outcome}.",
                record.no,
                english_name(f)
            )
        }
        _ => format!(
            "[Skyloom] Round {}: clouds rolled in, no constellation.",
            record.no
        ),
    }
}

pub fn summary_line(app: &App) -> String {
    let Some(night) = &app.night else {
        return String::new();
    };
    let woven = night
        .history
        .iter()
        .filter(|r| r.resonance.is_some_and(|x| x >= score::WOVEN))
        .count();
    let mut line = format!(
        "[Skyloom] The night is over: {woven} of {} constellations woven, team resonance {}%.",
        night.history.len(),
        night.team().unwrap_or(0)
    );
    let h = honours(app);
    for (key, label) in [
        ("silver", "Silver tongue"),
        ("truest", "Truest star"),
        ("wanderer", "Wanderer"),
    ] {
        if let Some(name) = h.get(key).and_then(Value::as_str) {
            line.push_str(&format!(" {label}: {name}."));
        }
    }
    line.chars().take(500).collect()
}
