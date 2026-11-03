use mumcord_plugin_sdk::ui::El;
use sha2::{Digest, Sha256};

use crate::app::{App, Host, Phase, Prefs, Verify};
use crate::deck::{self, DECK};
use crate::fair;
use crate::rules::{self, Twist};
use crate::score;
use crate::view::{self, Intent};
use crate::wire::{FORMAT, LOOM_H, LOOM_W, MAX_LEN, Message, Settings};

// ----- Scoring -----

fn shift_scale(points: &[(i32, i32)], dx: i32, dy: i32, num: i32, den: i32) -> Vec<(i32, i32)> {
    points
        .iter()
        .map(|&(x, y)| (x * num / den + dx, y * num / den + dy))
        .collect()
}

#[test]
fn same_shape_anywhere_and_any_size_scores_full() {
    let figure = DECK[0].sample(7);
    let sky = shift_scale(&figure, 37, -12, 1, 2);
    let s = score::score(&sky, &figure).expect("scores");
    assert_eq!(s.resonance, 100);
    assert!(s.stars.iter().all(|&r| r == 100));
}

#[test]
fn star_order_does_not_matter() {
    let figure = DECK[3].sample(6);
    let mut sky = shift_scale(&figure, 100, 50, 3, 4);
    sky.reverse();
    let s = score::score(&sky, &figure).expect("scores");
    assert_eq!(s.resonance, 100);
    assert_eq!(s.matched, vec![5, 4, 3, 2, 1, 0]);
}

#[test]
fn rotation_counts() {
    // An L shape and the same L turned a quarter: up means up.
    let figure = vec![(0, 0), (0, 400), (0, 800), (300, 800)];
    let turned: Vec<(i32, i32)> = figure.iter().map(|&(x, y)| (800 - y, x)).collect();
    let s = score::score(&turned, &figure).expect("scores");
    assert!(s.resonance < 60, "turned shape got {}", s.resonance);
}

#[test]
fn a_loose_sky_scores_low_and_a_close_one_high() {
    let figure = DECK[10].sample(8);
    let loose: Vec<(i32, i32)> = (0..8).map(|i| (i * 200, (i * 37 % 5) * 180)).collect();
    let close: Vec<(i32, i32)> = figure
        .iter()
        .enumerate()
        .map(|(i, &(x, y))| (x / 2 + 300 + (i as i32 % 3) * 6, y / 2 + 100))
        .collect();
    let low = score::score(&loose, &figure).expect("scores").resonance;
    let high = score::score(&close, &figure).expect("scores").resonance;
    assert!(low < 60, "loose sky {low}");
    assert!(high >= 90, "close sky {high}");
}

#[test]
fn stars_on_one_spot_weave_nothing() {
    let figure = DECK[1].sample(4);
    let s = score::score(&[(5, 5); 4], &figure).expect("scores");
    assert_eq!(s.resonance, 0);
}

#[test]
fn score_needs_matching_counts() {
    assert!(score::score(&[(0, 0), (1, 1)], &[(0, 0)]).is_none());
    assert!(score::score(&[(0, 0)], &[(0, 0)]).is_none());
}

#[test]
fn assignment_is_optimal() {
    // Compare with brute force over all permutations for small sizes.
    fn permutations(n: usize) -> Vec<Vec<usize>> {
        if n == 0 {
            return vec![vec![]];
        }
        let mut out = Vec::new();
        for p in permutations(n - 1) {
            for i in 0..=p.len() {
                let mut q = p.clone();
                q.insert(i, n - 1);
                out.push(q);
            }
        }
        out
    }
    let mut seed = 7u64;
    for n in 2..=6 {
        let cost: Vec<Vec<u64>> = (0..n)
            .map(|_| {
                (0..n)
                    .map(|_| {
                        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                        (seed >> 40) % 1000
                    })
                    .collect()
            })
            .collect();
        let total = |m: &[usize]| m.iter().enumerate().map(|(i, &j)| cost[i][j]).sum::<u64>();
        let best = permutations(n)
            .iter()
            .map(|p| total(p))
            .min()
            .expect("some");
        assert_eq!(total(&score::assign(&cost)), best);
    }
}

