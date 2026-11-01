//! Native tests of the Poll logic against a recording fake host. Scenario tests through
//! `mumcord-plugin test` (two simulated clients) follow once that command exists.

use std::collections::HashMap;

use mumcord_plugin_sdk::{
    events::Event,
    types::{Error, Peer},
    ui::{Component, El, Slot},
};

use crate::app::{App, Host, RESEND_DELAY_MS, TALLY_DELAY_MS, To, other_card_id, own_card_id};
use crate::wire::{
    Create, KIND_CREATE, KIND_TALLY, KIND_VOTE, KIND_VOTERS, Tally, Vote, VoterEntry, Voters,
};

#[derive(Default)]
struct Fake {
    now: u64,
    sent: Vec<(To, u8, Vec<u8>)>,
    chat: Vec<String>,
    superseded: Vec<(String, u32, u32)>,
    notified: Vec<(String, String)>,
    timers: Vec<(u32, u32)>,
    next_timer: u32,
    shown: HashMap<String, El>,
    warnings: Vec<String>,
    name: Option<String>,
    /// Answers for the next `send` calls; `Ok` once they run out.
    send_results: Vec<Result<(), Error>>,
    chat_result: Option<Error>,
    session: Option<u32>,
    /// Whether the popover is open; `None` until the plugin opens or closes it.
    popover: Option<bool>,
    /// The host refuses to open the popover (no fresh gesture).
    popover_refused: bool,
    /// The current event is a replay from chat history.
    replay: bool,
    /// Cards put in place of the current message, in order.
    replaced: Vec<String>,
    /// How many current messages were hidden.
    hidden: usize,
}

impl Host for Fake {
    fn now(&mut self) -> u64 {
        self.now
    }

    fn send(&mut self, to: To, kind: u8, payload: &[u8]) -> Result<(), Error> {
        let result = if self.send_results.is_empty() {
            Ok(())
        } else {
            self.send_results.remove(0)
        };
        if result.is_ok() {
            self.sent.push((to, kind, payload.to_vec()));
        }
        result
    }

    fn chat_send(&mut self, text: &str) -> Result<(), Error> {
        if let Some(e) = self.chat_result {
            return Err(e);
        }
        self.chat.push(text.to_owned());
        Ok(())
    }

    fn line_hash(&mut self, text: &str) -> u32 {
        u32::try_from(text.len()).unwrap_or(0) + 1000
    }

    fn supersede(&mut self, card: &str, sender: u32, hash: u32) {
        self.superseded.push((card.to_owned(), sender, hash));
    }

    fn is_replay(&mut self) -> bool {
        self.replay
    }

    fn replace_message(&mut self, card: &str) {
        self.replaced.push(card.to_owned());
    }

    fn hide_message(&mut self) {
        self.hidden += 1;
    }

    fn notify(&mut self, title: &str, body: &str) {
        self.notified.push((title.to_owned(), body.to_owned()));
    }

    fn start_timer(&mut self, ms: u32) -> Option<u32> {
        self.next_timer += 1;
        self.timers.push((self.next_timer, ms));
        Some(self.next_timer)
    }

    fn show(&mut self, slot: Slot, tree: El) {
        let key = match slot {
            Slot::Panel => "panel".to_owned(),
            Slot::ChatCard(id) => id,
            other => panic!("unexpected slot {other:?}"),
        };
        self.shown.insert(key, tree);
    }

    fn my_name(&mut self) -> Option<String> {
        self.name.clone()
    }

    fn my_session(&mut self) -> Option<u32> {
        self.session
    }

    fn open_popover(&mut self) -> Result<(), Error> {
        if self.popover_refused {
            return Err(Error::NotPermitted);
        }
        self.popover = Some(true);
        Ok(())
    }

    fn close_popover(&mut self) {
        self.popover = Some(false);
    }

    fn warn(&mut self, message: &str) {
        self.warnings.push(message.to_owned());
    }
}

impl Fake {
    fn sent_of(&self, kind: u8) -> Vec<(To, Vec<u8>)> {
        self.sent
            .iter()
            .filter(|(_, k, _)| *k == kind)
            .map(|(to, _, p)| (*to, p.clone()))
            .collect()
    }

    fn texts(&self, key: &str) -> Vec<String> {
        let mut out = Vec::new();
        collect_texts(self.shown.get(key).expect("slot shown"), &mut out);
        out
    }

    fn timer_with(&self, ms: u32) -> u32 {
        self.timers
            .iter()
            .rev()
            .find(|(_, m)| *m == ms)
            .map(|(id, _)| *id)
            .expect("timer started")
    }
}

fn collect_texts(el: &El, out: &mut Vec<String>) {
    // An option row (its button, then its bar) also reads as "Option: n votes".
    if let [button, meter] = el.children.as_slice()
        && let (Component::Button(b), Component::Meter(m)) = (&button.component, &meter.component)
    {
        out.push(format!("{}: {}", b.label, m.label));
    }
    match &el.component {
        Component::Text(t) => out.push(t.text.clone()),
        Component::Button(b) => out.push(format!(
            "[{}]{}",
            b.action,
            if b.disabled { " off" } else { "" }
        )),
        Component::Meter(m) => out.push(m.label.clone()),
        Component::TextInput(t) => out.push(format!("<{}>", t.action)),
        _ => {}
    }
    for child in &el.children {
        collect_texts(child, out);
    }
}

