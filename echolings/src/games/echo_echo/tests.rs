use mumcord_plugin_sdk::ui::{Animation, ButtonKind, Component, Node};

use super::*;
use crate::storage::AnimationLevel;
use crate::ui::{Motion, i18n::I18n};

const T0: u64 = 1_793_577_600;

const FULL: Motion = Motion {
    level: AnimationLevel::Full,
    reduced: false,
};

fn tune5() -> Vec<u8> {
    vec![0, 3, 1, 1, 2]
}

#[test]
fn timing_constants_agree() {
    assert_eq!(u64::from(GAME_MS), GAME_SECS * 1000);
    assert_eq!(u64::from(LOBBY_MS), LOBBY_SECS * 1000);
}

/// A game the user hosts whose lobby already closed.
fn started(id: u32) -> Game {
    let mut game = Game::open(id, tune5(), T0);
    assert!(game.begin(T0));
    game
}

#[test]
fn tunes_are_valid_and_vary() {
    let a = tune(T0);
    assert_eq!(a.len(), TUNE_LEN);
    assert!(a.iter().all(|n| *n < PADS));
    assert_ne!(a, tune(T0 + 1));
    assert_eq!(tune(0).len(), TUNE_LEN);
}

#[test]
fn messages_round_trip_and_reject_garbage() {
    let invite = Invite {
        id: 0xdead_beef,
        tune: tune5(),
        lobby: 30,
    };
    assert_eq!(Invite::decode(&invite.encode()), Some(invite.clone()));
    // An invite from before the lobby has no lobby byte: the round already started.
    let old = &invite.encode()[..11];
    assert_eq!(Invite::decode(old).map(|i| i.lobby), Some(0));
    // A peer cannot hold everyone in a longer lobby than ours.
    let mut greedy = invite.encode();
    greedy[11] = 255;
    assert_eq!(Invite::decode(&greedy).map(|i| i.lobby), Some(30));
    let mut bad_note = invite.encode();
    bad_note[6] = PADS;
    assert_eq!(Invite::decode(&bad_note), None);
    let short = Invite {
        id: 1,
        tune: vec![0, 1],
        lobby: 0,
    };
    assert_eq!(Invite::decode(&short.encode()), None);
    let long = Invite {
        id: 1,
        tune: vec![0; MAX_TUNE + 1],
        lobby: 0,
    };
    assert_eq!(Invite::decode(&long.encode()), None);
    assert_eq!(Invite::decode(&invite.encode()[..7]), None, "truncated");
    assert_eq!(Invite::decode(&[]), None);
    assert_eq!(
        Invite::decode(&[0, 0, 0, 0, 1, 3, 0, 0, 0]),
        None,
        "version 0"
    );

    let start = Start { id: 9 };
    assert_eq!(Start::decode(&start.encode()), Some(start));
    assert_eq!(Start::decode(&[1, 0, 0]), None);
    assert_eq!(Start::decode(&[0, 0, 0, 0, 9]), None, "version 0");

    let score = Score { id: 9, correct: 4 };
    assert_eq!(Score::decode(&score.encode()), Some(score));
    assert_eq!(Score::decode(&[1, 0, 0]), None);
    assert!(invite.encode().len() <= 997 && score.encode().len() <= 997);
}

#[test]
fn pad_actions_round_trip() {
    assert_eq!(action::parse_pad(&action::pad(3)), Some(3));
    assert_eq!(action::parse_pad(&action::pad(PADS)), None);
    assert_eq!(action::parse_pad("echo-pad:-1"), None);
}

