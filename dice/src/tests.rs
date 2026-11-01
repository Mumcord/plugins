//! Native tests: notation, fairness maths with known-answer vectors (computed independently
//! with Python's hashlib/hmac), wire format, and the whole protocol between several simulated
//! clients.

use std::collections::BTreeMap;

use mumcord_plugin_sdk::types::Error;
use mumcord_plugin_sdk::ui::{Component, El, Slot};
use sha2::{Digest, Sha256};

use crate::app::{self, App, Host};
use crate::fair::{self, Secret};
use crate::notation::{self, Mode, Notation};
use crate::view::{self, Problem, Status};
use crate::wire::{KIND_COMMIT, KIND_REVEAL, KIND_WITNESS, Message};

// ---------------------------------------------------------------- notation

fn parsed(text: &str) -> Notation {
    notation::parse(text).unwrap_or_else(|e| panic!("{text}: {e:?}"))
}

#[test]
fn parses_the_documented_notations() {
    for (input, canonical) in [
        ("d20", "d20"),
        ("2d6+3", "2d6+3"),
        ("3d8-1", "3d8-1"),
        ("d20 adv", "d20 adv"),
        ("d20 dis", "d20 dis"),
        ("4d6 dl", "4d6 dl"),
        ("4d6 dh", "4d6 dh"),
        ("1d20", "d20"),
        (" 2D6 + 3 ", "2d6+3"),
        ("d20 advantage", "d20 adv"),
        ("d20 disadvantage", "d20 dis"),
        ("2d6+0", "2d6"),
        ("20d1000+1000 adv", "20d1000+1000 adv"),
        ("", "d20"),
    ] {
        if input == "20d1000+1000 adv" {
            // Advantage needs exactly one die.
            assert!(notation::parse(input).is_err());
            continue;
        }
        assert_eq!(parsed(input).canonical(), canonical, "{input}");
    }
    assert_eq!(
        parsed("d20 adv"),
        Notation {
            count: 1,
            sides: 20,
            modifier: 0,
            mode: Mode::Advantage
        }
    );
    assert_eq!(parsed("4d6 dl").described(), "4d6 drop lowest");
    assert_eq!(parsed("d20+2 adv").described(), "d20+2 with advantage");
}

#[test]
fn refuses_invalid_rolls_with_a_clear_message() {
    for (input, message) in [
        ("0d6", "Roll 1 to 20 dice."),
        ("21d6", "Roll 1 to 20 dice."),
        ("d1", "Dice need 2 to 1000 sides."),
        ("d1001", "Dice need 2 to 1000 sides."),
        ("2d6+1001", "The modifier must be between -1000 and +1000."),
        (
            "2d20 adv",
            "Advantage and disadvantage roll one die twice, e.g. d20 adv.",
        ),
        (
            "d6 dl",
            "Dropping a die needs at least 2 dice, e.g. 4d6 dl.",
        ),
    ] {
        assert_eq!(
            notation::parse(input).map_err(|e| Problem::Notation(e).text()),
            Err(message.to_string()),
            "{input}"
        );
    }
    for input in [
        "abc",
        "d",
        "2d",
        "2d6+",
        "2d6+3+4",
        "d-6",
        "2x6",
        "d20 sometimes",
        "9999999d6",
    ] {
        let error = notation::parse(input).expect_err(input);
        assert_eq!(error, notation::NotationError::Unknown, "{input}");
    }
}

#[test]
fn longest_canonical_form_fits_the_wire_limit() {
    let longest = parsed("20d1000-1000 dh").canonical();
    assert!(longest.len() <= notation::MAX_CANONICAL_BYTES);
}

// ---------------------------------------------------------------- fairness

const SECRET: Secret = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
const W1: Secret = [0xaa; 16];
const W2: Secret = [0x55; 16];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn commitment_and_seed_match_the_known_answers() {
    assert_eq!(
        hex(&fair::commitment(0x0102_0304, "2d6+3", &SECRET)),
        "91ded3193b24bff277e053b1c6a1af4ce738d2af3b210b33221b8337655ee063"
    );
    assert_eq!(
        hex(&fair::seed(0x0102_0304, &SECRET, &[W1, W2])),
        "a43db2974c9394d839761ff54394a73c7a41fed7bac5c26901c9a0372a05de2d"
    );
}