fn peer(session: u32, name: &str, key: Option<&str>) -> Peer {
    Peer {
        session,
        name: name.to_owned(),
        key: key.map(str::to_owned),
    }
}

fn text(t: &str) -> Option<String> {
    Some(t.to_owned())
}

/// A creator with an open poll "Pizza or pasta?" (Pizza, Pasta), created at t = 1000.
fn creator() -> (App, Fake) {
    let mut app = App::default();
    let mut host = Fake {
        now: 1000,
        name: Some("Anna".into()),
        ..Fake::default()
    };
    app.start(&mut host);
    app.on_action(&mut host, None, "new-poll", None);
    app.on_action(&mut host, None, "question", text("Pizza or pasta?"));
    app.on_action(&mut host, None, "option-0", text("Pizza"));
    app.on_action(&mut host, None, "option-1", text(" Pasta "));
    app.on_action(&mut host, None, "create", None);
    (app, host)
}

fn create_payload(id: u32, hash: u32) -> Vec<u8> {
    Create {
        poll_id: id,
        line_hash: hash,
        question: "Pizza or pasta?".into(),
        options: vec!["Pizza".into(), "Pasta".into()],
    }
    .encode()
}

/// A voter who received poll 1000 from Anna (session 1).
fn voter() -> (App, Fake) {
    let mut app = App::default();
    let mut host = Fake::default();
    app.set_granted(&mut host, vec!["notify".into()]);
    app.on_peer_message(
        &mut host,
        &peer(1, "Anna", Some("ka")),
        KIND_CREATE,
        &create_payload(1000, 77),
        false,
    );
    (app, host)
}

#[test]
fn panel_starts_empty_and_new_poll_opens_the_form_in_the_popover() {
    let mut app = App::default();
    let mut host = Fake::default();
    app.start(&mut host);
    let texts = host.texts("panel");
    assert!(texts.contains(&"No poll in this channel".to_owned()));
    assert!(texts.contains(&"[new-poll]".to_owned()));
    assert!(!texts.contains(&"<question>".to_owned()));

    app.on_action(&mut host, None, "new-poll", None);
    assert_eq!(host.popover, Some(true));
    let texts = host.texts("panel");
    assert!(texts.contains(&"<question>".to_owned()));
    assert!(texts.contains(&"<option-1>".to_owned()));
    assert!(!texts.contains(&"<option-2>".to_owned()));
    assert!(
        texts.contains(&"[remove-option] off".to_owned()),
        "two options is the minimum"
    );
}

#[test]
fn options_can_be_added_up_to_ten() {
    let mut app = App::default();
    let mut host = Fake::default();
    app.on_action(&mut host, None, "new-poll", None);
    for _ in 0..12 {
        app.on_action(&mut host, None, "add-option", None);
    }
    let texts = host.texts("panel");
    assert!(texts.contains(&"<option-9>".to_owned()));
    assert!(!texts.contains(&"<option-10>".to_owned()));
    assert!(texts.contains(&"[add-option] off".to_owned()));
}

#[test]
fn create_posts_the_line_sends_the_poll_and_shows_a_card() {
    let (_app, host) = creator();
    assert_eq!(
        host.chat,
        vec!["📊 Pizza or pasta? · 1) Pizza · 2) Pasta · Vote with !poll 1 to !poll 2".to_owned()]
    );
    // The popover closes and the panel shows the new poll with its End button.
    assert_eq!(host.popover, Some(false));
    let creates = host.sent_of(KIND_CREATE);
    assert_eq!(creates.len(), 1);
    assert_eq!(creates[0].0, To::Channel);
    let create = Create::decode(&creates[0].1).expect("valid create");
    assert_eq!(create.poll_id, 1000);
    assert_eq!(create.options, vec!["Pizza".to_owned(), "Pasta".to_owned()]);
    assert_eq!(create.line_hash, host.chat[0].len() as u32 + 1000);
    let card = host.texts(&own_card_id(1000));
    assert!(card.contains(&"Pizza or pasta?".to_owned()));
    assert!(host.texts("panel").contains(&"[close]".to_owned()));
}

#[test]
fn invalid_form_is_refused_without_side_effects() {
    let mut app = App::default();
    let mut host = Fake::default();
    app.on_action(&mut host, None, "new-poll", None);
    app.on_action(&mut host, None, "question", text("Only a question"));
    app.on_action(&mut host, None, "option-0", text("One"));
    app.on_action(&mut host, None, "create", None);
    assert!(host.sent.is_empty() && host.chat.is_empty());
    assert!(
        host.texts("panel")
            .iter()
            .any(|t| t.contains("at least two options"))
    );
}

