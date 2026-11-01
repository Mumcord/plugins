//! The panel and the chat cards, built from the plugin state. Pure functions, so tests can
//! inspect the trees without a host.

use mumcord_plugin_sdk::{
    i18n::Strings,
    ui::{self, Align, ButtonKind, Direction, El, Justify},
};

use crate::app::{Draft, DraftError, Poll};
use crate::wire::{MAX_OPTION_BYTES, MAX_OPTIONS, MIN_OPTIONS, truncate};

/// The question field is a host text input, which holds at most 64 bytes (FR-6.41), although the
/// wire format accepts questions of up to 200 bytes (FR-6.81).
pub const QUESTION_INPUT_BYTES: u8 = 64;
/// Host limit for any text string in a tree (FR-6.42); used for the voter lines.
const MAX_TEXT_CHARS: usize = 200;

/// Options with a bar and a Vote button in the small inline panel (260×160 px, no scrolling);
/// the rest is one click away in the popover.
pub const INLINE_OPTIONS: usize = 2;
/// About one line of the inline panel.
const INLINE_QUESTION_CHARS: usize = 36;
/// An option button inline keeps room for its bar on the same line.
const INLINE_OPTION_CHARS: usize = 12;

/// Options on the chat card (440×160 px, no scrolling), the ones with the most votes; "Open"
/// shows all of them in the popover.
pub const CARD_OPTIONS: usize = 3;
/// About one line of the card's heading.
const CARD_QUESTION_CHARS: usize = 44;
/// An option label on the card keeps room for its bar and count on the same line.
const CARD_OPTION_CHARS: usize = 24;

/// What the panel shows.
pub struct Panel<'a> {
    pub draft: &'a Draft,
    /// The creation form is open (only ever in the expanded popover).
    pub creating: bool,
    pub expanded: bool,
    /// The poll of the channel right now, and whether the user created it.
    pub current: Option<(&'a Poll, Role)>,
    /// The user's own poll is open, so a new one has to wait.
    pub own_open: bool,
    /// `chat.triggers` is granted: people without Mumcord can vote with "!poll <n>".
    pub triggers: bool,
}

/// The panel: the creation form, the current poll or an empty state.
pub fn panel(s: &Strings, p: &Panel) -> El {
    if p.creating {
        return form(s, p.draft, p.triggers);
    }
    let Some((poll, role)) = p.current else {
        return ui::column()
            .child(ui::heading(&s.t("panel.empty")))
            .child(ui::caption(&s.t("panel.empty_hint")))
            .child(ui::row().child(new_poll_button(s, ButtonKind::Primary)));
    };
    let shown = if p.expanded {
        poll.options.len()
    } else {
        INLINE_OPTIONS
    };
    // Inline, everything has to fit 160 px without scrolling: tighter gaps and a one-line
    // question; the popover has room for the full text.
    let mut column = if p.expanded {
        ui::column()
            .child(ui::caption(&status_text(s, poll, role)))
            .child(ui::heading(&poll.question))
    } else {
        ui::stack(Direction::Column, 4, Align::Stretch)
            .child(ui::caption(&status_text(s, poll, role)))
            .child(ui::text(&cut_chars(&poll.question, INLINE_QUESTION_CHARS)))
    };
    let label_chars = if p.expanded {
        MAX_TEXT_CHARS
    } else {
        INLINE_OPTION_CHARS
    };
    for i in 0..poll.options.len().min(shown) {
        column = column.child(option_row(s, poll, i, label_chars));
    }
    if p.expanded {
        for line in voter_lines(poll) {
            column = column.child(ui::caption(&line));
        }
        // Without `chat.triggers` people in plain Mumble cannot vote, although the chat line
        // says so. The button opens the plugin's permissions, where the user can turn it on.
        if role == Role::Creator && !poll.closed && !p.triggers {
            column = column.child(
                ui::button("open-permissions", &s.t("card.hint_triggers"))
                    .kind(ButtonKind::Secondary),
            );
        }
    }
    let mut buttons = ui::row();
    if poll.options.len() > shown {
        buttons = buttons
            .child(ui::button("show-all", &s.t("button.show_all")).kind(ButtonKind::Secondary));
    }
    // While the user's own poll is open, a new one has to wait: End takes the place of New.
    if p.own_open {
        if role == Role::Creator {
            buttons =
                buttons.child(ui::button("close", &s.t("button.end")).kind(ButtonKind::Danger));
        }
    } else {
        // Secondary, so it does not look like a chosen option.
        buttons = buttons.child(new_poll_button(s, ButtonKind::Secondary));
    }
    column.child(buttons)
}

fn new_poll_button(s: &Strings, kind: ButtonKind) -> El {
    ui::button("new-poll", &s.t("button.new_poll")).kind(kind)
}