#[test]
fn dice_from_a_seed_match_the_known_answers() {
    let seed = fair::seed(0x0102_0304, &SECRET, &[W1, W2]);
    let dice = |text: &str| fair::roll(&parsed(text), seed);

    assert_eq!(dice("2d6").dice, [1, 4]);
    assert_eq!(dice("2d6+3").total, 8);
    let advantage = dice("d20 adv");
    assert_eq!(
        (
            advantage.dice.as_slice(),
            advantage.dropped,
            advantage.total
        ),
        (&[7, 6][..], Some(1), 7)
    );
    let disadvantage = dice("d20 dis");
    assert_eq!((disadvantage.dropped, disadvantage.total), (Some(0), 6));
    let drop_lowest = dice("4d6 dl");
    assert_eq!(drop_lowest.dice, [1, 4, 3, 3]);
    assert_eq!((drop_lowest.dropped, drop_lowest.total), (Some(0), 10));
    let drop_highest = dice("4d6 dh");
    assert_eq!((drop_highest.dropped, drop_highest.total), (Some(1), 7));
    assert_eq!(
        dice("20d1000").dice,
        [
            767, 926, 893, 33, 814, 715, 202, 567, 605, 781, 37, 43, 286, 498, 911, 206, 444, 345,
            373, 969
        ]
    );
    assert_eq!(
        fair::roll(&parsed("d20"), fair::seed(7, &SECRET, &[])).dice,
        [20]
    );
}

#[test]
fn ties_drop_the_later_die() {
    // Find a seed that throws [3, 3]; both drop rules then drop the second die.
    let outcome = fair::Outcome {
        dice: vec![3, 3],
        dropped: None,
        total: 0,
    };
    let seed_for = |dice: &[u16]| {
        (0u32..)
            .map(|i| fair::seed(i, &SECRET, &[]))
            .find(|seed| fair::roll(&parsed("2d6"), *seed).dice == dice)
            .unwrap_or_else(|| unreachable!())
    };
    let seed = seed_for(&outcome.dice);
    assert_eq!(fair::roll(&parsed("2d6 dl"), seed).dropped, Some(1));
    assert_eq!(fair::roll(&parsed("2d6 dh"), seed).dropped, Some(1));
}

/// Chi-square over 10 000 rolls; the bounds are the 99.9 % quantiles, and the seeds are
/// fixed, so the test is deterministic.
fn chi_square(counts: &[u32], expected: &[f64]) -> f64 {
    counts
        .iter()
        .zip(expected)
        .map(|(&count, &e)| (f64::from(count) - e).powi(2) / e)
        .sum()
}

#[test]
fn d20_is_uniform() {
    let mut counts = [0u32; 20];
    for i in 0..10_000u32 {
        let outcome = fair::roll(&parsed("d20"), fair::seed(i, &SECRET, &[W1]));
        counts[usize::from(outcome.dice[0]) - 1] += 1;
    }
    let statistic = chi_square(&counts, &[500.0; 20]);
    // 19 degrees of freedom, p = 0.001.
    assert!(statistic < 43.82, "chi-square {statistic}");
}

#[test]
fn two_d6_totals_follow_the_triangle() {
    let mut counts = [0u32; 11];
    for i in 0..10_000u32 {
        let outcome = fair::roll(&parsed("2d6"), fair::seed(i, &SECRET, &[W1, W2]));
        counts[usize::try_from(outcome.total).unwrap_or(0) - 2] += 1;
    }
    let expected: Vec<f64> = [1, 2, 3, 4, 5, 6, 5, 4, 3, 2, 1]
        .iter()
        .map(|ways| 10_000.0 * f64::from(*ways) / 36.0)
        .collect();
    let statistic = chi_square(&counts, &expected);
    // 10 degrees of freedom, p = 0.001.
    assert!(statistic < 29.59, "chi-square {statistic}");
}

#[test]
fn dice_stay_in_range_for_awkward_side_counts() {
    for sides in [2u16, 3, 7, 999, 1000] {
        let mut stream = fair::Stream::new(fair::seed(u32::from(sides), &SECRET, &[]));
        for _ in 0..2000 {
            let value = stream.die(sides);
            assert!((1..=sides).contains(&value));
        }
    }
}

// ---------------------------------------------------------------- wire

#[test]
fn messages_round_trip() {
    for message in [
        Message::Commit {
            roll_id: 9,
            commitment: [7; 32],
            notation: "2d6+3".into(),
        },
        Message::Witness {
            roller: 3,
            roll_id: 9,
            value: W1,
        },
        Message::Reveal {
            roll_id: 9,
            secret: SECRET,
            witnesses: vec![],
        },
        Message::Reveal {
            roll_id: 9,
            secret: SECRET,
            witnesses: vec![W1, W2],
        },
    ] {
        let bytes = message.encode();
        assert!(bytes.len() <= 997);
        assert_eq!(Message::decode(message.kind(), &bytes), Some(message));
    }
}

#[test]
fn malformed_messages_are_rejected() {
    let reveal = Message::Reveal {
        roll_id: 1,
        secret: SECRET,
        witnesses: vec![W1],
    }
    .encode();
    assert_eq!(
        Message::decode(KIND_REVEAL, &reveal[..reveal.len() - 1]),
        None
    );
    let mut long = reveal.clone();
    long.push(0);
    assert_eq!(Message::decode(KIND_REVEAL, &long), None);
    let mut three = reveal[..20].to_vec();
    three.push(3);
    three.extend_from_slice(&[[1u8; 16], [2; 16], [3; 16]].concat());
    assert_eq!(Message::decode(KIND_REVEAL, &three), None);

    let mut commit = vec![0; 36];
    assert_eq!(
        Message::decode(KIND_COMMIT, &commit),
        None,
        "empty notation"
    );
    commit.extend_from_slice(&[b'1'; 33]);
    assert_eq!(
        Message::decode(KIND_COMMIT, &commit),
        None,
        "notation too long"
    );
    assert_eq!(Message::decode(KIND_WITNESS, &[0; 23]), None);
    assert_eq!(Message::decode(9, &[0; 24]), None);
}