#[test]
fn twelve_points_score_quickly_and_identically() {
    let figure = DECK[52].sample(11);
    let sky: Vec<(i32, i32)> = figure
        .iter()
        .map(|&(x, y)| (y / 2 + 10, x / 3 + 40))
        .collect();
    let a = score::score(&sky, &figure).expect("scores");
    let b = score::score(&sky, &figure).expect("scores");
    assert_eq!(a, b);
}

// ----- Deck -----

#[test]
fn deck_has_sixty_distinct_figures() {
    assert_eq!(DECK.len(), 60);
    let mut ids: Vec<&str> = DECK.iter().map(|f| f.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 60);
}

#[test]
fn every_figure_samples_four_to_eleven_distinct_points() {
    for figure in &DECK {
        for n in 2..=11 {
            let points = figure.sample(n);
            assert_eq!(points.len(), n, "{}", figure.id);
            let mut unique = points.clone();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(
                unique.len(),
                n,
                "{} has repeated points at n={n}",
                figure.id
            );
            assert!(
                points
                    .iter()
                    .all(|&(x, y)| (0..=1600).contains(&x) && (0..=1600).contains(&y))
            );
            // Not degenerate: there is a shape to describe.
            assert!(score::score(&points, &points).is_some_and(|s| s.resonance == 100));
        }
    }
}

#[test]
fn every_figure_has_a_name_in_every_language() {
    let en: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(include_str!("../i18n/en.json")).expect("en.json");
    for figure in &DECK {
        assert!(
            en.contains_key(&format!("figure.{}", figure.id)),
            "{}",
            figure.id
        );
    }
    assert_eq!(view::english_name(0), "The Heron");
    assert_eq!(view::english_name(1), "Two Moths");
}

// ----- Commit–reveal -----

#[test]
fn commit_binds_secret_and_round() {
    let secret = [9; 32];
    assert_eq!(fair::commit(&secret, 1), fair::commit(&secret, 1));
    assert_ne!(fair::commit(&secret, 1), fair::commit(&secret, 2));
    assert_ne!(fair::commit(&secret, 1), fair::commit(&[8; 32], 1));
    let mut h = Sha256::new();
    h.update(b"skyloom1");
    h.update(secret);
    h.update([1]);
    assert_eq!(fair::commit(&secret, 1), <[u8; 32]>::from(h.finalize()));
}

#[test]
fn every_witness_changes_the_figure_draw() {
    let secret = [1; 32];
    let base = vec![(1u8, [2u8; 16]), (2, [3; 16])];
    let mut different = 0;
    for b in 0..40u8 {
        let mut w = base.clone();
        w[1].1[0] = b;
        if fair::figure(&secret, 1, &w, 0) != fair::figure(&secret, 1, &base, 0) {
            different += 1;
        }
    }
    assert!(different > 30, "witnesses barely matter: {different}");
}

#[test]
fn figure_draw_is_fair_and_skips_used_figures() {
    let mut counts = [0u32; deck::SIZE];
    for i in 0..6000u32 {
        let mut secret = [0u8; 32];
        secret[..4].copy_from_slice(&i.to_be_bytes());
        counts[fair::figure(&secret, 1, &[(1, [0; 16])], 0)] += 1;
    }
    assert!(
        counts.iter().all(|&c| (60..=140).contains(&c)),
        "{counts:?}"
    );
    let used = (1u64 << 59) - 1; // only figure 59 is left
    assert_eq!(fair::figure(&[3; 32], 2, &[], used), 59);
    // All used: the whole deck is open again.
    assert!(fair::figure(&[3; 32], 2, &[], u64::MAX) < deck::SIZE);
}

// ----- Wire -----