#[test]
fn a_second_poll_needs_the_first_closed() {
    let (mut app, mut host) = creator();
    app.on_action(&mut host, None, "question", text("Again?"));
    app.on_action(&mut host, None, "create", None);
    assert_eq!(host.sent_of(KIND_CREATE).len(), 1);
}

#[test]
fn chat_failure_still_creates_the_poll_without_a_hash() {
    let mut app = App::default();
    let mut host = Fake {
        chat_result: Some(Error::NotPermitted),
        ..Fake::default()
    };
    app.on_action(&mut host, None, "question", text("Q?"));
    app.on_action(&mut host, None, "option-0", text("A"));
    app.on_action(&mut host, None, "option-1", text("B"));
    app.on_action(&mut host, None, "create", None);
    let create = Create::decode(&host.sent_of(KIND_CREATE)[0].1).expect("create");
    assert_eq!(create.line_hash, 0);
    assert_eq!(host.warnings.len(), 1);
}

#[test]
fn votes_count_per_key_or_session_and_can_change() {
    let (mut app, mut host) = creator();
    let ben = peer(2, "Ben", Some("kb"));
    let ben_again = peer(9, "Ben", Some("kb"));
    let guest = peer(3, "Guest", None);
    let vote = |option| {
        Vote {
            poll_id: 1000,
            option,
        }
        .encode()
    };
    app.on_peer_message(&mut host, &ben, KIND_VOTE, &vote(0), true);
    // Same key from a new session (reconnect) replaces the vote instead of adding one.
    app.on_peer_message(&mut host, &ben_again, KIND_VOTE, &vote(1), true);
    app.on_peer_message(&mut host, &guest, KIND_VOTE, &vote(1), true);
    // A user without a key counts per session.
    app.on_peer_message(
        &mut host,
        &peer(4, "Guest", None),
        KIND_VOTE,
        &vote(0),
        true,
    );
    let card = host.texts(&own_card_id(1000));
    assert!(card.contains(&"Pizza: 1 vote".to_owned()), "{card:?}");
    assert!(card.contains(&"Pasta: 2 votes".to_owned()), "{card:?}");
}

#[test]
fn bad_votes_are_ignored() {
    let (mut app, mut host) = creator();
    let ben = peer(2, "Ben", Some("kb"));
    // Not direct, wrong poll, option out of range, garbage.
    app.on_peer_message(
        &mut host,
        &ben,
        KIND_VOTE,
        &Vote {
            poll_id: 1000,
            option: 0,
        }
        .encode(),
        false,
    );
    app.on_peer_message(
        &mut host,
        &ben,
        KIND_VOTE,
        &Vote {
            poll_id: 999,
            option: 0,
        }
        .encode(),
        true,
    );
    app.on_peer_message(
        &mut host,
        &ben,
        KIND_VOTE,
        &Vote {
            poll_id: 1000,
            option: 2,
        }
        .encode(),
        true,
    );
    app.on_peer_message(&mut host, &ben, KIND_VOTE, &[1, 2, 3], true);
    assert!(
        host.texts(&own_card_id(1000))
            .contains(&"Pizza: 0 votes".to_owned())
    );
    assert!(host.timers.is_empty(), "no tally scheduled");
}

#[test]
fn tally_is_coalesced_to_one_per_two_seconds() {
    let (mut app, mut host) = creator();
    for session in 2..6 {
        let p = peer(session, "V", None);
        app.on_peer_message(
            &mut host,
            &p,
            KIND_VOTE,
            &Vote {
                poll_id: 1000,
                option: 0,
            }
            .encode(),
            true,
        );
    }
    assert_eq!(host.timers.len(), 1, "one timer for all four votes");
    assert!(
        host.sent_of(KIND_TALLY).is_empty(),
        "nothing before the timer fires"
    );
    let timer = host.timer_with(TALLY_DELAY_MS);
    app.on_timer(&mut host, timer);
    let tallies = host.sent_of(KIND_TALLY);
    assert_eq!(tallies.len(), 1);
    let tally = Tally::decode(&tallies[0].1).expect("tally");
    assert_eq!(tally.counts, vec![4, 0]);
    assert!(!tally.closed);
    // The timer firing again without new votes sends nothing.
    app.on_timer(&mut host, timer);
    assert_eq!(host.sent_of(KIND_TALLY).len(), 1);
}

#[test]
fn creator_votes_on_own_card_without_a_message() {
    let (mut app, mut host) = creator();
    app.on_action(&mut host, Some(&own_card_id(1000)), "vote-1", None);
    assert!(host.sent_of(KIND_VOTE).is_empty());
    let card = host.texts(&own_card_id(1000));
    assert!(card.contains(&"✓ Pasta: 1 vote".to_owned()));
    assert!(card.contains(&"Pasta: Anna".to_owned()));
}