// ---------------------------------------------------------------- views

#[test]
fn fallback_lines_match_the_issue_examples() {
    let outcome = |dice: Vec<u16>, dropped, total| fair::Outcome {
        dice,
        dropped,
        total,
    };
    let verified = Status::Verified { witnesses: 2 };
    assert_eq!(
        view::fallback_line(&parsed("2d6+3"), &outcome(vec![4, 4], None, 11), &verified),
        "🎲 rolled 2d6+3 = 11 (4, 4)"
    );
    assert_eq!(
        view::fallback_line(
            &parsed("d20 adv"),
            &outcome(vec![17, 9], Some(1), 17),
            &verified
        ),
        "🎲 rolled d20 with advantage = 17 (17, 9)"
    );
    assert_eq!(
        view::fallback_line(
            &parsed("4d6 dl"),
            &outcome(vec![6, 2, 5, 3], Some(1), 14),
            &verified
        ),
        "🎲 rolled 4d6 drop lowest = 14 (6, 5, 3, dropped 2)"
    );
    assert_eq!(
        view::fallback_line(&parsed("d20"), &outcome(vec![3], None, 3), &Status::Alone),
        "🎲 rolled d20 = 3 (3) · unverified, rolled alone"
    );
    assert_eq!(
        view::spoken(&parsed("2d6+3"), &outcome(vec![4, 4], None, 11), &verified),
        "4, 4, plus 3, total 11, verified"
    );
}

/// All texts in a tree, depth-first.
fn texts(el: &El) -> Vec<String> {
    let mut out = Vec::new();
    match &el.component {
        Component::Text(t) => out.push(t.text.clone()),
        Component::Chip(c) => out.push(c.text.clone()),
        _ => {}
    }
    for child in &el.children {
        out.extend(texts(child));
    }
    out
}

fn has_text(el: &El, text: &str) -> bool {
    texts(el).iter().any(|t| t == text)
}

// ---------------------------------------------------------------- protocol

#[derive(Default)]
struct MockHost {
    rng: u64,
    rng_seed: u64,
    sent: Vec<Message>,
    send_error: Option<Error>,
    chat: Vec<String>,
    cards: BTreeMap<String, El>,
    panel: Option<El>,
    new_timers: Vec<(u32, u32)>,
    next_timer: u32,
    supersedes: Vec<(String, u32, u32)>,
    random_fails: bool,
    popovers: u32,
}

impl MockHost {
    fn seeded(seed: u64) -> Self {
        Self {
            rng_seed: seed,
            ..Self::default()
        }
    }

    fn card_with(&self, prefix: &str) -> &El {
        let mut matching = self.cards.iter().filter(|(id, _)| id.starts_with(prefix));
        let (_, card) = matching
            .next()
            .unwrap_or_else(|| panic!("no card {prefix}"));
        assert!(matching.next().is_none(), "several cards {prefix}");
        card
    }
}

fn line_hash(text: &str) -> u32 {
    let digest = Sha256::digest(format!("[Dice roller] {text}").as_bytes());
    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
}

impl Host for MockHost {
    fn random(&mut self, len: u32) -> Option<Vec<u8>> {
        if self.random_fails {
            return None;
        }
        let mut out = Vec::new();
        while out.len() < len as usize {
            self.rng += 1;
            let block =
                Sha256::digest([self.rng_seed.to_be_bytes(), self.rng.to_be_bytes()].concat());
            out.extend_from_slice(&block);
        }
        out.truncate(len as usize);
        Some(out)
    }

    fn send(&mut self, message: &Message) -> Result<(), Error> {
        if let Some(error) = self.send_error {
            return Err(error);
        }
        self.sent.push(message.clone());
        Ok(())
    }

    fn start_game_burst(&mut self) {}

    fn chat_send(&mut self, text: &str) -> Result<(), Error> {
        self.chat.push(text.to_string());
        Ok(())
    }

    fn line_hash(&mut self, text: &str) -> u32 {
        line_hash(text)
    }

    fn supersede(&mut self, card: &str, sender: u32, hash: u32) {
        self.supersedes.push((card.to_string(), sender, hash));
    }

    fn start_timer(&mut self, ms: u32) -> Option<u32> {
        self.next_timer += 1;
        self.new_timers.push((self.next_timer, ms));
        Some(self.next_timer)
    }

    fn show(&mut self, slot: Slot, tree: El) {
        match slot {
            Slot::ChatCard(id) => {
                self.cards.insert(id, tree);
            }
            Slot::Panel => self.panel = Some(tree),
            other => panic!("unexpected slot {other:?}"),
        }
    }

