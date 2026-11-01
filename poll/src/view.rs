//! The panel and the chat cards, built from the plugin state. Pure functions, so tests can
//! inspect the trees without a host.

use mumcord_plugin_sdk::{
    i18n::Strings,
    ui::{self, ButtonKind, El},
};

use crate::app::{Draft, DraftError, Poll};
use crate::wire::{MAX_OPTION_BYTES, MAX_OPTIONS, MIN_OPTIONS, truncate};

/// The question field is a host text input, which holds at most 64 bytes (FR-6.41), although the
/// wire format accepts questions of up to 200 bytes (FR-6.81).
pub const QUESTION_INPUT_BYTES: u8 = 64;
/// Host limit for any text string in a tree (FR-6.42); used for the voter lines.
const MAX_TEXT_CHARS: usize = 200;

/// The panel: the creation form, or the user's open poll with a Close button.
pub fn panel(s: &Strings, draft: &Draft, own_open: Option<&Poll>) -> El {
    if let Some(poll) = own_open {
        return ui::column()
            .child(ui::heading(&s.t("panel.your_poll")))
            .child(ui::text(&poll.question))
            .child(ui::caption(&votes_text(s, poll.total())))
            .child(ui::button("close", &s.t("button.close")).kind(ButtonKind::Danger));
    }
    let mut form = ui::column()
        .child(ui::heading(&s.t("panel.new_poll")))
        .child(ui::text_input(
            "question",
            &s.t("field.question"),
            &draft.question,
            QUESTION_INPUT_BYTES,
        ));
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
    form = form.child(ui::button("create", &s.t("button.create")));
    if let Some(error) = draft.error {
        let key = match error {
            DraftError::PollOpen => "error.poll_open",
            DraftError::Incomplete => "error.incomplete",
        };
        form = form.child(ui::caption(&s.t(key)).color("danger"));
    }
    form
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

/// A poll's chat card: one meter and Vote button per option, the status and the voter list.
pub fn card(s: &Strings, poll: &Poll, role: Role) -> El {
    let open = !poll.closed && !poll.frozen;
    let total = poll.total();
    let mut card = ui::column()
        .child(ui::heading(&poll.question))
        .child(ui::caption(&status_text(s, poll, role)));
    for (i, option) in poll.options.iter().enumerate() {
        let count = poll.counts.get(i).copied().unwrap_or(0);
        let share = if total == 0 {
            0.0
        } else {
            f32::from(count) / total as f32
        };
        let chosen = poll.my_vote == Some(i);
        let vote = ui::button(
            &format!("vote-{i}"),
            &s.t_with("button.vote", &[("option", option)]),
        )
        .kind(if chosen {
            ButtonKind::Primary
        } else {
            ButtonKind::Secondary
        })
        .disabled(!open);
        card = card.child(
            ui::row()
                .child(ui::meter(
                    &format!("{option}: {}", votes_text(s, u32::from(count))),
                    share,
                ))
                .child(vote),
        );
    }
    for line in voter_lines(poll) {
        card = card.child(ui::caption(&line));
    }
    if role == Role::Creator && !poll.closed {
        card = card.child(ui::button("close", &s.t("button.close")).kind(ButtonKind::Danger));
    }
    card
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

/// The fallback chat line posted at creation, for official Mumble clients. The host prefixes
/// "[Poll] " and allows 500 characters in total.
pub fn creation_line(question: &str, options: &[String]) -> String {
    let list: Vec<String> = options
        .iter()
        .enumerate()
        .map(|(i, o)| format!("{}) {o}", i + 1))
        .collect();
    chat_line(&format!("{question} {}", list.join(" ")))
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

/// Chat lines are limited to 500 characters including the host's "[Poll] " prefix.
fn chat_line(text: &str) -> String {
    truncate(text, 480).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::VoterEntry;

    #[test]
    fn creation_line_fits_the_chat_limit() {
        let options = vec!["ö".repeat(20); MAX_OPTIONS];
        let line = creation_line(&"ä".repeat(100), &options);
        assert!(line.chars().count() + "[Poll] ".len() <= 500);
        assert_eq!(
            creation_line("Pizza or pasta?", &["Pizza".into(), "Pasta".into()]),
            "Pizza or pasta? 1) Pizza 2) Pasta"
        );
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
