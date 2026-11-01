//! The dice card, the error card and the history panel. Pure functions, so tests can inspect
//! the trees without a host.

use mumcord_plugin_sdk::ui::{self, ButtonKind, El, Sprite};

use crate::fair::Outcome;
use crate::notation::{Mode, Notation, NotationError};
use crate::text::{t, t_with, tn};

/// Six die faces side by side, played as a tumbling die while the roll waits for witnesses.
pub const DIE_SPRITE: &str = "assets/die-faces.svg";
const DIE_SIZE: u16 = 48;
const HISTORY_ROWS: usize = 20;
/// Names are untrusted server text; long ones are cut so the card stays readable.
const MAX_NAME_CHARS: usize = 32;

/// Why a roll failed its check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Commitment,
    Witness,
    LeftOut,
}

impl Reason {
    fn text(self) -> String {
        t(match self {
            Reason::Commitment => "reason.commitment",
            Reason::Witness => "reason.witness",
            Reason::LeftOut => "reason.left_out",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Rolling,
    Verified { witnesses: usize },
    Alone,
    NotRevealed,
    Failed { reason: Reason },
}

impl Status {
    pub fn label(&self) -> String {
        t(match self {
            Status::Rolling => "status.rolling",
            Status::Verified { .. } => "status.verified",
            Status::Alone => "status.alone",
            Status::NotRevealed => "status.not_revealed",
            Status::Failed { .. } => "status.failed",
        })
    }

    fn color(&self) -> &'static str {
        match self {
            Status::Rolling => "text-muted",
            Status::Verified { .. } => "success",
            Status::Alone => "warning",
            Status::NotRevealed | Status::Failed { .. } => "danger",
        }
    }

    fn explanation(&self) -> String {
        match self {
            Status::Rolling => t("explain.rolling"),
            Status::Verified { witnesses } => tn("explain.verified", *witnesses as u64),
            Status::Alone => t("explain.alone"),
            Status::NotRevealed => t("explain.not_revealed"),
            Status::Failed { reason } => t_with("explain.failed", &[("reason", &reason.text())]),
        }
    }
}

/// Why `/roll` showed an error card instead of a roll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    Notation(NotationError),
    Busy,
    NoRandom,
    RateLimited,
    NotSent,
}

impl Problem {
    pub fn text(self) -> String {
        t(match self {
            Problem::Notation(NotationError::Unknown) => "error.unknown",
            Problem::Notation(NotationError::Modifier) => "error.modifier",
            Problem::Notation(NotationError::DiceCount) => "error.dice_count",
            Problem::Notation(NotationError::Sides) => "error.sides",
            Problem::Notation(NotationError::AdvantageNeedsOneDie) => "error.advantage",
            Problem::Notation(NotationError::DropNeedsTwoDice) => "error.drop",
            Problem::Busy => "error.busy",
            Problem::NoRandom => "error.no_random",
            Problem::RateLimited => "error.rate_limited",
            Problem::NotSent => "error.not_sent",
        })
    }
}

/// The roll as the card names it, e.g. "d20 with advantage" in the user's language.
fn describe(notation: &Notation) -> String {
    let mut plain = *notation;
    plain.mode = Mode::Plain;
    let base = plain.canonical();
    let key = match notation.mode {
        Mode::Plain => return base,
        Mode::Advantage => "roll.advantage",
        Mode::Disadvantage => "roll.disadvantage",
        Mode::DropLowest => "roll.drop_lowest",
        Mode::DropHighest => "roll.drop_highest",
    };
    t_with(key, &[("roll", &base)])
}

/// Who rolled, as the card shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Who {
    Me,
    Peer(String),
}

impl Who {
    fn subject(&self) -> String {
        match self {
            Who::Me => t("you"),
            Who::Peer(name) => name.chars().take(MAX_NAME_CHARS).collect(),
        }
    }
}

pub struct Card<'a> {
    pub who: &'a Who,
    pub notation: &'a Notation,
    pub status: &'a Status,
    pub outcome: Option<&'a Outcome>,
}