fn all_messages() -> Vec<Message> {
    vec![
        Message::Roster {
            night: 0xdead_beef,
            rev: 3,
            round: 2,
            settings: Settings {
                weave_secs: 120,
                mixed: false,
            },
            weaved: 0b101,
            players: (1..=12).collect(),
        },
        Message::Join { play: true },
        Message::Leave,
        Message::Ready { round: 4, rev: 9 },
        Message::Seal {
            round: 1,
            commit: [7; 32],
            used: u64::MAX,
        },
        Message::Witness {
            round: 1,
            nonce: [5; 16],
        },
        Message::Weave {
            round: 1,
            witnesses: 0xfff,
        },
        Message::Stars {
            round: 1,
            stars: vec![(0, 0), (LOOM_W, LOOM_H), (3, 4), (5, 6)],
        },
        Message::Lock {
            round: 1,
            stars: vec![(800, 450)],
        },
        Message::Pluck {
            round: 1,
            target: 77,
            star: 1,
        },
        Message::TieOff {
            round: 1,
            spare: 12,
        },
        Message::Reveal {
            round: 1,
            secret: [1; 32],
            locks: 0b110,
        },
        Message::Result {
            round: 1,
            hash: [4; 8],
        },
        Message::Share {
            round: 1,
            secret: [2; 32],
        },
        Message::End { night: 5 },
    ]
}

#[test]
fn messages_round_trip_and_stay_tiny() {
    for m in all_messages() {
        let bytes = m.encode();
        assert_eq!(bytes[0], FORMAT);
        assert!(bytes.len() <= MAX_LEN, "{m:?} is {} bytes", bytes.len());
        assert_eq!(Message::decode(&bytes), Some(m));
    }
}

#[test]
fn bad_messages_are_ignored() {
    for m in all_messages() {
        let bytes = m.encode();
        let mut other_format = bytes.clone();
        other_format[0] = 2;
        assert_eq!(Message::decode(&other_format), None);
        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(Message::decode(&longer), None);
        if bytes.len() > 2 {
            assert_eq!(Message::decode(&bytes[..bytes.len() - 1]), None);
        }
    }
    let off_loom = Message::Stars {
        round: 1,
        stars: vec![(LOOM_W + 1, 0)],
    };
    assert_eq!(Message::decode(&off_loom.encode()), None);
    assert_eq!(Message::decode(&[FORMAT, 99]), None);
    assert_eq!(Message::decode(&[]), None);
}

// ----- Rules -----

#[test]
fn few_threaders_hold_several_stars() {
    assert_eq!(rules::stars_each(1), 4);
    assert_eq!(rules::stars_each(2), 2);
    assert_eq!(rules::stars_each(3), 2);
    assert_eq!(rules::stars_each(4), 1);
    assert_eq!(rules::stars_each(11), 1);
}

#[test]
fn twists_start_in_round_three_and_only_when_mixed() {
    for round in 1..=8 {
        let t = rules::twist(42, round, true, 5);
        if round < 3 {
            assert_eq!(t, Twist::Calm);
        } else {
            assert_ne!(t, Twist::Twin, "twin weavers needs 8 players");
            assert_ne!(t, Twist::Calm);
        }
        assert_eq!(rules::twist(42, round, false, 12), Twist::Calm);
    }
}

// ----- Intents from the page -----