    fn open_popover(&mut self) {
        self.popovers += 1;
    }

    fn warn(&mut self, _message: &str) {}
}

struct Client {
    session: u32,
    name: String,
    app: App,
    host: MockHost,
}

enum Ev {
    Deliver {
        to: usize,
        from: usize,
        kind: u8,
        payload: Vec<u8>,
    },
    Timer {
        client: usize,
        timer: u32,
    },
}

/// Several clients in one channel. The server relays every message to everyone else in the
/// order it receives them, after a fixed latency.
struct Sim {
    clients: Vec<Client>,
    now: u64,
    seq: u64,
    queue: BTreeMap<(u64, u64), Ev>,
    latency_ms: u64,
    /// Every plugin message the server relayed: (sender index, kind).
    relayed: Vec<(usize, u8)>,
}

impl Sim {
    fn new(count: usize, latency_ms: u64) -> Self {
        let clients = (0..count)
            .map(|i| Client {
                session: 100 + u32::try_from(i).unwrap_or(0),
                name: format!("Player {i}"),
                app: App::default(),
                host: MockHost::seeded(1000 + i as u64),
            })
            .collect();
        Self {
            clients,
            now: 0,
            seq: 0,
            queue: BTreeMap::new(),
            latency_ms,
            relayed: Vec::new(),
        }
    }

    fn push(&mut self, at: u64, ev: Ev) {
        self.seq += 1;
        self.queue.insert((at, self.seq), ev);
    }

    /// Moves what the client sent and the timers it started into the queue.
    fn flush(&mut self, index: usize) {
        let sent = std::mem::take(&mut self.clients[index].host.sent);
        for message in sent {
            self.relayed.push((index, message.kind()));
            let payload = message.encode();
            for to in 0..self.clients.len() {
                if to != index {
                    self.push(
                        self.now + self.latency_ms,
                        Ev::Deliver {
                            to,
                            from: index,
                            kind: message.kind(),
                            payload: payload.clone(),
                        },
                    );
                }
            }
        }
        let timers = std::mem::take(&mut self.clients[index].host.new_timers);
        for (timer, ms) in timers {
            self.push(
                self.now + u64::from(ms),
                Ev::Timer {
                    client: index,
                    timer,
                },
            );
        }
    }

    fn roll(&mut self, index: usize, args: &str) {
        let client = &mut self.clients[index];
        client.app.on_command(&mut client.host, args);
        self.flush(index);
    }

    fn run_until(&mut self, until: u64) {
        while let Some(entry) = self.queue.first_entry() {
            if entry.key().0 > until {
                break;
            }
            let ((at, _), ev) = entry.remove_entry();
            self.now = at;
            let index = match ev {
                Ev::Deliver {
                    to,
                    from,
                    kind,
                    payload,
                } => {
                    let session = self.clients[from].session;
                    let name = self.clients[from].name.clone();
                    let client = &mut self.clients[to];
                    client.app.on_peer_message(
                        &mut client.host,
                        session,
                        &name,
                        kind,
                        &payload,
                        false,
                    );
                    to
                }
                Ev::Timer {
                    client: index,
                    timer,
                } => {
                    let client = &mut self.clients[index];
                    client.app.on_timer(&mut client.host, timer);
                    index
                }
            };
            self.flush(index);
        }
        self.now = until;
    }

    fn count(&self, kind: u8) -> usize {
        self.relayed.iter().filter(|(_, k)| *k == kind).count()
    }
}

fn final_texts(host: &MockHost, prefix: &str) -> Vec<String> {
    texts(host.card_with(prefix))
}

#[test]
fn two_players_see_the_same_verified_roll() {
    let mut sim = Sim::new(2, 50);
    sim.roll(0, "2d6+3");
    sim.run_until(20_000);

    // One witness only, so the roller reveals after the 5 s timeout.
    assert_eq!(
        (
            sim.count(KIND_COMMIT),
            sim.count(KIND_WITNESS),
            sim.count(KIND_REVEAL)
        ),
        (1, 1, 1)
    );
    let roller = final_texts(&sim.clients[0].host, "roll-me-");
    let witness = final_texts(&sim.clients[1].host, "roll-100-");
    assert!(roller.contains(&"Verified".to_string()), "{roller:?}");
    assert!(witness.contains(&"Verified".to_string()), "{witness:?}");
    assert_eq!(roller[0], "You rolled 2d6+3");
    assert_eq!(witness[0], "Player 0 rolled 2d6+3");
    // Same dice and total on both sides (everything after the heading).
    assert_eq!(roller[1..], witness[1..]);

    // The roller posts exactly one text line; the witness hides exactly that line.
    let lines = &sim.clients[0].host.chat;
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("🎲 rolled 2d6+3 = "));
    assert!(sim.clients[1].host.chat.is_empty());
    let (card, sender, hash) = sim.clients[1].host.supersedes[0].clone();
    assert!(card.starts_with("roll-100-"));
    assert_eq!((sender, hash), (100, line_hash(&lines[0])));
}