pub fn card(card: &Card<'_>) -> El {
    let title = match card.who {
        Who::Me => t_with("card.title_me", &[("roll", &describe(card.notation))]),
        Who::Peer(_) => t_with(
            "card.title",
            &[
                ("who", &card.who.subject()),
                ("roll", &describe(card.notation)),
            ],
        ),
    };
    let mut root = ui::column().child(ui::heading(&title));
    match (card.status, card.outcome) {
        (Status::Rolling, _) => {
            root = root.child(
                ui::row()
                    .child(ui::sprite(Sprite {
                        asset: DIE_SPRITE.to_string(),
                        alt: t("card.tumbling"),
                        frame_width: DIE_SIZE,
                        frame_height: DIE_SIZE,
                        frames: 6,
                        fps: 10,
                        still_frame: 4,
                    }))
                    .child(ui::progress(&t("card.waiting"), None)),
            );
        }
        (_, Some(outcome)) => {
            let mut dice = ui::row();
            for (i, value) in outcome.dice.iter().enumerate() {
                dice = if outcome.dropped == Some(i) {
                    dice.child(ui::chip(&dropped(*value)).color("text-muted"))
                } else {
                    dice.child(ui::chip(&value.to_string()))
                };
            }
            root = root
                .child(dice.child(ui::heading(&format!("= {}", outcome.total))))
                .child(ui::caption(&spoken(card.notation, outcome, card.status)));
        }
        _ => {}
    }
    root.child(ui::chip(&card.status.label()).color(card.status.color()))
        .child(ui::caption(&card.status.explanation()))
}

fn dropped(value: u16) -> String {
    t_with("dice.dropped", &[("value", &value.to_string())])
}

/// Text alternative for the dice: "4, 4, plus 3, total 11, verified".
pub fn spoken(notation: &Notation, outcome: &Outcome, status: &Status) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (i, value) in outcome.dice.iter().enumerate() {
        if outcome.dropped == Some(i) {
            parts.push(dropped(*value));
        } else {
            parts.push(value.to_string());
        }
    }
    if notation.modifier > 0 {
        parts.push(t_with(
            "spoken.plus",
            &[("n", &notation.modifier.to_string())],
        ));
    } else if notation.modifier < 0 {
        parts.push(t_with(
            "spoken.minus",
            &[("n", &notation.modifier.unsigned_abs().to_string())],
        ));
    }
    parts.push(t_with("spoken.total", &[("n", &outcome.total.to_string())]));
    parts.push(status.label().to_lowercase());
    parts.join(", ")
}

/// The plain-text line for people without the plugin (always English: other people read it), sent once after the reveal. Mumcord adds
/// the "[Dice roller] " prefix and Mumble shows who sent it:
/// `🎲 rolled 2d6+3 = 11 (4, 4)`, `🎲 rolled 4d6 drop lowest = 14 (6, 5, 3, dropped 2)`.
pub fn fallback_line(notation: &Notation, outcome: &Outcome, status: &Status) -> String {
    let dice: Vec<String> = match notation.mode {
        Mode::DropLowest | Mode::DropHighest => {
            let mut kept: Vec<String> = outcome
                .dice
                .iter()
                .enumerate()
                .filter(|(i, _)| outcome.dropped != Some(*i))
                .map(|(_, value)| value.to_string())
                .collect();
            if let Some(value) = outcome.dropped.and_then(|i| outcome.dice.get(i)) {
                kept.push(format!("dropped {value}"));
            }
            kept
        }
        _ => outcome.dice.iter().map(u16::to_string).collect(),
    };
    let mut line = format!(
        "🎲 rolled {} = {} ({})",
        notation.described(),
        outcome.total,
        dice.join(", ")
    );
    if *status == Status::Alone {
        line.push_str(" · unverified, rolled alone");
    }
    line
}

/// Shown instead of a roll when `/roll` cannot start; only the user sees it.
pub fn error_card(problem: Problem) -> El {
    ui::column()
        .child(ui::text(&problem.text()).color("danger"))
        .child(ui::caption(&t("error.hint")))
}

/// One finished roll in the history panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub who: Who,
    pub notation: Notation,
    pub total: Option<i32>,
    pub status: Status,
}

/// "Mallory: d20 = 14" or "You: d20 void"; void rolls never show a total.
fn entry_text(entry: &Entry) -> String {
    let result = match (entry.total, &entry.status) {
        (Some(total), Status::Verified { .. } | Status::Alone) => format!("= {total}"),
        _ => t("history.void"),
    };
    t_with(
        "history.row",
        &[
            ("who", &entry.who.subject()),
            ("roll", &describe(&entry.notation)),
            ("result", &result),
        ],
    )
}