/// Two clients: A starts, B joins; both repeat; each sees the other's score.
#[test]
fn two_players_play_a_round() {
    let mut a = Game::open(7, tune5(), T0);
    let invite = Invite::decode(
        &Invite {
            id: a.id,
            tune: a.tune.clone(),
            lobby: 30,
        }
        .encode(),
    )
    .unwrap();
    let mut b = Game::invited(invite, 1, "Ana".into(), T0);
    assert_eq!(b.phase, Phase::Invited);
    assert_eq!(b.press(0), None, "not joined");
    assert!(b.join());
    assert!(!b.join());
    assert_eq!(b.phase, Phase::Waiting, "joined into the lobby");
    assert_eq!(b.playing(), None);

    // The host's lobby closes; B starts when the start message arrives.
    assert!(a.begin(T0 + 30));
    let start = Start::decode(&Start { id: 7 }.encode()).unwrap();
    assert!(b.start_received(1, start, T0 + 30));
    assert!(!b.start_received(1, start, T0 + 31), "only once");

    // The tune plays one note per step, then it is the player's turn.
    for game in [&mut a, &mut b] {
        let mut heard = vec![game.playing().unwrap()];
        while game.step() {
            heard.push(game.playing().unwrap());
        }
        assert_eq!(heard, tune5());
        assert_eq!(game.phase, Phase::Repeat { entered: 0 });
        assert_eq!(game.playing(), None);
    }

    // A repeats it all; B slips on the third note.
    let mut a_score = None;
    for note in tune5() {
        a_score = a.press(note);
    }
    assert_eq!(a_score, Some(5));
    assert_eq!(b.press(0), None);
    assert_eq!(b.press(3), None);
    assert_eq!(b.press(2), Some(2));
    assert_eq!(b.press(1), None, "turn is over");

    let wire = |s: u8, id| Score::decode(&Score { id, correct: s }.encode()).unwrap();
    assert!(a.score_received(2, "Bo".into(), wire(2, 7)));
    assert!(
        !a.score_received(2, "Bo".into(), wire(5, 7)),
        "first score counts"
    );
    assert!(!a.score_received(3, "Cy".into(), wire(5, 8)), "other game");
    assert!(b.score_received(1, "Ana".into(), wire(200, 7)));
    assert_eq!(b.scores[&1].correct, 5, "clamped to the tune");

    a.end();
    b.end();
    assert!(
        !a.score_received(4, "Late".into(), wire(5, 7)),
        "after the end"
    );
    assert_eq!(a.results(), vec![(None, 5), (Some("Bo"), 2)]);
    assert_eq!(b.results(), vec![(Some("Ana"), 5), (None, 2)]);
}

/// The lobby: nobody but the host may start the round, late joiners play at once, and a round
/// whose host never sent the start is given up.
#[test]
fn lobby_starts_on_the_hosts_message() {
    let mut host = Game::open(3, tune5(), T0);
    assert_eq!(host.phase, Phase::Waiting);
    assert_eq!(host.starts_at, Some(T0 + LOBBY_SECS));
    // Nobody joined: the host plays alone.
    assert!(host.begin(T0 + LOBBY_SECS));
    assert!(!host.begin(T0 + LOBBY_SECS), "only once");
    assert_eq!(host.phase, Phase::Listen { step: 0 });
    assert_eq!(host.ends_at, T0 + LOBBY_SECS + GAME_SECS);

    let invite = |lobby| Invite {
        id: 3,
        tune: tune5(),
        lobby,
    };
    let mut joined = Game::invited(invite(30), 1, "Ana".into(), T0);
    assert_eq!(joined.ends_at, T0 + 30 + NO_START_GRACE_SECS);
    joined.join();
    assert!(
        !joined.start_received(2, Start { id: 3 }, T0 + 5),
        "not the host"
    );
    assert!(
        !joined.start_received(1, Start { id: 4 }, T0 + 5),
        "other game"
    );
    assert_eq!(joined.phase, Phase::Waiting);
    assert!(
        joined.start_received(1, Start { id: 3 }, T0 + 5),
        "Start now"
    );
    assert_eq!(joined.phase, Phase::Listen { step: 0 });
    assert_eq!(joined.ends_at, T0 + 5 + GAME_SECS);

    // Someone who did not join in the lobby can still join the running round.
    let mut late = Game::invited(invite(30), 1, "Ana".into(), T0);
    assert!(late.start_received(1, Start { id: 3 }, T0 + 30));
    assert_eq!(late.phase, Phase::Invited);
    late.join();
    assert_eq!(late.phase, Phase::Listen { step: 0 });

    // An invite without a lobby has already started.
    let mut old = Game::invited(invite(0), 1, "Ana".into(), T0);
    assert_eq!((old.starts_at, old.ends_at), (None, T0 + GAME_SECS));
    old.join();
    assert_eq!(old.phase, Phase::Listen { step: 0 });

    // The host left: no start arrives and the round is given up.
    let mut orphan = Game::invited(invite(30), 1, "Ana".into(), T0);
    orphan.join();
    orphan.end();
    assert!(orphan.never_started());
    assert!(
        !orphan.start_received(1, Start { id: 3 }, T0 + 50),
        "too late"
    );
    assert!(!host.never_started());
}