#[test]
fn close_posts_the_result_and_sends_final_tally_and_voters() {
    let (mut app, mut host) = creator();
    app.on_peer_message(
        &mut host,
        &peer(2, "Ben", Some("kb")),
        KIND_VOTE,
        &Vote {
            poll_id: 1000,
            option: 0,
        }
        .encode(),
        true,
    );
    app.on_action(&mut host, Some(&own_card_id(1000)), "close", None);
    assert_eq!(host.chat[1], "Closed: Pizza or pasta? Pizza: 1, Pasta: 0");
    let tally = Tally::decode(&host.sent_of(KIND_TALLY)[0].1).expect("tally");
    assert!(tally.closed);
    assert_eq!(tally.close_hash, host.chat[1].len() as u32 + 1000);
    let voters = Voters::decode(&host.sent_of(KIND_VOTERS)[0].1).expect("voters");
    assert_eq!(
        voters.entries,
        vec![VoterEntry {
            option: 0,
            name: "Ben".into()
        }]
    );
    // Closed: no End button, votes refused, the panel is empty with "New poll" again.
    assert!(
        !host
            .texts(&own_card_id(1000))
            .contains(&"[close]".to_owned())
    );
    assert!(host.texts("panel").contains(&"[new-poll]".to_owned()));
    app.on_peer_message(
        &mut host,
        &peer(3, "Cy", None),
        KIND_VOTE,
        &Vote {
            poll_id: 1000,
            option: 1,
        }
        .encode(),
        true,
    );
    assert!(
        host.texts(&own_card_id(1000))
            .contains(&"Pasta: 0 votes".to_owned())
    );
    // A pending tally timer no longer sends anything.
    let timer = host.timer_with(TALLY_DELAY_MS);
    app.on_timer(&mut host, timer);
    assert_eq!(host.sent_of(KIND_TALLY).len(), 1);
}

#[test]
fn voter_sees_card_supersedes_the_line_and_is_notified() {
    let (_app, host) = voter();
    let card = other_card_id(1, 1000);
    assert!(
        host.texts(&card)
            .iter()
            .any(|t| t.starts_with("Poll by Anna"))
    );
    assert_eq!(host.superseded, vec![(card, 1, 77)]);
    assert_eq!(
        host.notified,
        vec![(
            "New poll".to_owned(),
            "Anna asks: Pizza or pasta?".to_owned()
        )]
    );
}

#[test]
fn no_notification_without_permission_or_for_resends() {
    let mut app = App::default();
    let mut host = Fake::default();
    app.on_peer_message(
        &mut host,
        &peer(1, "Anna", None),
        KIND_CREATE,
        &create_payload(1, 0),
        false,
    );
    app.set_granted(&mut host, vec!["notify".into()]);
    app.on_peer_message(
        &mut host,
        &peer(2, "Ben", None),
        KIND_CREATE,
        &create_payload(2, 0),
        true,
    );
    assert!(host.notified.is_empty());
    assert!(host.superseded.is_empty(), "hash 0 means no line to hide");
}

#[test]
fn voting_sends_directly_to_the_creator() {
    let (mut app, mut host) = voter();
    app.on_action(&mut host, Some(&other_card_id(1, 1000)), "vote-1", None);
    let votes = host.sent_of(KIND_VOTE);
    assert_eq!(votes.len(), 1);
    assert_eq!(votes[0].0, To::Direct(1));
    assert_eq!(
        Vote::decode(&votes[0].1),
        Some(Vote {
            poll_id: 1000,
            option: 1
        })
    );
}

#[test]
fn tally_and_voters_only_count_from_the_creator() {
    let (mut app, mut host) = voter();
    let card = other_card_id(1, 1000);
    let tally = |closed, counts: Vec<u16>| {
        Tally {
            poll_id: 1000,
            closed,
            close_hash: 55,
            counts,
        }
        .encode()
    };
    app.on_peer_message(
        &mut host,
        &peer(2, "Mallory", None),
        KIND_TALLY,
        &tally(false, vec![9, 9]),
        false,
    );
    assert!(host.texts(&card).contains(&"Pizza: 0 votes".to_owned()));
    app.on_peer_message(
        &mut host,
        &peer(1, "Anna", None),
        KIND_TALLY,
        &tally(false, vec![2, 1]),
        false,
    );
    assert!(host.texts(&card).contains(&"Pizza: 2 votes".to_owned()));
    // Wrong option count is ignored.
    app.on_peer_message(
        &mut host,
        &peer(1, "Anna", None),
        KIND_TALLY,
        &tally(false, vec![1, 1, 1]),
        false,
    );
    assert!(host.texts(&card).contains(&"Pizza: 2 votes".to_owned()));
    let voters = Voters::split(
        1000,
        &[VoterEntry {
            option: 1,
            name: "Ben".into(),
        }],
    );
    app.on_peer_message(
        &mut host,
        &peer(2, "Mallory", None),
        KIND_VOTERS,
        &voters[0].encode(),
        false,
    );
    assert!(!host.texts(&card).contains(&"Pasta: Ben".to_owned()));
    app.on_peer_message(
        &mut host,
        &peer(1, "Anna", None),
        KIND_VOTERS,
        &voters[0].encode(),
        false,
    );
    assert!(host.texts(&card).contains(&"Pasta: Ben".to_owned()));
    app.on_peer_message(
        &mut host,
        &peer(1, "Anna", None),
        KIND_TALLY,
        &tally(true, vec![2, 2]),
        false,
    );
    let texts = host.texts(&card);
    assert!(texts.iter().any(|t| t.starts_with("Closed")));
    assert!(texts.contains(&"[vote-0] off".to_owned()));
    assert_eq!(host.superseded.last(), Some(&(card, 1, 55)));
}