/// The history list: rolls of this session on this device, newest first.
fn history_list(entries: &[Entry]) -> El {
    let mut root = ui::column().child(ui::heading(&t("history.title")));
    if entries.is_empty() {
        return root.child(ui::caption(&t("history.empty")));
    }
    let mut list = ui::list(&t("history.list"));
    for entry in entries.iter().rev().take(HISTORY_ROWS) {
        list = list.child(
            ui::row()
                .child(ui::text(&entry_text(entry)))
                .child(ui::chip(&entry.status.label()).color(entry.status.color())),
        );
    }
    root = root.child(list);
    root
}

/// Action id of the help icon button.
pub const HELP_ACTION: &str = "help";
/// Prefix of the quick-roll buttons' action ids; the rest is the notation to roll.
pub const QUICK_PREFIX: &str = "quick:";
/// The quick-roll buttons, in rows of four so the inline panel (260 x 160, no scrolling) fits.
const QUICK_ROWS: [&[&str]; 3] = [
    &["d4", "d6", "d8", "d10"],
    &["d12", "d20", "d100", "2d6"],
    &["d20 adv", "d20 dis"],
];

fn quick_rolls() -> El {
    let mut root = ui::column();
    for row in QUICK_ROWS {
        let mut buttons = ui::row();
        for notation in row {
            buttons = buttons.child(
                ui::button(&format!("{QUICK_PREFIX}{notation}"), notation)
                    .kind(ButtonKind::Secondary),
            );
        }
        root = root.child(buttons);
    }
    root
}

/// The newest roll in one small line, or a hint when nothing was rolled yet.
fn last_roll(entries: &[Entry]) -> El {
    ui::caption(&match entries.last() {
        Some(entry) => t_with("panel.last", &[("row", &entry_text(entry))]),
        None => t("panel.none"),
    })
}

/// The panel shown inline in the Plugins strip: quick rolls, the last roll and the help button.
pub fn panel_inline(entries: &[Entry]) -> El {
    quick_rolls().child(ui::row().child(last_roll(entries)).child(ui::icon_button(
        HELP_ACTION,
        "assets/icons/help.svg",
        &t("help.label"),
    )))
}

/// Action ids of the two section headers in the large popover.
pub const TOGGLE_HELP_ACTION: &str = "toggle:help";
pub const TOGGLE_MARKS_ACTION: &str = "toggle:marks";

/// A full-width header button; the arrow shows whether the section below is open.
fn section_header(action: &str, title: &str, open: bool) -> El {
    let arrow = if open { "\u{25be}" } else { "\u{25b8}" };
    ui::button(action, &format!("{arrow} {title}"))
        .kind(ButtonKind::Secondary)
        .expanded(open)
}

/// Which help sections of the large popover are unfolded.
#[derive(Clone, Copy, Default)]
pub struct Sections {
    pub help: bool,
    pub marks: bool,
}

/// The panel in the large popover: the collapsible help and the history.
pub fn panel_expanded(entries: &[Entry], open: Sections) -> El {
    let mut root = ui::column().child(section_header(
        TOGGLE_HELP_ACTION,
        &t("help.title"),
        open.help,
    ));
    if open.help {
        root = root
            .child(ui::caption(&t("help.notation")))
            .child(ui::caption(&t("help.advantage")))
            .child(ui::caption(&t("help.drop")))
            .child(ui::caption(&t("help.limits")))
            .child(ui::caption(&t("help.buttons")));
    }
    root = root.child(section_header(
        TOGGLE_MARKS_ACTION,
        &t("help.marks"),
        open.marks,
    ));
    if open.marks {
        for (status, key) in [
            (Status::Verified { witnesses: 2 }, "help.verified"),
            (Status::Alone, "help.alone"),
            (Status::NotRevealed, "help.not_revealed"),
        ] {
            root = root
                .child(ui::chip(&status.label()).color(status.color()))
                .child(ui::caption(&t(key)));
        }
    }
    root.child(ui::caption(&t("help.history")))
        .child(history_list(entries))
}