fn texts(nodes: &[Node]) -> Vec<String> {
    nodes
        .iter()
        .filter_map(|n| match &n.component {
            Component::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect()
}

fn pads(nodes: &[Node]) -> Vec<(String, bool, ButtonKind, Option<Animation>)> {
    nodes
        .iter()
        .filter_map(|n| match &n.component {
            Component::Button(b) if b.action.starts_with("echo-pad:") => {
                Some((b.label.clone(), b.disabled, b.kind, n.animation))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn card_shows_each_phase() {
    let t = I18n::new("en");
    let view = |game: &Game, motion| {
        GameView {
            game,
            t: &t,
            motion,
        }
        .card()
        .into_nodes()
    };

    let mut game = Game::invited(
        Invite {
            id: 1,
            tune: tune5(),
            lobby: 30,
        },
        3,
        "Ana".into(),
        T0,
    );
    let has = |nodes: &[Node], action: &str| {
        nodes.iter().any(|n| {
            matches!(&n.component,
            Component::Button(b) if b.action == action)
        })
    };
    let countdown = |nodes: &[Node]| {
        nodes.iter().find_map(|n| match &n.component {
            Component::Countdown(c) => Some((c.label.clone(), c.until)),
            _ => None,
        })
    };
    let nodes = view(&game, FULL);
    assert!(texts(&nodes)[1].starts_with("Ana started Echo Echo"));
    assert!(has(&nodes, action::JOIN));
    assert_eq!(countdown(&nodes), Some(("Starts in".into(), T0 + 30)));

    game.join();
    let nodes = view(&game, FULL);
    assert_eq!(texts(&nodes)[1], "You're in. The tune starts soon…");
    assert!(!has(&nodes, action::START_NOW), "only the host starts");
    assert_eq!(countdown(&nodes), Some(("Starts in".into(), T0 + 30)));

    let host = Game::open(5, tune5(), T0);
    let nodes = view(&host, FULL);
    assert!(texts(&nodes)[1].starts_with("You started Echo Echo"));
    assert!(has(&nodes, action::START_NOW));
    assert_eq!(
        countdown(&nodes),
        Some(("Starts in".into(), T0 + LOBBY_SECS))
    );

    game.start_received(3, Start { id: 1 }, T0 + 30);
    let nodes = view(&game, FULL);
    assert_eq!(texts(&nodes)[1], "Listen… note 1 of 5");
    let p = pads(&nodes);
    assert_eq!(p.len(), 4);
    assert!(p.iter().all(|(_, disabled, _, _)| *disabled));
    assert_eq!(p[0].0, "♪ 1");
    assert_eq!(
        (p[0].2, p[0].3),
        (ButtonKind::Primary, Some(Animation::Hop))
    );
    assert_eq!(p[1].2, ButtonKind::Secondary);
    // Reduced motion keeps the highlight but drops the hop.
    let still = Motion {
        level: AnimationLevel::Still,
        reduced: false,
    };
    let p = pads(&view(&game, still));
    assert_eq!((p[0].2, p[0].3), (ButtonKind::Primary, None));

    while game.step() {}
    let nodes = view(&game, FULL);
    assert_eq!(texts(&nodes)[1], "Your turn: 0 of 5 notes");
    assert!(pads(&nodes).iter().all(|(_, disabled, _, _)| !disabled));

    game.press(1);
    assert!(texts(&view(&game, FULL))[1].starts_with("You echoed 0 of 5"));

    game.end();
    let nodes = view(&game, FULL);
    assert_eq!(texts(&nodes)[1], "You 0/5");
    assert!(nodes.iter().any(|n| matches!(&n.component,
        Component::Button(b) if b.action == action::CLOSE)));

    let mut empty = started(2);
    empty.end();
    assert_eq!(texts(&view(&empty, FULL))[1], "Nobody finished this round.");

    let mut orphan = Game::open(4, tune5(), T0);
    orphan.end();
    assert_eq!(texts(&view(&orphan, FULL))[1], "This round did not start.");
    assert!(t.missing().is_empty(), "{:?}", t.missing());
}