#[test]
fn creator_leaving_freezes_the_poll() {
    let (mut app, mut host) = voter();
    app.on_left(&mut host, &peer(5, "Someone else", None));
    let card = other_card_id(1, 1000);
    assert!(
        !host
            .texts(&card)
            .iter()
            .any(|t| t.contains("Creator left, poll is frozen"))
    );
    app.on_left(&mut host, &peer(1, "Anna", None));
    let texts = host.texts(&card);
    assert!(
        texts
            .iter()
            .any(|t| t.starts_with("Creator left, poll is frozen")),
        "{texts:?}"
    );
    assert!(texts.contains(&"[vote-0] off".to_owned()));
    app.on_action(&mut host, Some(&card), "vote-0", None);
    assert!(host.sent_of(KIND_VOTE).is_empty());
}

#[test]
fn a_new_poll_from_the_same_creator_ends_the_old_one() {
    let (mut app, mut host) = voter();
    app.on_peer_message(
        &mut host,
        &peer(1, "Anna", None),
        KIND_CREATE,
        &create_payload(1001, 0),
        false,
    );
    assert!(
        host.texts(&other_card_id(1, 1000))
            .iter()
            .any(|t| t.starts_with("Closed"))
    );
    assert!(
        host.texts(&other_card_id(1, 1001))
            .iter()
            .any(|t| t.starts_with("Poll by"))
    );
    // A resend of the poll we already have changes nothing.
    let before = host.shown.len();
    app.on_peer_message(
        &mut host,
        &peer(1, "Anna", None),
        KIND_CREATE,
        &create_payload(1001, 0),
        true,
    );
    assert_eq!(host.shown.len(), before);
}

#[test]
fn late_joiner_gets_the_poll_with_one_retry() {
    let (mut app, mut host) = creator();
    app.on_peer_message(
        &mut host,
        &peer(2, "Ben", Some("kb")),
        KIND_VOTE,
        &Vote {
            poll_id: 1000,
            option: 0,
        }
        .encode(),
        true,
    );
    host.sent.clear();
    // The joiner has not announced Poll yet: nobody receives it, so one retry is scheduled.
    host.send_results = vec![Err(Error::NoRecipients)];
    app.on_joined(&mut host, &peer(7, "Cy", None));
    assert!(host.sent.is_empty());
    let timer = host.timer_with(RESEND_DELAY_MS);
    app.on_timer(&mut host, timer);
    let to_cy: Vec<u8> = host
        .sent
        .iter()
        .filter(|(to, ..)| *to == To::Direct(7))
        .map(|(_, k, _)| *k)
        .collect();
    assert_eq!(to_cy, vec![KIND_CREATE, KIND_TALLY, KIND_VOTERS]);
    let create = Create::decode(&host.sent[0].2).expect("create");
    assert_eq!(create.line_hash, 0);
    let tally = Tally::decode(&host.sent[1].2).expect("tally");
    assert_eq!(
        tally.counts,
        vec![1, 0],
        "the resend carries the current tally"
    );
    // When the retry fails as well, it gives up instead of retrying forever.
    host.sent.clear();
    host.send_results = vec![Err(Error::NoRecipients)];
    let timers = host.timers.len();
    app.on_joined(&mut host, &peer(8, "Dee", None));
    host.send_results = vec![Err(Error::NoRecipients)];
    let retry = host.timer_with(RESEND_DELAY_MS);
    app.on_timer(&mut host, retry);
    assert_eq!(host.timers.len(), timers + 1);
    assert!(host.sent.is_empty());
}

#[test]
fn no_resend_without_an_open_poll() {
    let mut app = App::default();
    let mut host = Fake::default();
    app.on_joined(&mut host, &peer(7, "Cy", None));
    assert!(host.sent.is_empty());
}

#[test]
fn rate_limited_messages_wait_for_the_retry_timer() {
    let (mut app, mut host) = voter();
    host.send_results = vec![Err(Error::RateLimited)];
    app.on_action(&mut host, Some(&other_card_id(1, 1000)), "vote-0", None);
    assert!(host.sent.is_empty());
    let timer = host.timer_with(crate::app::RETRY_DELAY_MS);
    app.on_timer(&mut host, timer);
    assert_eq!(host.sent_of(KIND_VOTE).len(), 1);
}