/// The creation form, laid out like the mockup in #284.
fn form(s: &Strings, draft: &Draft, triggers: bool) -> El {
    let mut form = ui::column()
        .child(ui::heading(&s.t("panel.new_poll")))
        .child(ui::text_input(
            "question",
            &s.t("field.question"),
            &draft.question,
            QUESTION_INPUT_BYTES,
        ))
        .child(ui::caption(&s.t_with(
            "field.options",
            &[
                ("count", &draft.options.len().to_string()),
                ("max", &MAX_OPTIONS.to_string()),
            ],
        )));
    for (i, option) in draft.options.iter().enumerate() {
        form = form.child(ui::text_input(
            &format!("option-{i}"),
            &s.t_with("field.option", &[("number", &(i + 1).to_string())]),
            option,
            option_input_bytes(),
        ));
    }
    form = form.child(
        ui::row()
            .child(
                ui::button("add-option", &s.t("button.add_option"))
                    .kind(ButtonKind::Secondary)
                    .disabled(draft.options.len() >= MAX_OPTIONS),
            )
            .child(
                ui::button("remove-option", &s.t("button.remove_option"))
                    .kind(ButtonKind::Secondary)
                    .disabled(draft.options.len() <= MIN_OPTIONS),
            ),
    );
    if let Some(error) = draft.error {
        let key = match error {
            DraftError::PollOpen => "error.poll_open",
            DraftError::Incomplete => "error.incomplete",
        };
        form = form.child(ui::caption(&s.t(key)).color("danger"));
    }
    if triggers {
        form = form.child(ui::caption(&s.t("form.hint_triggers")));
    }
    form.child(
        ui::row()
            .child(ui::button("cancel", &s.t("button.cancel")).kind(ButtonKind::Secondary))
            .child(ui::button("create", &s.t("button.create")).kind(ButtonKind::Primary)),
    )
}

fn option_input_bytes() -> u8 {
    u8::try_from(MAX_OPTION_BYTES).unwrap_or(u8::MAX)
}

/// Who is looking at a card decides which controls it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Creator,
    Voter,
}

/// A poll's chat card, a summary that always fits its slot: the question, the status with
/// "Open" (the whole poll in the popover, where people vote) and the options with the most
/// votes as slim result rows.
pub fn card(s: &Strings, poll: &Poll, role: Role) -> El {
    // The host fades anything below 144 px of content (the 160 px slot minus padding). "Open"
    // sits beside the question and the status line instead of on a line of its own, and the
    // lines go without a gap (their line height spaces them already): 40 px + 3 × 32 px rows
    // leave room for taller emoji glyphs, so the third option stays fully visible.
    let summary = ui::stack(Direction::Column, 0, Align::Stretch)
        .child(ui::heading(&cut_chars(&poll.question, CARD_QUESTION_CHARS)))
        .child(ui::caption(&status_text(s, poll, role)));
    let mut card = ui::stack(Direction::Column, 0, Align::Stretch).child(
        ui::row()
            .justify(Justify::FillFirst)
            .child(summary)
            .child(ui::button("open", &s.t("button.open")).kind(ButtonKind::Primary)),
    );
    for i in top_options(poll) {
        card = card.child(result_row(s, poll, i));
    }
    card
}

/// The indexes of the options with the most votes, ties in the poll's order.
fn top_options(poll: &Poll) -> Vec<usize> {
    let mut order: Vec<usize> = (0..poll.options.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(poll.counts.get(i).copied().unwrap_or(0)));
    order.truncate(CARD_OPTIONS);
    order
}

/// One option on the card: its name (with a checkmark once chosen) and its bar with the count
/// as text, so the result is never shown by colour alone.
fn result_row(s: &Strings, poll: &Poll, i: usize) -> El {
    let count = poll.counts.get(i).copied().unwrap_or(0);
    let label = cut_chars(&poll.options[i], CARD_OPTION_CHARS);
    let label = if poll.my_vote == Some(i) {
        format!("✓ {label}")
    } else {
        label
    };
    ui::row().child(ui::text(&label)).child(ui::meter(
        &votes_text(s, u32::from(count)),
        share(poll, count),
    ))
}

fn share(poll: &Poll, count: u16) -> f32 {
    let total = poll.total();
    if total == 0 {
        0.0
    } else {
        f32::from(count) / total as f32
    }
}

/// One option: a button with its name that votes for it (highlighted once chosen), and its bar
/// with the count as text, so the result is never shown by colour alone.
fn option_row(s: &Strings, poll: &Poll, i: usize, label_chars: usize) -> El {
    let option = &poll.options[i];
    let count = poll.counts.get(i).copied().unwrap_or(0);
    let kind = if poll.my_vote == Some(i) {
        ButtonKind::Primary
    } else {
        ButtonKind::Secondary
    };
    let vote = ui::button(&format!("vote-{i}"), &cut_chars(option, label_chars))
        .kind(kind)
        .disabled(poll.closed || poll.frozen);
    ui::row().child(vote).child(ui::meter(
        &votes_text(s, u32::from(count)),
        share(poll, count),
    ))
}

fn status_text(s: &Strings, poll: &Poll, role: Role) -> String {
    let votes = votes_text(s, poll.total());
    let key = if poll.frozen {
        "status.frozen"
    } else if poll.closed {
        "status.closed"
    } else if role == Role::Creator {
        "status.own"
    } else {
        "status.by"
    };
    s.t_with(key, &[("name", &poll.creator), ("votes", &votes)])
}