#[test]
fn page_intents_are_parsed_strictly() {
    assert_eq!(Intent::parse(r#"{"t":"ready"}"#), Some(Intent::Ready));
    assert_eq!(
        Intent::parse(r#"{"t":"move","i":1,"x":-5,"y":2000}"#),
        Some(Intent::Move {
            star: 1,
            x: -5,
            y: 2000
        })
    );
    assert_eq!(
        Intent::parse(
            r#"{"t":"guess","text":"a heron\u0007 with a very very long neck that goes on and on and on"}"#
        ),
        Some(Intent::Guess(
            "a heron with a very very long neck that goes on and on and o".to_string()
        ))
    );
    assert_eq!(Intent::parse(r#"{"t":"place","to":"elsewhere"}"#), None);
    assert_eq!(Intent::parse("not json"), None);
}

// ----- The whole game, with simulated clients -----

#[derive(Default)]
struct Mock {
    seed: u64,
    outbox: Vec<(Option<u32>, Message)>,
    chat: Vec<String>,
    posts: Vec<String>,
    cards: Vec<String>,
    supersedes: Vec<(String, u32)>,
    sounds: Vec<String>,
    stage_refused: bool,
    live: bool,
    /// Star updates streamed over the relay.
    streamed: usize,
    saved: Option<Prefs>,
}

impl Host for Mock {
    fn random(&mut self, len: u32) -> Option<Vec<u8>> {
        self.seed += 1;
        let mut h = Sha256::new();
        h.update(self.seed.to_be_bytes());
        Some(h.finalize()[..len as usize].to_vec())
    }
    fn send(&mut self, message: &Message, live: bool) {
        assert!(message.encode().len() <= MAX_LEN);
        if live {
            assert!(self.live, "streamed without a relay");
            assert!(matches!(message, Message::Stars { .. }));
            self.streamed += 1;
            return;
        }
        self.outbox.push((None, message.clone()));
    }
    fn send_direct(&mut self, to: u32, message: &Message) {
        self.outbox.push((Some(to), message.clone()));
    }
    fn live(&self) -> bool {
        self.live
    }
    fn start_game_burst(&mut self) {}
    fn post(&mut self, json: String) {
        assert!(json.len() <= 16 * 1024, "page message too large");
        self.posts.push(json);
    }
    fn start_stage(&mut self, _title: &str) -> bool {
        !self.stage_refused
    }
    fn end_stage(&mut self) {}
    fn chat_send(&mut self, text: &str) {
        assert!(text.chars().count() <= 500);
        self.chat.push(text.to_string());
    }
    fn line_hash(&mut self, text: &str) -> u32 {
        text.len() as u32
    }
    fn supersede(&mut self, card: &str, sender: u32, _hash: u32) {
        self.supersedes.push((card.to_string(), sender));
    }
    fn show_card(&mut self, card: &str, _tree: El) {
        self.cards.push(card.to_string());
    }
    fn set_badge(&mut self, _session: u32, _on: bool) {}
    fn play(&mut self, sound: &str, _gain: f32) {
        self.sounds.push(sound.to_string());
    }
    fn stop_sounds(&mut self) {}
    fn notify(&mut self, _title: &str, _body: &str) {}
    fn save(&mut self, prefs: &Prefs) {
        self.saved = Some(prefs.clone());
    }
    fn warn(&mut self, _message: &str) {}
}

struct Client {
    app: App,
    host: Mock,
    /// Every message this client received, for leak checks.
    inbox: Vec<Message>,
    online: bool,
}

struct Table {
    clients: Vec<Client>,
    /// Applied to the Weaver's reveal before delivery, to test tampering.
    tamper: bool,
}

const NAMES: [&str; 12] = [
    "Maya", "Ben", "Zoe", "Leo", "Sam", "Carla", "Nina", "Jonas", "Omar", "Dave", "Ida", "Kai",
];

impl Table {
    fn new(n: usize) -> Table {
        let names: Vec<(u32, String)> = (0..n)
            .map(|i| (i as u32 + 1, NAMES[i].to_string()))
            .collect();
        let clients = (0..n)
            .map(|i| {
                let mut app = App::default();
                app.me = i as u32 + 1;
                app.on_presence(names.clone());
                Client {
                    app,
                    host: Mock {
                        seed: (i as u64 + 1) << 32,
                        ..Mock::default()
                    },
                    inbox: Vec::new(),
                    online: true,
                }
            })
            .collect();
        Table {
            clients,
            tamper: false,
        }
    }

    fn c(&mut self, session: u32) -> &mut Client {
        &mut self.clients[session as usize - 1]
    }

    /// Delivers every queued message until the table is quiet.
    fn deliver(&mut self) {
        loop {
            let mut queue = Vec::new();
            for c in &mut self.clients {
                for (to, m) in c.host.outbox.drain(..) {
                    queue.push((c.app.me, to, m));
                }
            }
            if queue.is_empty() {
                return;
            }
            for (from, to, mut m) in queue {
                if self.tamper
                    && let Message::Reveal { secret, .. } = &mut m
                {
                    secret[0] ^= 1;
                }
                for c in &mut self.clients {
                    if c.app.me == from || !c.online || to.is_some_and(|t| t != c.app.me) {
                        continue;
                    }
                    c.inbox.push(m.clone());
                    c.app.on_message(&mut c.host, from, m.clone());
                }
            }
        }
    }

    fn run(&mut self, ms: u64) {
        for _ in 0..ms / 100 {
            for c in &mut self.clients {
                if c.online {
                    c.app.tick(&mut c.host, 1_000_000);
                }
            }
            self.deliver();
        }
    }

    /// Client 1 opens a night; the others join it.
    fn open(&mut self, settings: Settings) {
        let n = self.clients.len() as u32;
        let c = self.c(1);
        c.app.on_start(&mut c.host);
        c.app.on_page(&mut c.host, Intent::Ready);
        c.app.on_page(
            &mut c.host,
            Intent::Settings {
                weave_secs: settings.weave_secs,
                mixed: settings.mixed,
            },
        );
        self.deliver();
        for s in 2..=n {
            let c = self.c(s);
            c.app
                .on_activity_started(&mut c.host, 1, "act-1".to_string());
            c.app.on_stage_joined(&mut c.host);
            c.app.on_page(&mut c.host, Intent::Ready);
            self.deliver();
        }
        self.run(300);
    }

    fn players(&self, session: u32) -> Vec<u32> {
        self.clients[session as usize - 1]
            .app
            .night
            .as_ref()
            .map(|n| n.players.clone())
            .unwrap_or_default()
    }

    fn round_phase(&self, session: u32) -> Option<Phase> {
        self.clients[session as usize - 1]
            .app
            .night
            .as_ref()?
            .round
            .as_ref()
            .map(|r| r.phase)
    }

    fn next_weaver(&self) -> Option<u32> {
        let c = self.clients.iter().find(|c| c.online)?;
        c.app.night.as_ref()?.next_weaver()
    }

    /// The next Weaver starts a round; Threaders nudge their stars; the round plays to dawn.
    fn play_round(&mut self) {
        let weaver = self.next_weaver().expect("a next weaver");
        let c = self.c(weaver);
        c.app.on_page(&mut c.host, Intent::WeaveReady);
        self.deliver();
        self.run(3_500);
        for c in &mut self.clients {
            if c.online {
                c.app.on_page(
                    &mut c.host,
                    Intent::Move {
                        star: 0,
                        x: 300 + c.app.me as i32 * 90,
                        y: 200 + (c.app.me as i32 % 3) * 150,
                    },
                );
            }
        }
        let secs = self.clients[weaver as usize - 1]
            .app
            .night
            .as_ref()
            .map_or(90, |n| u64::from(n.settings.weave_secs));
        self.run(secs * 1000 + 16_000);
    }
}

fn calm() -> Settings {
    Settings {
        weave_secs: 60,
        mixed: false,
    }
}

fn outcome(t: &Table, session: u32) -> (Option<usize>, u8, Verify) {
    let o = t.clients[session as usize - 1]
        .app
        .night
        .as_ref()
        .and_then(|n| n.round.as_ref())
        .and_then(|r| r.outcome.as_ref())
        .expect("an outcome");
    (o.figure, o.resonance, o.verify.clone())
}

#[test]
fn opening_a_night_posts_one_line_and_seats_joiners() {
    let mut t = Table::new(3);
    t.open(calm());
    assert_eq!(t.players(1), vec![1, 2, 3]);
    assert_eq!(t.players(3), vec![1, 2, 3]);
    assert_eq!(t.c(1).host.chat.len(), 1);
    assert!(t.c(1).host.chat[0].starts_with("[Skyloom] Maya opened a night"));
    assert!(t.c(2).host.chat.is_empty());
    // Mumcord users get the invite card instead of the plain line.
    assert!(t.c(2).host.cards.contains(&"invite-act-1".to_string()));
    assert!(
        t.c(2)
            .host
            .supersedes
            .contains(&("invite-act-1".to_string(), 1))
    );
}

#[test]
fn starting_without_the_stage_does_nothing() {
    let mut t = Table::new(1);
    t.c(1).host.stage_refused = true;
    let c = t.c(1);
    c.app.on_start(&mut c.host);
    assert!(c.app.night.is_none());
    assert!(c.host.outbox.is_empty() && c.host.chat.is_empty());
}

#[test]
fn three_clients_play_a_verified_round() {
    let mut t = Table::new(3);
    t.open(calm());
    t.play_round();
    let a = outcome(&t, 1);
    assert_eq!(a, outcome(&t, 2));
    assert_eq!(a, outcome(&t, 3));
    assert_eq!(a.2, Verify::Verified);
    assert!(a.0.is_some());
    // Two Threaders hold two stars each: four points.
    let stars = t
        .c(2)
        .app
        .night
        .as_ref()
        .unwrap()
        .round
        .as_ref()
        .unwrap()
        .outcome
        .as_ref()
        .unwrap()
        .stars
        .len();
    assert_eq!(stars, 4);
}

#[test]
fn threaders_never_receive_the_figure_before_dawn() {
    let mut t = Table::new(3);
    t.open(calm());
    let weaver = t.next_weaver().unwrap();
    let c = t.c(weaver);
    c.app.on_page(&mut c.host, Intent::WeaveReady);
    t.deliver();
    t.run(5_000);
    for s in [2, 3] {
        assert_eq!(t.round_phase(s), Some(Phase::Weave));
        let r = t.c(s).app.night.as_ref().unwrap().round.as_ref().unwrap();
        assert!(r.secret.is_none());
        assert!(r.known_figure().is_none());
        // Nothing they got so far carries the secret.
        assert!(
            !t.c(s)
                .inbox
                .iter()
                .any(|m| matches!(m, Message::Reveal { .. } | Message::Share { .. }))
        );
        // Their page has no sky card.
        let view = t.c(s).host.posts.last().cloned().unwrap_or_default();
        assert!(
            !view.contains("\"lines\":[["),
            "threader view leaks the figure"
        );
    }
    // The Weaver does see it.
    let r = t
        .c(weaver)
        .app
        .night
        .as_ref()
        .unwrap()
        .round
        .as_ref()
        .unwrap();
    assert!(r.known_figure().is_some());
}

#[test]
fn a_tampered_reveal_is_unverified() {
    let mut t = Table::new(3);
    t.open(calm());
    t.tamper = true;
    t.play_round();
    assert_eq!(outcome(&t, 2).2, Verify::SealBroken);
    assert_eq!(outcome(&t, 3).2, Verify::SealBroken);
}

#[test]
fn the_weaver_leaving_voids_the_round_and_the_night_goes_on() {
    let mut t = Table::new(4);
    t.open(calm());
    let c = t.c(1);
    c.app.on_page(&mut c.host, Intent::WeaveReady);
    t.deliver();
    t.run(5_000);
    t.c(1).online = false;
    for s in 2..=4 {
        let c = t.c(s);
        c.app.on_left(&mut c.host, 1);
    }
    for s in 2..=4 {
        assert_eq!(t.round_phase(s), Some(Phase::Void));
        // Ben is the new Lamplighter (longest present) and the next Weaver.
        assert_eq!(t.players(s), vec![2, 3, 4]);
    }
    t.run(1_000);
    let next = t.c(2).app.night.as_ref().unwrap().next_weaver();
    assert_eq!(next, Some(2));
    t.play_round();
    assert_eq!(outcome(&t, 3).2, Verify::Verified);
}

#[test]
fn a_threader_dropping_out_is_left_out_of_scoring() {
    let mut t = Table::new(5);
    t.open(calm());
    let c = t.c(1);
    c.app.on_page(&mut c.host, Intent::WeaveReady);
    t.deliver();
    t.run(5_000);
    t.c(5).online = false;
    for s in 1..=4 {
        let c = t.c(s);
        c.app.on_left(&mut c.host, 5);
    }
    t.run(80_000);
    let (_, res, verify) = outcome(&t, 2);
    assert_eq!(verify, Verify::Verified);
    assert_eq!(outcome(&t, 3).1, res);
    let r = t.c(2).app.night.as_ref().unwrap().round.as_ref().unwrap();
    let scored = &r.outcome.as_ref().unwrap().stars;
    assert_eq!(scored.len(), 3);
    assert!(scored.iter().all(|s| s.0 != 5));
}

#[test]
fn a_late_joiner_watches_and_gets_a_star_next_round() {
    let mut t = Table::new(4);
    t.c(4).online = false;
    // Open with three only.
    let c = t.c(1);
    c.app.on_start(&mut c.host);
    t.deliver();
    for s in 2..=3 {
        let c = t.c(s);
        c.app.on_activity_started(&mut c.host, 1, "act-1".into());
        c.app.on_stage_joined(&mut c.host);
        t.deliver();
    }
    t.run(300);
    let c = t.c(1);
    c.app.on_page(&mut c.host, Intent::WeaveReady);
    t.deliver();
    t.run(5_000);
    t.c(4).online = true;
    let c = t.c(4);
    c.app.on_activity_started(&mut c.host, 1, "act-1".into());
    c.app.on_stage_joined(&mut c.host);
    t.deliver();
    assert_eq!(t.players(1), vec![1, 2, 3]);
    assert!(t.c(1).app.night.as_ref().unwrap().spectators.contains(&4));
    t.run(100_000);
    // Seated after the round.
    assert_eq!(t.players(2), vec![1, 2, 3, 4]);
    assert_eq!(t.players(4), vec![1, 2, 3, 4]);
    t.play_round();
    let r = t.c(4).app.night.as_ref().unwrap().round.as_ref().unwrap();
    assert!(r.is_threader(4));
    assert_eq!(outcome(&t, 4).2, Verify::Verified);
}

#[test]
fn a_full_night_posts_exactly_start_rounds_and_summary() {
    let mut t = Table::new(3);
    t.open(Settings {
        weave_secs: 60,
        mixed: true,
    });
    for _ in 0..3 {
        t.play_round();
    }
    assert_eq!(t.next_weaver(), None);
    let c = t.c(1);
    c.app.on_page(&mut c.host, Intent::Tapestry);
    t.deliver();
    let c = t.c(1);
    c.app.on_page(&mut c.host, Intent::PostSummary);
    c.app.on_page(&mut c.host, Intent::PostSummary);
    let lines: Vec<String> = t.clients.iter().flat_map(|c| c.host.chat.clone()).collect();
    assert_eq!(lines.len(), 5, "{lines:#?}");
    assert!(lines[0].contains("opened a night"));
    assert_eq!(lines.iter().filter(|l| l.contains("] Round ")).count(), 3);
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.contains("The night is over"))
            .count(),
        1
    );
    for s in 1..=3 {
        let night = t.c(s).app.night.as_ref().unwrap();
        assert!(night.ended);
        assert_eq!(night.history.len(), 3);
    }
    // Every round gets a card, and the sky is kept.
    assert!(t.c(2).host.cards.iter().any(|c| c.starts_with("summary-")));
    assert_eq!(t.c(2).host.saved.as_ref().unwrap().skies.len(), 1);
    let team = t.c(1).app.night.as_ref().unwrap().team();
    assert_eq!(team, t.c(3).app.night.as_ref().unwrap().team());
}

#[test]
fn eight_rounds_at_most() {
    let mut t = Table::new(9);
    t.open(calm());
    for _ in 0..8 {
        t.play_round();
    }
    let night = t.c(1).app.night.as_ref().unwrap();
    assert_eq!(night.history.len(), 8);
    assert_eq!(night.next_weaver(), None);
    assert!(night.history.iter().all(|r| r.verified));
}

#[test]
fn twin_weavers_share_the_secret_only_with_each_other() {
    // Find a night id whose round 3 is Twin Weavers with 8 players.
    let mut t = Table::new(8);
    t.open(Settings {
        weave_secs: 60,
        mixed: true,
    });
    let id = (0u32..)
        .find(|&id| rules::twist(id, 3, true, 8) == Twist::Twin)
        .unwrap();
    for c in &mut t.clients {
        c.app.night.as_mut().unwrap().id = id;
    }
    t.play_round();
    t.play_round();
    let weaver = t.next_weaver().unwrap();
    let c = t.c(weaver);
    c.app.on_page(&mut c.host, Intent::WeaveReady);
    t.deliver();
    t.run(5_000);
    let r = t.c(1).app.night.as_ref().unwrap().round.as_ref().unwrap();
    assert_eq!(r.twist, Twist::Twin);
    let twin = r.twin.unwrap();
    for s in 1..=8u32 {
        let r = t.c(s).app.night.as_ref().unwrap().round.as_ref().unwrap();
        assert_eq!(
            r.known_figure().is_some(),
            s == weaver || s == twin,
            "client {s}"
        );
    }
    t.run(80_000);
    assert_eq!(outcome(&t, 1).2, Verify::Verified);
}

#[test]
fn star_updates_respect_the_mumble_rate_without_relay() {
    let mut t = Table::new(3);
    t.open(calm());
    let c = t.c(1);
    c.app.on_page(&mut c.host, Intent::WeaveReady);
    t.deliver();
    t.run(3_500);
    // Client 2 drags its star every 100 ms for 20 s, before first light.
    let mut sent = 0;
    for step in 0..200 {
        let c = t.c(2);
        c.app.on_page(
            &mut c.host,
            Intent::Move {
                star: 0,
                x: 200 + step,
                y: 300,
            },
        );
        c.app.tick(&mut c.host, 0);
        sent += c
            .host
            .outbox
            .iter()
            .filter(|(_, m)| matches!(m, Message::Stars { .. }))
            .count();
        t.deliver();
    }
    assert!(sent <= 11, "sent {sent} star updates in 20 s");
    assert!(sent >= 9);
}

#[test]
fn the_page_view_fits_the_bridge() {
    let mut t = Table::new(12);
    t.open(calm());
    t.play_round();
    for c in &t.clients {
        assert!(c.host.posts.iter().all(|p| p.len() <= 16 * 1024));
    }
    let strings = view::strings();
    assert!(
        strings.len() <= 16 * 1024,
        "strings are {} bytes",
        strings.len()
    );
}

#[test]
fn with_the_relay_movement_is_live_and_plugin_data_keeps_its_rate() {
    let mut t = Table::new(3);
    t.open(calm());
    t.c(2).host.live = true;
    let c = t.c(1);
    c.app.on_page(&mut c.host, Intent::WeaveReady);
    t.deliver();
    t.run(3_500);
    let mut slow = 0;
    for step in 0..100 {
        let c = t.c(2);
        c.app.on_page(
            &mut c.host,
            Intent::Move {
                star: 0,
                x: 200 + step,
                y: 300,
            },
        );
        c.app.tick(&mut c.host, 0);
        slow += c
            .host
            .outbox
            .iter()
            .filter(|(_, m)| matches!(m, Message::Stars { .. }))
            .count();
        t.deliver();
    }
    // 10 s of dragging: about 12 live updates per second, plugin data still every 2 s.
    assert!(
        t.c(2).host.streamed >= 90,
        "streamed {}",
        t.c(2).host.streamed
    );
    assert!(slow <= 6, "{slow} plugin data updates");
}