#[test]
fn nobody_else_running_poll_is_not_an_error() {
    let mut app = App::default();
    let mut host = Fake {
        send_results: vec![Err(Error::NoRecipients)],
        ..Fake::default()
    };
    app.on_action(&mut host, None, "question", text("Q?"));
    app.on_action(&mut host, None, "option-0", text("A"));
    app.on_action(&mut host, None, "option-1", text("B"));
    app.on_action(&mut host, None, "create", None);
    assert!(host.warnings.is_empty());
    assert!(host.shown.contains_key(&own_card_id(1)));
}

#[test]
fn trees_stay_within_host_limits() {
    let (mut app, mut host) = creator();
    for session in 0..150 {
        let p = peer(session + 10, &"N".repeat(128), None);
        app.on_peer_message(
            &mut host,
            &p,
            KIND_VOTE,
            &Vote {
                poll_id: 1000,
                option: 0,
            }
            .encode(),
            true,
        );
    }
    for (key, tree) in &host.shown {
        let nodes = tree.clone().into_nodes();
        assert!(nodes.len() <= 500, "{key}: {} nodes", nodes.len());
        let mut texts = Vec::new();
        collect_texts(tree, &mut texts);
        assert!(texts.iter().all(|t| t.chars().count() <= 200), "{key}");
    }
}

#[test]
fn every_language_has_the_keys_of_english() {
    use mumcord_plugin_sdk::i18n::LANGUAGES;
    use std::collections::BTreeSet;

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("i18n");
    let read = |tag: &str| -> std::collections::BTreeMap<String, String> {
        let text = std::fs::read_to_string(dir.join(format!("{tag}.json"))).expect("language file");
        serde_json::from_str(&text).expect("flat JSON object of strings")
    };
    let english = read("en");
    // Plural forms differ per language, so compare the plain keys and, for plurals, the
    // placeholder only.
    let plain = |m: &std::collections::BTreeMap<String, String>| -> BTreeSet<String> {
        m.keys()
            .filter(|k| !k.starts_with("votes_"))
            .cloned()
            .collect()
    };
    for tag in LANGUAGES {
        let file = read(tag);
        assert_eq!(plain(&file), plain(&english), "keys of {tag}.json");
        assert!(
            file.contains_key("votes_other"),
            "{tag}.json needs votes_other"
        );
        for (key, text) in &file {
            if key.starts_with("votes_") {
                assert!(text.contains("{{count}}"), "{tag}.json {key}");
            } else {
                let placeholders = |t: &str| -> BTreeSet<String> {
                    t.split("{{")
                        .skip(1)
                        .filter_map(|r| r.split("}}").next())
                        .map(str::to_owned)
                        .collect()
                };
                assert_eq!(
                    placeholders(text),
                    placeholders(&english[key]),
                    "{tag}.json {key}"
                );
            }
        }
    }
}

#[test]
fn a_language_change_shows_the_panel_again_in_that_language() {
    let mut app = App::default();
    let mut host = Fake::default();
    app.start(&mut host);
    assert!(app.on_event(&mut host, &Event::LanguageChanged("de-AT".into())));
    assert!(
        host.texts("panel")
            .iter()
            .any(|t| t == "Keine Umfrage in diesem Kanal")
    );
    assert!(!app.on_event(&mut host, &Event::Timer(1)));
}

fn trigger(app: &mut App, host: &mut Fake, sender: &Peer, args: &str) {
    app.on_chat_trigger(host, sender, args);
}

#[test]
fn chat_triggers_count_once_per_person_and_can_change() {
    let (mut app, mut host) = creator();
    host.session = Some(9);
    let ben = peer(2, "Ben", Some("kb"));
    let dora = peer(4, "Dora", None);
    trigger(&mut app, &mut host, &ben, "1");
    trigger(&mut app, &mut host, &dora, "2 pasta please");
    trigger(&mut app, &mut host, &ben, "2");
    // Ben's plugin vote replaces his chat vote: the same peer key.
    app.on_peer_message(
        &mut host,
        &peer(7, "Ben", Some("kb")),
        KIND_VOTE,
        &Vote {
            poll_id: 1000,
            option: 0,
        }
        .encode(),
        true,
    );
    // The creator's own "!poll 2" counts as their own vote.
    trigger(&mut app, &mut host, &peer(9, "Anna", Some("ka")), "#2");
    let timer = host.timer_with(TALLY_DELAY_MS);
    app.on_timer(&mut host, timer);
    let tally = Tally::decode(&host.sent_of(KIND_TALLY)[0].1).expect("tally");
    assert_eq!(tally.counts, vec![1, 2]);
    assert!(
        host.texts(&own_card_id(1000))
            .contains(&"[vote-1]".to_owned())
    );
}

#[test]
fn bad_chat_triggers_change_nothing() {
    let (mut app, mut host) = creator();
    let ben = peer(2, "Ben", Some("kb"));
    for args in ["", "0", "3", "-1", "abc", "1.5", "99999999999999999999"] {
        trigger(&mut app, &mut host, &ben, args);
    }
    assert!(host.timers.iter().all(|&(_, ms)| ms != TALLY_DELAY_MS));
    app.on_action(&mut host, None, "close", None);
    trigger(&mut app, &mut host, &ben, "1");
    let tallies = host.sent_of(KIND_TALLY);
    assert_eq!(tallies.len(), 1);
    assert_eq!(
        Tally::decode(&tallies[0].1).expect("tally").counts,
        vec![0, 0]
    );
}