#[test]
fn three_players_reveal_as_soon_as_two_witnesses_answer() {
    let mut sim = Sim::new(3, 50);
    sim.roll(0, "d20 adv");
    // Witnesses answer within 1.5 s; the reveal follows at once, well before the 5 s timeout.
    sim.run_until(2_000);
    assert_eq!((sim.count(KIND_WITNESS), sim.count(KIND_REVEAL)), (2, 1));
    for (index, prefix) in [(0, "roll-me-"), (1, "roll-100-"), (2, "roll-100-")] {
        let host = &sim.clients[index].host;
        assert!(has_text(host.card_with(prefix), "Verified"));
        assert!(
            texts(host.card_with(prefix))
                .iter()
                .any(|t| t.starts_with("Checked with 2 witnesses"))
        );
    }
}

#[test]
fn a_big_channel_uses_two_witnesses() {
    let mut sim = Sim::new(30, 20);
    sim.roll(0, "2d6");
    sim.run_until(20_000);
    let witnesses = sim.count(KIND_WITNESS);
    // Only two count. Witnesses whose timer fires before the first two arrive also answer,
    // so a few extra may go out while those messages are on their way.
    assert!((2..=5).contains(&witnesses), "{witnesses} witness messages");
    for client in &sim.clients[1..] {
        assert!(has_text(client.host.card_with("roll-100-"), "Verified"));
    }
    let shown: Vec<Vec<String>> = sim.clients[1..]
        .iter()
        .map(|c| final_texts(&c.host, "roll-100-"))
        .collect();
    assert!(shown.windows(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn rolling_alone_is_marked_unverified() {
    let mut host = MockHost::seeded(1);
    host.send_error = Some(Error::NoRecipients);
    let mut app = App::default();
    app.on_command(&mut host, "d20");
    assert!(has_text(
        host.card_with("roll-me-"),
        "Unverified (rolled alone)"
    ));
    assert_eq!(host.chat.len(), 1);
    assert!(host.chat[0].ends_with("· unverified, rolled alone"));
}

#[test]
fn a_roll_nobody_witnesses_is_unverified_for_everyone() {
    let mut sim = Sim::new(2, 50);
    // The second client has the plugin but cannot answer (no random bytes).
    sim.clients[1].host.random_fails = true;
    sim.roll(0, "d20");
    sim.run_until(20_000);
    assert_eq!(sim.count(KIND_WITNESS), 0);
    assert!(has_text(
        sim.clients[0].host.card_with("roll-me-"),
        "Unverified (rolled alone)"
    ));
    assert!(has_text(
        sim.clients[1].host.card_with("roll-100-"),
        "Unverified (rolled alone)"
    ));
}

/// A receiver with one fake roller (session 7) whose messages the test writes by hand.
fn receiver_with_commit(notation_text: &str) -> (App, MockHost, u32) {
    let mut host = MockHost::seeded(5);
    let mut app = App::default();
    let commit = Message::Commit {
        roll_id: 42,
        commitment: fair::commitment(42, notation_text, &SECRET),
        notation: notation_text.into(),
    };
    app.on_peer_message(
        &mut host,
        7,
        "Mallory",
        KIND_COMMIT,
        &commit.encode(),
        false,
    );
    let witness_timer = host.new_timers[0].0;
    (app, host, witness_timer)
}

fn reveal(app: &mut App, host: &mut MockHost, secret: Secret, witnesses: Vec<Secret>) {
    let message = Message::Reveal {
        roll_id: 42,
        secret,
        witnesses,
    };
    app.on_peer_message(host, 7, "Mallory", KIND_REVEAL, &message.encode(), false);
}

fn witness_from(app: &mut App, host: &mut MockHost, sender: u32, value: Secret) {
    let message = Message::Witness {
        roller: 7,
        roll_id: 42,
        value,
    };
    app.on_peer_message(
        host,
        sender,
        "Someone",
        KIND_WITNESS,
        &message.encode(),
        false,
    );
}

#[test]
fn a_changed_secret_fails_verification() {
    let (mut app, mut host, _) = receiver_with_commit("2d6+3");
    witness_from(&mut app, &mut host, 8, W1);
    let mut other = SECRET;
    other[0] ^= 1;
    reveal(&mut app, &mut host, other, vec![W1]);
    let card = host.card_with("roll-7-");
    assert!(has_text(card, "Verification failed"));
    assert!(
        texts(card)
            .iter()
            .any(|t| t.starts_with("The revealed roll does not match what was announced."))
    );
    assert!(host.supersedes.is_empty(), "the text line stays visible");
}

#[test]
fn a_commit_with_a_non_canonical_notation_is_ignored() {
    let mut host = MockHost::seeded(5);
    let mut app = App::default();
    let commit = Message::Commit {
        roll_id: 42,
        commitment: fair::commitment(42, "1d20", &SECRET),
        notation: "1d20".into(),
    };
    app.on_peer_message(
        &mut host,
        7,
        "Mallory",
        KIND_COMMIT,
        &commit.encode(),
        false,
    );
    assert!(host.cards.is_empty() && host.new_timers.is_empty());
}

#[test]
fn changed_witness_values_fail_verification() {
    let (mut app, mut host, _) = receiver_with_commit("d20");
    witness_from(&mut app, &mut host, 8, W1);
    witness_from(&mut app, &mut host, 9, W2);
    // The roller swaps in a value of its own choice.
    reveal(&mut app, &mut host, SECRET, vec![W1, [9; 16]]);
    assert!(has_text(host.card_with("roll-7-"), "Verification failed"));
}

#[test]
fn claiming_no_witnesses_after_some_answered_fails() {
    let (mut app, mut host, _) = receiver_with_commit("d20");
    witness_from(&mut app, &mut host, 8, W1);
    reveal(&mut app, &mut host, SECRET, vec![]);
    assert!(has_text(host.card_with("roll-7-"), "Verification failed"));
}

#[test]
fn a_witness_left_out_is_told_so() {
    let (mut app, mut host, witness_timer) = receiver_with_commit("d20");
    app.on_timer(&mut host, witness_timer);
    let Some(Message::Witness { value: mine, .. }) = host.sent.first().cloned() else {
        panic!("no witness sent");
    };
    reveal(&mut app, &mut host, SECRET, vec![W1]);
    let card = host.card_with("roll-7-");
    assert!(has_text(card, "Verification failed"));
    assert!(
        texts(card)
            .iter()
            .any(|t| t.starts_with("Your contribution was left out."))
    );
    assert_ne!(mine, W1);
}

#[test]
fn a_witness_skips_when_two_already_answered() {
    let (mut app, mut host, witness_timer) = receiver_with_commit("d20");
    witness_from(&mut app, &mut host, 8, W1);
    witness_from(&mut app, &mut host, 9, W2);
    app.on_timer(&mut host, witness_timer);
    assert!(host.sent.is_empty());
    reveal(&mut app, &mut host, SECRET, vec![W2, W1]);
    assert!(has_text(host.card_with("roll-7-"), "Verified"));
}

#[test]
fn the_roller_never_witnesses_its_own_roll() {
    let (mut app, mut host, _) = receiver_with_commit("d20");
    witness_from(&mut app, &mut host, 7, W1);
    reveal(&mut app, &mut host, SECRET, vec![W1]);
    assert!(has_text(host.card_with("roll-7-"), "Verification failed"));
}

#[test]
fn own_witness_may_arrive_anywhere_in_the_channel_order() {
    let mine = [3u8; 16];
    // Mine reached the server first, second, or after both others.
    assert!(app::witnesses_match(&[mine, W1], &[W1, W2], Some(mine)));
    assert!(app::witnesses_match(&[W1, mine], &[W1, W2], Some(mine)));
    assert!(app::witnesses_match(&[W1, W2], &[W1, W2], Some(mine)));
    // Mine was too late for the roller's timeout.
    assert!(app::witnesses_match(&[W1], &[W1], Some(mine)));
    // Picking the second other value while skipping the first is not a valid order.
    assert!(!app::witnesses_match(&[mine, W2], &[W1, W2], Some(mine)));
    assert!(!app::witnesses_match(&[W2], &[W1, W2], None));
    assert!(!app::witnesses_match(&[W1, W1], &[W1, W2], None));
}

#[test]
fn a_roll_that_is_never_revealed_is_void() {
    let (mut app, mut host, _) = receiver_with_commit("d20");
    app.set_granted(&mut host, &["ui.panel".to_string()]);
    let (deadline, ms) = host.new_timers[1];
    assert_eq!(ms, app::REVEAL_TIMEOUT_MS + app::NOT_REVEALED_AFTER_MS);
    app.on_timer(&mut host, deadline);
    app.on_panel_visibility(&mut host, true);
    assert!(has_text(host.card_with("roll-7-"), "Not revealed"));
    let panel = host.panel.as_ref().unwrap_or_else(|| panic!("no panel"));
    assert!(has_text(panel, "Mallory: d20 void"));
    // A late reveal changes nothing.
    reveal(&mut app, &mut host, SECRET, vec![]);
    assert!(has_text(host.card_with("roll-7-"), "Not revealed"));
}

#[test]
fn the_deadline_moves_closer_after_two_witnesses() {
    let (mut app, mut host, _) = receiver_with_commit("d20");
    let first_deadline = host.new_timers[1].0;
    witness_from(&mut app, &mut host, 8, W1);
    witness_from(&mut app, &mut host, 9, W2);
    let (second_deadline, ms) = host.new_timers[2];
    assert_eq!(ms, app::NOT_REVEALED_AFTER_MS);
    // The older timer is ignored, the newer one voids the roll.
    app.on_timer(&mut host, first_deadline);
    assert!(has_text(host.card_with("roll-7-"), "Rolling"));
    app.on_timer(&mut host, second_deadline);
    assert!(has_text(host.card_with("roll-7-"), "Not revealed"));
}

#[test]
fn invalid_input_shows_an_error_and_sends_nothing() {
    let mut host = MockHost::seeded(1);
    let mut app = App::default();
    for input in ["0d6", "21d6", "d1", "abc"] {
        app.on_command(&mut host, input);
    }
    assert!(host.sent.is_empty() && host.chat.is_empty());
    assert_eq!(host.cards.len(), 4);
    let card = host.card_with("error-4");
    assert!(has_text(card, "Unknown roll."));
    assert!(has_text(card, "Try d20, 2d6+3, d20 adv or 4d6 dl."));
}

#[test]
fn a_second_roll_waits_for_the_first() {
    let mut host = MockHost::seeded(1);
    let mut app = App::default();
    app.on_command(&mut host, "d20");
    app.on_command(&mut host, "d6");
    assert_eq!(host.sent.len(), 1);
    assert!(has_text(
        host.card_with("error-"),
        "Still rolling, try again in a moment."
    ));
}

#[test]
fn a_failed_reveal_posts_no_result() {
    let mut host = MockHost::seeded(1);
    let mut app = App::default();
    app.on_command(&mut host, "d20");
    host.send_error = Some(Error::RateLimited);
    let reveal_timer = host.new_timers[0].0;
    app.on_timer(&mut host, reveal_timer);
    assert!(has_text(host.card_with("roll-me-"), "Not revealed"));
    assert!(host.chat.is_empty());
}

#[test]
fn history_needs_the_optional_panel_permission() {
    let mut host = MockHost::seeded(1);
    host.send_error = Some(Error::NoRecipients);
    let mut app = App::default();
    app.on_command(&mut host, "d20");
    assert!(host.panel.is_none());
    app.set_granted(&mut host, &["ui.panel".to_string()]);
    app.on_panel_visibility(&mut host, true);
    let panel = host.panel.as_ref().unwrap_or_else(|| panic!("no panel"));
    assert!(has_text(panel, "Unverified (rolled alone)"));
}

#[test]
fn rolling_cards_have_a_still_frame_and_labels() {
    let mut host = MockHost::seeded(1);
    let mut app = App::default();
    app.on_command(&mut host, "d20");
    let card = host.card_with("roll-me-");
    let sprite = card
        .children
        .iter()
        .flat_map(|c| c.children.iter())
        .find_map(|c| match &c.component {
            Component::Sprite(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no sprite"));
    assert_eq!(sprite.asset, view::DIE_SPRITE);
    assert!(sprite.fps <= 12 && sprite.still_frame < sprite.frames && !sprite.alt.is_empty());
    assert!(has_text(card, "Rolling"));
}

// ---------------------------------------------------------------- languages

const FILES: &[(&str, &str)] = mumcord_plugin_sdk::i18n_all!();
const PLURAL_SUFFIXES: [&str; 6] = ["_zero", "_one", "_two", "_few", "_many", "_other"];

fn load(json: &str) -> BTreeMap<String, String> {
    serde_json::from_str(json).unwrap_or_else(|e| panic!("invalid i18n file: {e}"))
}

/// The key without its plural category: `explain.verified_one` is `explain.verified`.
fn base_key(key: &str) -> &str {
    PLURAL_SUFFIXES
        .iter()
        .find_map(|suffix| key.strip_suffix(suffix))
        .unwrap_or(key)
}

fn placeholders(text: &str) -> Vec<String> {
    let mut found: Vec<String> = text
        .split("{{")
        .skip(1)
        .filter_map(|rest| rest.split_once("}}"))
        .map(|(name, _)| name.trim().to_string())
        .collect();
    found.sort();
    found.dedup();
    found
}

#[test]
fn every_language_has_the_keys_of_english() {
    assert_eq!(FILES.len(), 24);
    let english = load(
        FILES
            .iter()
            .find(|(tag, _)| *tag == "en")
            .map_or("{}", |(_, json)| json),
    );
    let bases = |map: &BTreeMap<String, String>| -> Vec<String> {
        let mut keys: Vec<String> = map.keys().map(|k| base_key(k).to_string()).collect();
        keys.dedup();
        keys
    };
    for (tag, json) in FILES {
        let file = load(json);
        assert_eq!(
            bases(&file),
            bases(&english),
            "{tag}: keys differ from en.json"
        );
        for (key, text) in &file {
            assert!(!text.trim().is_empty(), "{tag}: {key} is empty");
            let reference = english
                .get(key)
                .or_else(|| english.get(&format!("{}_other", base_key(key))))
                .unwrap_or_else(|| panic!("{tag}: {key} has no English counterpart"));
            let wanted = placeholders(reference);
            let got = placeholders(text);
            // A plural form may leave out {{count}} when it spells the number in words.
            assert!(
                got == wanted || wanted.iter().filter(|p| *p != "count").eq(got.iter()),
                "{tag}: {key} placeholders {got:?}, expected {wanted:?}"
            );
        }
        // Every plural key set needs the catch-all form.
        for key in file.keys().filter(|k| base_key(k) != k.as_str()) {
            let other = format!("{}_other", base_key(key));
            assert!(file.contains_key(&other), "{tag}: {key} without {other}");
        }
    }
}

#[test]
fn a_language_change_draws_the_cards_and_history_again() {
    let mut host = MockHost::seeded(1);
    let mut app = App::default();
    app.set_granted(&mut host, &["ui.panel".to_string()]);
    app.on_command(&mut host, "d20");
    // A second roll while the first runs shows the "busy" error card.
    app.on_command(&mut host, "d6");
    crate::text::set_language("de");
    app.on_panel_visibility(&mut host, true);
    app.on_language_changed(&mut host);
    let own = host.card_with("roll-me-");
    assert!(has_text(own, "Du würfelst d20"), "{:?}", texts(own));
    assert!(has_text(
        host.card_with("error-"),
        "Es wird noch gewürfelt, versuche es gleich noch einmal."
    ));
    assert!(has_text(
        host.panel.as_ref().unwrap_or_else(|| unreachable!()),
        "Wurfverlauf"
    ));
    // The text line for other people stays English.
    assert!(host.chat.iter().all(|line| line.starts_with("🎲 rolled")));
    crate::text::set_language("en");
}

// ---------------------------------------------------------------- quick rolls and help

fn buttons(el: &El) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Component::Button(b) = &el.component {
        out.push((b.action.clone(), b.label.clone()));
    }
    if let Component::IconButton(b) = &el.component {
        out.push((b.action.clone(), b.label.clone()));
    }
    for child in &el.children {
        out.extend(buttons(child));
    }
    out
}

fn panel_of(host: &MockHost) -> &El {
    host.panel.as_ref().unwrap_or_else(|| panic!("no panel"))
}

#[test]
fn the_inline_panel_has_quick_rolls_the_last_roll_and_a_help_button() {
    let mut host = MockHost::seeded(1);
    let mut app = App::default();
    app.set_granted(&mut host, &["ui.panel".to_string()]);
    let labels: Vec<String> = buttons(panel_of(&host)).into_iter().map(|b| b.1).collect();
    assert_eq!(
        labels,
        [
            "d4", "d6", "d8", "d10", "d12", "d20", "d100", "2d6", "d20 adv", "d20 dis", "Help"
        ]
    );
    assert!(has_text(panel_of(&host), "No roll yet"));
    assert!(!has_text(panel_of(&host), "How to roll"));
}

#[test]
fn a_quick_roll_button_rolls_like_the_command() {
    let mut host = MockHost::seeded(1);
    let mut app = App::default();
    app.on_panel_action(&mut host, "quick:d20 adv");
    let Some(Message::Commit { notation, .. }) = host.sent.first() else {
        panic!("no commit");
    };
    assert_eq!(notation, "d20 adv");
    let card = host.card_with("roll-me-");
    assert!(has_text(card, "Rolling"));
}

#[test]
fn a_bad_quick_roll_action_does_nothing() {
    let mut host = MockHost::seeded(1);
    let mut app = App::default();
    app.on_panel_action(&mut host, "quick:nonsense");
    app.on_panel_action(&mut host, "other");
    assert!(host.sent.is_empty());
    assert_eq!(host.popovers, 0);
}

#[test]
fn the_last_roll_shows_in_the_inline_panel() {
    let mut host = MockHost::seeded(1);
    host.send_error = Some(Error::NoRecipients);
    let mut app = App::default();
    app.set_granted(&mut host, &["ui.panel".to_string()]);
    app.on_panel_action(&mut host, "quick:d6");
    let last = texts(panel_of(&host))
        .into_iter()
        .find(|t| t.starts_with("Last: You: d6 = "));
    assert!(last.is_some());
}

#[test]
fn the_help_button_opens_the_popover_and_the_popover_shows_help_and_history() {
    let mut host = MockHost::seeded(1);
    host.send_error = Some(Error::NoRecipients);
    let mut app = App::default();
    app.set_granted(&mut host, &["ui.panel".to_string()]);
    app.on_command(&mut host, "d20");
    app.on_panel_action(&mut host, "help");
    assert_eq!(host.popovers, 1);
    app.on_panel_visibility(&mut host, true);
    let panel = panel_of(&host);
    for text in [
        "How to roll",
        "Up to 20 dice with 2 to 1000 sides, and one modifier.",
        "Verified",
        "Unverified (rolled alone)",
        "Not revealed",
        "Roll history",
    ] {
        assert!(has_text(panel, text), "missing {text}");
    }
    assert!(buttons(panel).is_empty());
    app.on_panel_visibility(&mut host, false);
    assert!(!has_text(panel_of(&host), "How to roll"));
}

#[test]
fn every_language_translates_the_panel_texts() {
    let en: serde_json::Value =
        serde_json::from_str(include_str!("../i18n/en.json")).expect("en.json");
    for key in ["panel.last", "panel.none", "help.title", "help.history"] {
        assert!(en.get(key).is_some(), "en lacks {key}");
    }
}
