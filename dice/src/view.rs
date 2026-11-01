//! The dice card, the error card and the history panel. Pure functions, so tests can inspect
//! the trees without a host.

use mumcord_plugin_sdk::ui::{self, El, Sprite};

use crate::fair::Outcome;
use crate::notation::{HINT, Mode, Notation};

/// Six die faces side by side, played as a tumbling die while the roll waits for witnesses.
pub const DIE_SPRITE: &str = "assets/die-faces.svg";
const DIE_SIZE: u16 = 48;
const HISTORY_ROWS: usize = 20;
/// Names are untrusted server text; long ones are cut so the card stays readable.
const MAX_NAME_CHARS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Rolling,
    Verified { witnesses: usize },
    Alone,
    NotRevealed,
    Failed { reason: String },
}

impl Status {
    pub fn label(&self) -> &'static str {
        match self {
            Status::Rolling => "Rolling",
            Status::Verified { .. } => "Verified",
            Status::Alone => "Unverified (rolled alone)",
            Status::NotRevealed => "Not revealed",
            Status::Failed { .. } => "Verification failed",
        }
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
            Status::Rolling => "Waiting for other players to add their part.".to_string(),
            Status::Verified { witnesses } => format!(
                "Checked with {witnesses} {}: the roller locked in a secret before anyone added \
                 theirs, so nobody could pick the result.",
                if *witnesses == 1 {
                    "witness"
                } else {
                    "witnesses"
                }
            ),
            Status::Alone => {
                "Nobody else with Dice roller could witness this roll, so it was not checked."
                    .to_string()
            }
            Status::NotRevealed => {
                "The roller never revealed this roll, so it is void and does not count.".to_string()
            }
            Status::Failed { reason } => format!("{reason} This roll does not count."),
        }
    }
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
            Who::Me => "You".to_string(),
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
    let mut root = ui::column().child(ui::heading(&format!(
        "{} rolled {}",
        card.who.subject(),
        card.notation.described()
    )));
    match (card.status, card.outcome) {
        (Status::Rolling, _) => {
            root = root.child(
                ui::row()
                    .child(ui::sprite(Sprite {
                        asset: DIE_SPRITE.to_string(),
                        alt: "Dice tumbling".to_string(),
                        frame_width: DIE_SIZE,
                        frame_height: DIE_SIZE,
                        frames: 6,
                        fps: 10,
                        still_frame: 4,
                    }))
                    .child(ui::progress("Waiting for witnesses", None)),
            );
        }
        (_, Some(outcome)) => {
            let mut dice = ui::row();
            for (i, value) in outcome.dice.iter().enumerate() {
                dice = if outcome.dropped == Some(i) {
                    dice.child(ui::chip(&format!("{value} dropped")).color("text-muted"))
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
    root.child(ui::chip(card.status.label()).color(card.status.color()))
        .child(ui::caption(&card.status.explanation()))
}

/// Text alternative for the dice: "4, 4, plus 3, total 11, verified".
pub fn spoken(notation: &Notation, outcome: &Outcome, status: &Status) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (i, value) in outcome.dice.iter().enumerate() {
        if outcome.dropped == Some(i) {
            parts.push(format!("{value} dropped"));
        } else {
            parts.push(value.to_string());
        }
    }
    if notation.modifier > 0 {
        parts.push(format!("plus {}", notation.modifier));
    } else if notation.modifier < 0 {
        parts.push(format!("minus {}", notation.modifier.unsigned_abs()));
    }
    parts.push(format!("total {}", outcome.total));
    parts.push(status.label().to_lowercase());
    parts.join(", ")
}

/// The plain-text line for people without the plugin, sent once after the reveal. Mumcord adds
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
pub fn error_card(message: &str) -> El {
    ui::column()
        .child(ui::text(message).color("danger"))
        .child(ui::caption(HINT))
}

/// One finished roll in the history panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub who: Who,
    pub notation: Notation,
    pub total: Option<i32>,
    pub status: Status,
}

/// The optional history popover (`ui.panel`): rolls of this session on this device, newest
/// first. Void rolls are listed but never show a total.
pub fn history(entries: &[Entry]) -> El {
    let mut root = ui::column().child(ui::heading("Roll history"));
    if entries.is_empty() {
        return root.child(ui::caption("No rolls yet. Type /roll d20 in the chat."));
    }
    let mut list = ui::list("Rolls of this session");
    for entry in entries.iter().rev().take(HISTORY_ROWS) {
        let result = match (entry.total, &entry.status) {
            (Some(total), Status::Verified { .. } | Status::Alone) => format!("= {total}"),
            _ => "void".to_string(),
        };
        list = list.child(
            ui::row()
                .child(ui::text(&format!(
                    "{}: {} {result}",
                    entry.who.subject(),
                    entry.notation.described()
                )))
                .child(ui::chip(entry.status.label()).color(entry.status.color())),
        );
    }
    root = root.child(list);
    root
}