#[test]
fn only_the_creator_counts_chat_triggers() {
    let (mut app, mut host) = voter();
    trigger(&mut app, &mut host, &peer(2, "Ben", None), "1");
    assert!(host.sent.is_empty());
}

#[test]
fn the_fallback_line_names_the_trigger_when_it_is_granted() {
    let mut app = App::default();
    let mut host = Fake::default();
    app.set_granted(&mut host, vec!["chat.triggers".into()]);
    app.on_action(&mut host, None, "new-poll", None);
    assert!(
        host.texts("panel")
            .iter()
            .any(|t| t.contains("!poll and the option number"))
    );
    app.on_action(&mut host, None, "question", text("Q?"));
    app.on_action(&mut host, None, "option-0", text("A"));
    app.on_action(&mut host, None, "option-1", text("B"));
    app.on_action(&mut host, None, "create", None);
    assert_eq!(
        host.chat,
        vec!["📊 Q? · 1) A · 2) B · Vote with !poll 1 to !poll 2".to_owned()]
    );
}

#[test]
fn the_form_only_shows_in_the_popover() {
    let mut app = App::default();
    let mut host = Fake {
        popover_refused: true,
        ..Fake::default()
    };
    app.start(&mut host);
    app.on_action(&mut host, None, "new-poll", None);
    assert!(!host.texts("panel").contains(&"<question>".to_owned()));
    assert_eq!(host.warnings.len(), 1);

    // Expanded by the user, "New poll" works without opening anything.
    app.set_expanded(&mut host, true);
    app.on_action(&mut host, None, "new-poll", None);
    assert!(host.texts("panel").contains(&"<question>".to_owned()));
    app.on_action(&mut host, None, "question", text("Kept?"));

    // Back inline: the form goes away, the draft stays for next time.
    app.set_expanded(&mut host, false);
    assert!(!host.texts("panel").contains(&"<question>".to_owned()));
    app.set_expanded(&mut host, true);
    app.on_action(&mut host, None, "new-poll", None);
    assert!(host.texts("panel").contains(&"<question>".to_owned()));

    app.on_action(&mut host, None, "cancel", None);
    assert_eq!(host.popover, Some(false));
    assert!(host.texts("panel").contains(&"[new-poll]".to_owned()));
}

#[test]
fn the_panel_shows_the_current_poll_compact_inline_and_full_expanded() {
    let (mut app, mut host) = voter();
    let options: Vec<String> = ["A", "B", "C", "D"].map(str::to_owned).to_vec();
    let create = Create {
        poll_id: 2000,
        line_hash: 0,
        question: "Four?".into(),
        options,
    };
    app.on_peer_message(
        &mut host,
        &peer(5, "Eve", None),
        KIND_CREATE,
        &create.encode(),
        false,
    );
    let inline = host.texts("panel");
    assert!(inline.contains(&"Four?".to_owned()), "the newest poll");
    assert!(inline.contains(&"[vote-1]".to_owned()));
    assert!(!inline.contains(&"[vote-2]".to_owned()));
    assert!(inline.contains(&"[show-all]".to_owned()));
    assert!(inline.contains(&"[new-poll]".to_owned()));
    assert!(
        !inline.contains(&"[close]".to_owned()),
        "End is for the creator only"
    );

    app.on_action(&mut host, None, "show-all", None);
    assert_eq!(host.popover, Some(true));
    let expanded = host.texts("panel");
    assert!(expanded.contains(&"[vote-3]".to_owned()));
    assert!(!expanded.contains(&"[show-all]".to_owned()));

    // A vote from the panel goes to that poll's creator.
    app.on_action(&mut host, None, "vote-3", None);
    let (to, payload) = host.sent_of(KIND_VOTE).pop().expect("vote sent");
    assert_eq!(to, To::Direct(5));
    assert_eq!(Vote::decode(&payload).expect("vote").option, 3);

    // Eve leaves: her poll freezes and Anna's is current again.
    app.on_left(&mut host, &peer(5, "Eve", None));
    assert!(host.texts("panel").contains(&"Pizza or pasta?".to_owned()));
    app.on_left(&mut host, &peer(1, "Anna", None));
    assert!(
        host.texts("panel")
            .contains(&"No poll in this channel".to_owned())
    );
}

#[test]
fn new_poll_waits_while_my_poll_is_open() {
    let (mut app, mut host) = creator();
    let texts = host.texts("panel");
    assert!(!texts.contains(&"[new-poll]".to_owned()));
    assert!(texts.contains(&"[close]".to_owned()));
    app.on_action(&mut host, None, "new-poll", None);
    assert!(!host.texts("panel").contains(&"<question>".to_owned()));
}