fn votes_text(s: &Strings, n: u32) -> String {
    s.tn("votes", u64::from(n), &[])
}

/// "Pizza: Anna, Ben" per option that has named voters, each cut to the host's text limit.
fn voter_lines(poll: &Poll) -> Vec<String> {
    poll.options
        .iter()
        .enumerate()
        .filter_map(|(i, option)| {
            let names: Vec<&str> = poll
                .voters
                .iter()
                .filter(|v| usize::from(v.option) == i)
                .map(|v| v.name.as_str())
                .collect();
            if names.is_empty() {
                return None;
            }
            let line = format!("{option}: {}", names.join(", "));
            Some(cut_chars(&line, MAX_TEXT_CHARS))
        })
        .collect()
}

fn cut_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(max - 1).collect();
    cut.push('…');
    cut
}

/// The fallback chat line posted at creation, for official Mumble clients and Mumcord users
/// without Poll. Plain text only (the host escapes plugin lines), so the parts are separated by
/// " · "; it always ends with how to vote with "!poll <n>".
pub fn creation_line(s: &Strings, question: &str, options: &[String]) -> String {
    let list: Vec<String> = options
        .iter()
        .enumerate()
        .map(|(i, o)| format!("{}) {o}", i + 1))
        .collect();
    let how = s.t_with("chat.vote_with", &[("count", &options.len().to_string())]);
    // The hint must survive the length cut, so the question and the options give way.
    let separator = " · ";
    let icon = "📊 ";
    let room = CHAT_LINE_BYTES.saturating_sub(how.len() + separator.len() + icon.len());
    let poll = format!("{question}{separator}{}", list.join(separator));
    chat_line(&format!("{icon}{}{separator}{how}", truncate(&poll, room)))
}

/// The fallback chat line posted when the creator closes the poll.
pub fn close_line(s: &Strings, poll: &Poll) -> String {
    let results: Vec<String> = poll
        .options
        .iter()
        .zip(&poll.counts)
        .map(|(o, n)| format!("{o}: {n}"))
        .collect();
    chat_line(&s.t_with(
        "chat.closed",
        &[
            ("question", &poll.question),
            ("results", &results.join(", ")),
        ],
    ))
}

/// Chat lines are limited to 500 characters including the host's "[Poll] " prefix; counting
/// bytes keeps well within that.
const CHAT_LINE_BYTES: usize = 480;

fn chat_line(text: &str) -> String {
    truncate(text, CHAT_LINE_BYTES).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::VoterEntry;

    fn english() -> Strings {
        Strings::new(mumcord_plugin_sdk::i18n_all!())
    }

    #[test]
    fn creation_line_tells_how_to_vote() {
        let s = english();
        let options = vec!["Pizza".to_owned(), "Pasta".to_owned()];
        assert_eq!(
            creation_line(&s, "Pizza or pasta?", &options),
            "📊 Pizza or pasta? · 1) Pizza · 2) Pasta · Vote with !poll 1 to !poll 2"
        );
    }

    #[test]
    fn creation_line_fits_the_chat_limit_and_keeps_the_hint() {
        let s = english();
        let options = vec!["ö".repeat(20); MAX_OPTIONS];
        let line = creation_line(&s, &"ä".repeat(100), &options);
        assert!(line.chars().count() + "[Poll] ".len() <= 500);
        assert!(line.ends_with("Vote with !poll 1 to !poll 10"), "{line}");
    }

    #[test]
    fn card_puts_open_beside_the_question_so_three_options_fit() {
        // The host fades everything below 144 px of content; a line of its own for "Open" and
        // gaps between the lines pushed the third option there.
        let options = ["A", "B", "C", "D"].map(String::from).to_vec();
        let poll = Poll::new(1, "Q?".into(), options, "Anna".into());
        let card = card(&english(), &poll, Role::Voter);
        assert_eq!(card.children.len(), 1 + CARD_OPTIONS);
        let ui::Component::Stack(lines) = &card.component else {
            panic!("the card is a stack");
        };
        assert_eq!(lines.gap, 0);
        let head = &card.children[0];
        let ui::Component::Stack(row) = &head.component else {
            panic!("the heading row is a stack");
        };
        assert_eq!(row.direction, Direction::Row);
        assert_eq!(row.justify, Some(ui::Justify::FillFirst));
        assert_eq!(head.children.len(), 2, "question and status, then Open");
        assert!(matches!(
            head.children[1].component,
            ui::Component::Button(_)
        ));
    }

    #[test]
    fn voter_lines_group_by_option_and_respect_the_text_limit() {
        let mut poll = Poll::new(1, "Q?".into(), vec!["A".into(), "B".into()], "Anna".into());
        poll.voters = (0..100)
            .map(|i| VoterEntry {
                option: 0,
                name: format!("Voter number {i}"),
            })
            .collect();
        poll.voters.push(VoterEntry {
            option: 1,
            name: "Ben".into(),
        });
        let lines = voter_lines(&poll);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].chars().count(), MAX_TEXT_CHARS);
        assert_eq!(lines[1], "B: Ben");
    }
}
