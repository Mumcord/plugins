//! The chat card and the dialog, built from a parsed fit. Pure functions, so tests can inspect
//! the trees without a host. Every name shown comes from the pasted text: the plugin ships no
//! EVE data and no EVE images.

use mumcord_plugin_sdk::{
    i18n::Strings,
    ui::{self, Align, ButtonKind, Direction, El, Justify},
};

use crate::app::Pasted;
use crate::eft::{Fit, Group, Item, Section};

pub const ACTION_OPEN: &str = "open";

/// Host limit for any text string in a tree (FR-6.42).
const MAX_TEXT_CHARS: usize = 200;
/// Dialog title and subtitle limit.
const MAX_TITLE_CHARS: usize = 64;
/// Modules named on the card; the card is 440×160 px, so one line of them.
const CARD_MODULES: usize = 3;
/// About one line of the card.
const CARD_LINE_CHARS: usize = 70;

/// The card in the chat: ship, fit name and who pasted it, the slot summary, the most common
/// modules and the Open button.
pub fn card(s: &Strings, p: &Pasted) -> El {
    let fit = &p.fit;
    let byline = if fit.name.is_empty() {
        s.t_with("card.pasted_by", &[("sender", &p.sender)])
    } else {
        s.t_with("card.byline", &[("name", &fit.name), ("sender", &p.sender)])
    };
    let title = ui::stack(Direction::Column, 2, Align::Start)
        .child(ui::heading(&cut(&fit.ship, CARD_LINE_CHARS)))
        .child(ui::caption(&cut(&byline, CARD_LINE_CHARS)));
    let top = ui::row().justify(Justify::SpaceBetween).child(title).child(
        ui::row()
            .child(copy_button(s, p))
            .child(ui::button(ACTION_OPEN, &s.t("card.open")).kind(ButtonKind::Primary)),
    );
    let mut chips = ui::stack(Direction::Row, 4, Align::Center);
    for group in &fit.groups {
        if group.section != Section::Other {
            chips = chips.child(ui::chip(&summary(s, group)));
        }
    }
    let mut column = ui::stack(Direction::Column, 6, Align::Stretch)
        .child(top)
        .child(chips);
    let modules = top_modules(s, fit);
    if !modules.is_empty() {
        column = column.child(ui::caption(&modules));
    }
    column
}

/// A card whose fit is no longer kept: the Open button is gone.
pub fn dropped_card(s: &Strings, ship: &str) -> El {
    ui::stack(Direction::Column, 4, Align::Start)
        .child(ui::heading(&cut(ship, CARD_LINE_CHARS)))
        .child(ui::caption(&s.t("card.dropped")))
}

pub fn dialog_title(fit: &Fit) -> String {
    // The fit name is the user's own text, so it needs no translation.
    let title = if fit.name.is_empty() {
        &fit.ship
    } else {
        &fit.name
    };
    cut(title, MAX_TITLE_CHARS)
}

pub fn dialog_subtitle(s: &Strings, p: &Pasted) -> String {
    cut(
        &s.t_with(
            "dialog.subtitle",
            &[("ship", &p.fit.ship), ("sender", &p.sender)],
        ),
        MAX_TITLE_CHARS,
    )
}

/// The full fit, one block per section.
pub fn dialog(s: &Strings, p: &Pasted) -> El {
    let fit = &p.fit;
    let mut column = ui::column().child(ui::row().child(copy_button(s, p)));
    for (i, group) in fit.groups.iter().enumerate() {
        if i > 0 {
            column = column.child(ui::divider());
        }
        let mut block =
            ui::stack(Direction::Column, 4, Align::Stretch).child(ui::heading(&summary(s, group)));
        for item in &group.items {
            block = block.child(ui::text(&cut(&item_line(s, item), MAX_TEXT_CHARS)));
        }
        if group.empty > 0 {
            block = block.child(ui::caption(&s.tn(
                "item.empty",
                u64::from(group.empty),
                &[],
            )));
        }
        column = column.child(block);
    }
    column
}

/// Copies the fit exactly as it was pasted, ready to import into EVE or Pyfa; the host does the
/// copying when the user clicks, so the plugin needs no clipboard access.
fn copy_button(s: &Strings, p: &Pasted) -> El {
    ui::copy_button(&s.t("button.copy"), &p.text)
}

/// "High 7/8" for slots, "Drones 5" for bays.
pub fn summary(s: &Strings, group: &Group) -> String {
    let section = s.t(section_key(group.section));
    let filled = group.filled().to_string();
    if group.section.is_slot() {
        let total = (group.filled().saturating_add(group.empty)).to_string();
        s.t_with(
            "summary.slots",
            &[
                ("section", &section),
                ("filled", &filled),
                ("total", &total),
            ],
        )
    } else {
        s.t_with(
            "summary.count",
            &[("section", &section), ("count", &filled)],
        )
    }
}

fn section_key(section: Section) -> &'static str {
    match section {
        Section::Low => "section.low",
        Section::Mid => "section.mid",
        Section::High => "section.high",
        Section::Rig => "section.rig",
        Section::Subsystem => "section.subsystem",
        Section::Drones => "section.drones",
        Section::DronesOrCargo => "section.drones_or_cargo",
        Section::Cargo => "section.cargo",
        Section::Other => "section.other",
    }
}

/// "2× 200mm AutoCannon II · Republic Fleet EMP S (offline)".
fn item_line(s: &Strings, item: &Item) -> String {
    let count = item.count.to_string();
    let mut line = s.t_with("item.line", &[("count", &count), ("name", &item.name)]);
    if let Some(charge) = &item.charge {
        line = s.t_with("item.with_charge", &[("line", &line), ("charge", charge)]);
    }
    if item.offline {
        line = s.t_with("item.offline", &[("line", &line)]);
    }
    line
}

/// The most common fitted modules, e.g. "3× Gyrostabilizer II · 2× 200mm AutoCannon II".
fn top_modules(s: &Strings, fit: &Fit) -> String {
    let mut modules: Vec<&Item> = fit
        .groups
        .iter()
        .filter(|g| g.section.is_slot())
        .flat_map(|g| &g.items)
        .collect();
    // Stable sort: equal counts keep the fit's order (low slots first).
    modules.sort_by_key(|item| std::cmp::Reverse(item.count));
    // Whole entries only, as many as fit the line; the first one is cut if it must be.
    let mut line = String::new();
    for item in modules.iter().take(CARD_MODULES) {
        let count = item.count.to_string();
        let entry = s.t_with("item.line", &[("count", &count), ("name", &item.name)]);
        if line.is_empty() {
            line = cut(&entry, CARD_LINE_CHARS);
        } else if line.chars().count() + 3 + entry.chars().count() <= CARD_LINE_CHARS {
            line = format!("{line} · {entry}");
        } else {
            break;
        }
    }
    line
}

/// Cuts at a character boundary and marks the cut with an ellipsis.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}
