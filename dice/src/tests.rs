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
use crate::view::{self, Status};
use crate::wire::{KIND_COMMIT, KIND_REVEAL, KIND_WITNESS, Message};

// ---------------------------------------------------------------- notation

fn parsed(text: &str) -> Notation {
    notation::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
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
            notation::parse(input).map_err(|e| e.0),
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
        let error = notation::parse(input).expect_err(input).0;
        assert!(error.starts_with("Unknown roll."), "{input}: {error}");
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
    assert!(has_text(
        host.card_with("error-4"),
        "Unknown roll. Try d20, 2d6+3, d20 adv or 4d6 dl."
    ));
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