#[test]
fn the_creators_card_hints_at_the_missing_trigger_permission() {
    let hint = "Turn on !poll voting in the plugin permissions".to_owned();
    let (mut app, mut host) = creator();
    assert!(host.texts(&own_card_id(1000)).contains(&hint));
    assert!(!host.texts("panel").contains(&hint));
    app.set_granted(&mut host, vec!["chat.triggers".into()]);
    assert!(!host.texts(&own_card_id(1000)).contains(&hint));
}

#[test]
fn my_vote_is_marked_on_the_card() {
    let (mut app, mut host) = creator();
    app.on_action(&mut host, Some(&own_card_id(1000)), "vote-1", None);
    let card = host.texts(&own_card_id(1000));
    assert!(
        card.iter()
            .any(|t| t.starts_with("[vote-1]") || t.starts_with("✓ Pasta"))
    );
}

#[test]
fn the_creators_own_lines_give_way_to_the_card() {
    let (mut app, mut host) = creator();
    // Without `events.presence` the own session is unknown: nothing to supersede.
    assert!(host.superseded.is_empty());
    app.on_action(&mut host, None, "close", None);
    assert!(host.superseded.is_empty());

    let mut app = App::default();
    let mut host = Fake {
        now: 1000,
        session: Some(9),
        ..Fake::default()
    };
    app.on_action(&mut host, None, "question", text("Pizza or pasta?"));
    app.on_action(&mut host, None, "option-0", text("Pizza"));
    app.on_action(&mut host, None, "option-1", text("Pasta"));
    app.on_action(&mut host, None, "create", None);
    app.on_action(&mut host, None, "close", None);
    let card = own_card_id(1000);
    assert_eq!(
        host.superseded,
        vec![
            (card.clone(), 9, host.chat[0].len() as u32 + 1000),
            (card, 9, host.chat[1].len() as u32 + 1000),
        ]
    );
}

#[test]
fn counted_chat_votes_are_hidden() {
    let (mut app, mut host) = creator();
    let ben = peer(2, "Ben", Some("kb"));
    trigger(&mut app, &mut host, &ben, "1");
    assert_eq!(host.hidden, 1);
    // Not a vote for this poll: the line stays.
    trigger(&mut app, &mut host, &ben, "3");
    trigger(&mut app, &mut host, &ben, "pizza");
    assert_eq!(host.hidden, 1);
}

#[test]
fn voters_hide_chat_votes_for_a_known_poll_without_counting() {
    let (mut app, mut host) = voter();
    trigger(&mut app, &mut host, &peer(2, "Ben", None), "2");
    assert_eq!(host.hidden, 1);
    assert!(host.sent.is_empty());

    // Without any poll the line stays.
    let mut app = App::default();
    let mut host = Fake::default();
    trigger(&mut app, &mut host, &peer(2, "Ben", None), "2");
    assert_eq!(host.hidden, 0);
}

#[test]
fn replayed_chat_votes_are_hidden_but_not_counted() {
    let (mut app, mut host) = creator();
    host.replay = true;
    trigger(&mut app, &mut host, &peer(2, "Ben", Some("kb")), "1");
    assert_eq!(host.hidden, 1);
    assert!(host.timers.iter().all(|&(_, ms)| ms != TALLY_DELAY_MS));
    assert!(host.texts("panel").iter().any(|t| t.contains("0 votes")));
}

#[test]
fn replayed_own_lines_of_a_known_poll_give_way_to_its_card() {
    let (mut app, mut host) = creator();
    app.on_action(&mut host, None, "close", None);
    let create_line = format!("[Poll] {}", host.chat[0]);
    let close_line = format!("[Poll] {}", host.chat[1]);
    // Live own lines are left to supersede.
    app.on_own_line(&mut host, &create_line);
    assert!(host.replaced.is_empty());

    host.replay = true;
    app.on_own_line(&mut host, &create_line);
    assert_eq!(host.replaced, vec![own_card_id(1000)]);
    app.on_own_line(&mut host, &close_line);
    assert_eq!(host.hidden, 1);
    // A line of a poll this client does not know stays as it is.
    app.on_own_line(&mut host, "[Poll] 📊 Tea?");
    app.on_own_line(&mut host, "no prefix");
    assert_eq!(host.replaced.len(), 1);
    assert_eq!(host.hidden, 1);
}

#[test]
fn replayed_lines_of_another_creators_poll_give_way_to_its_card() {
    let (mut app, mut host) = voter();
    host.replay = true;
    // The fake hash is the text length plus 1000; the voter's poll has line hash 77.
    app.on_own_line(&mut host, "[Poll] x");
    assert!(host.replaced.is_empty());
    let mut app = App::default();
    let mut host = Fake::default();
    let line = "📊 Pizza or pasta? · 1) Pizza · 2) Pasta";
    app.on_peer_message(
        &mut host,
        &peer(1, "Anna", Some("ka")),
        KIND_CREATE,
        &create_payload(1000, line.len() as u32 + 1000),
        false,
    );
    host.replay = true;
    app.on_own_line(&mut host, &format!("[Poll] {line}"));
    assert_eq!(host.replaced, vec![other_card_id(1, 1000)]);
}
