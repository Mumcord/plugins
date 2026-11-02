use super::page::{self, FromPage, Look, PageRunner};
use super::*;
use crate::storage::AnimationLevel;
use crate::ui::{Motion, i18n::I18n};

const T0: u64 = 1_793_577_600;

#[test]
fn timing_constants_agree() {
    assert_eq!(u64::from(RESULTS_MS), RESULTS_SECS * 1000);
    assert_eq!(u64::from(LOBBY_MS), LOBBY_SECS * 1000);
    // The results wait for every run to end.
    assert!(u64::from(RACE_MS) < RESULTS_SECS * 1000);
}

/// A race the user hosts whose lobby already closed.
fn started(id: u32) -> Race {
    let mut race = Race::open(id, T0);
    assert!(race.begin(T0));
    race
}

#[test]
fn messages_round_trip_and_reject_garbage() {
    let invite = Invite {
        id: 0xdead_beef,
        lobby: 30,
    };
    assert_eq!(Invite::decode(&invite.encode()), Some(invite));
    let mut greedy = invite.encode();
    greedy[5] = 255;
    assert_eq!(Invite::decode(&greedy).map(|i| i.lobby), Some(30));
    assert_eq!(Invite::decode(&invite.encode()[..5]), None);
    assert_eq!(Invite::decode(&[0, 0, 0, 0, 1, 30]), None);

    let start = Start { id: 9 };
    assert_eq!(Start::decode(&start.encode()), Some(start));
    assert_eq!(Start::decode(&[1, 0, 0]), None);

    let progress = Progress { id: 9, hops: 4 };
    assert_eq!(Progress::decode(&progress.encode()), Some(progress));
    assert_eq!(Progress::decode(&[1, 0, 0, 0, 9, GOAL + 1]), None);

    let score = Score {
        id: 9,
        hops: GOAL,
        ms: 6_400,
    };
    assert_eq!(Score::decode(&score.encode()), Some(score));
    let unfinished = Score {
        id: 9,
        hops: 3,
        ms: 0,
    };
    assert_eq!(Score::decode(&unfinished.encode()), Some(unfinished));
    // A finish faster than possible, or slower than the race, is a forgery.
    let fast = Score {
        id: 9,
        hops: GOAL,
        ms: MIN_FINISH_MS - 1,
    };
    assert_eq!(Score::decode(&fast.encode()), None);
    let slow = Score {
        id: 9,
        hops: GOAL,
        ms: 15_001,
    };
    assert_eq!(Score::decode(&slow.encode()), None);
    assert_eq!(Score::decode(&[1, 0, 0, 0, 9, GOAL + 1, 0, 0]), None);
    assert_eq!(Score::decode(&[1, 0, 0, 0, 9, 3]), None);
}

#[test]
fn courses_stay_on_the_bar_and_vary() {
    let a = course(7);
    assert_eq!(a.len(), usize::from(GOAL));
    assert!(a.iter().all(|z| (ZONE_MIN..=ZONE_MAX).contains(z)));
    assert_eq!(a, course(7));
    assert_ne!(a, course(8));
}

#[test]
fn the_host_starts_the_race_for_everyone_who_joined() {
    let mut race = Race::open(5, T0);
    assert_eq!(race.phase, Phase::Waiting);
    assert_eq!(race.starts_at, Some(T0 + LOBBY_SECS));
    assert!(race.begin(T0 + 3));
    assert_eq!(race.phase, Phase::Racing);
    assert_eq!(race.ends_at, T0 + 3 + RESULTS_SECS);
    assert!(!race.begin(T0 + 4));

    let invite = Invite { id: 5, lobby: 30 };
    let mut guest = Race::invited(invite, 2, "Bob".into(), T0);
    assert_eq!(guest.phase, Phase::Invited);
    assert!(guest.join());
    assert_eq!(guest.phase, Phase::Waiting);
    // Only the host's start for this race counts.
    assert!(!guest.start_received(3, Start { id: 5 }, T0 + 2));
    assert!(!guest.start_received(2, Start { id: 6 }, T0 + 2));
    assert!(guest.start_received(2, Start { id: 5 }, T0 + 2));
    assert_eq!(guest.phase, Phase::Racing);
    assert!(!guest.start_received(2, Start { id: 5 }, T0 + 3));
}

#[test]
fn an_invite_without_a_lobby_has_started_and_a_lost_start_gives_up() {
    let mut late = Race::invited(Invite { id: 5, lobby: 0 }, 2, "Bob".into(), T0);
    assert!(late.join());
    assert_eq!(late.phase, Phase::Racing);

    let mut orphan = Race::invited(Invite { id: 5, lobby: 30 }, 2, "Bob".into(), T0);
    assert_eq!(orphan.ends_at, T0 + 30 + NO_START_GRACE_SECS);
    orphan.end();
    assert!(orphan.never_started());
}

#[test]
fn hops_count_one_at_a_time_and_the_snack_ends_the_run() {
    let mut race = started(5);
    assert_eq!(race.hop(2, 300, T0), None);
    assert_eq!(race.hops, 0);
    for hop in 1..GOAL {
        assert_eq!(race.hop(hop, 0, T0), None);
    }
    assert_eq!(race.hop(GOAL - 1, 0, T0), None);
    let score = race.hop(GOAL, 6_400, T0 + 6);
    assert_eq!(
        score,
        Some(Score {
            id: 5,
            hops: GOAL,
            ms: 6_400
        })
    );
    assert_eq!(race.phase, Phase::Finished);
    assert_eq!(race.hop(GOAL + 1, 7_000, T0 + 7), None);
    assert_eq!(race.time_up(T0 + 15), None);
}

#[test]
fn a_finish_time_never_beats_the_clock() {
    // The page was reopened mid-race and restarted its clock: the plugin's start counts.
    let mut race = started(5);
    race.hops = GOAL - 1;
    let score = race.hop(GOAL, 2_500, T0 + 9);
    assert_eq!(score.map(|s| s.ms), Some(8_000));
    let mut quick = started(6);
    quick.hops = GOAL - 1;
    assert_eq!(quick.hop(GOAL, 10, T0).map(|s| s.ms), Some(MIN_FINISH_MS));
}

#[test]
fn time_up_scores_the_hops_so_far() {
    let mut race = started(5);
    race.hop(1, 0, T0);
    race.hop(2, 0, T0);
    let score = race.time_up(T0 + 15).expect("still running");
    assert_eq!(score.hops, 2);
    assert_eq!(race.mine, Some(score));
}

#[test]
fn others_move_on_the_track_and_rank_by_hops_then_time() {
    let mut race = started(5);
    assert!(race.progress_received(2, "Bob".into(), Progress { id: 5, hops: 3 }));
    assert!(!race.progress_received(2, "Bob".into(), Progress { id: 5, hops: 2 }));
    assert!(!race.progress_received(2, "Bob".into(), Progress { id: 6, hops: 4 }));
    assert_eq!(race.runners[&2].hops, 3);

    let bob = Score {
        id: 5,
        hops: GOAL,
        ms: 7_000,
    };
    let cleo = Score {
        id: 5,
        hops: GOAL,
        ms: 5_000,
    };
    let dan = Score {
        id: 5,
        hops: 4,
        ms: 0,
    };
    assert!(race.score_received(2, "Bob".into(), bob));
    assert!(!race.score_received(2, "Bob".into(), cleo));
    assert!(!race.progress_received(2, "Bob".into(), Progress { id: 5, hops: GOAL }));
    assert!(race.score_received(3, "Cleo".into(), cleo));
    assert!(race.score_received(4, "Dan".into(), dan));
    // Someone who only sent progress has no result.
    assert!(race.progress_received(6, "Eve".into(), Progress { id: 5, hops: 9 }));
    race.hops = 6;
    race.time_up(T0 + 15);
    let names: Vec<_> = race.results().into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, [Some("Cleo"), Some("Bob"), None, Some("Dan")]);
    race.end();
    assert!(!race.score_received(7, "Fay".into(), cleo));
}

#[test]
fn page_messages_are_parsed_strictly() {
    assert_eq!(page::parse("ready"), Some(FromPage::Ready));
    assert_eq!(page::parse("start"), Some(FromPage::Start));
    assert_eq!(
        page::parse(r#"{"hop":3,"ms":1200}"#),
        Some(FromPage::Hop { hops: 3, ms: 1200 })
    );
    assert_eq!(page::parse(r#"{"hop":11,"ms":1}"#), None);
    assert_eq!(page::parse(r#"{"hop":1,"ms":1,"x":1}"#), None);
    assert_eq!(page::parse("nonsense"), None);
}

#[test]
fn the_page_state_carries_the_course_runners_and_texts() {
    let t = I18n::new("de");
    let mut race = started(5);
    race.hops = 2;
    race.progress_received(2, "Bob <b>".into(), Progress { id: 5, hops: 4 });
    let pet_of = |session: u32| (session == 2).then(|| "pets/bob.svg".to_owned());
    let look = Look {
        t: &t,
        language: "de",
        motion: Motion {
            level: AnimationLevel::Subtle,
            reduced: true,
        },
        dark: true,
        me: PageRunner {
            name: "Du".into(),
            pet: page::web_picture("assets/pets/fizzle-buddy-ember-small.svg"),
            hops: 0,
            me: true,
        },
        pet_of: &pet_of,
    };
    let json = page::state(&race, look, T0 + 4, "status".into(), Vec::new());
    let value: serde_json::Value = serde_json::from_str(&json).expect("json");
    assert_eq!(value["phase"], "racing");
    assert_eq!(value["leftMs"], 11_000);
    assert_eq!(value["motion"], "still");
    assert_eq!(value["dark"], true);
    assert_eq!(
        value["course"].as_array().map(Vec::len),
        Some(usize::from(GOAL))
    );
    assert_eq!(value["runners"][0]["hops"], 2);
    assert_eq!(
        value["runners"][0]["pet"],
        "pets/fizzle-buddy-ember-small.svg"
    );
    assert_eq!(value["runners"][1]["name"], "Bob <b>");
    assert_eq!(value["runners"][1]["pet"], "pets/bob.svg");
    assert_eq!(value["strings"]["lang"], "de");
    assert!(json.len() < 16 * 1024);
    assert!(t.missing().is_empty(), "missing: {:?}", t.missing());
}

#[test]
fn the_card_shows_lobby_race_and_results() {
    let t = I18n::new("en");
    let invite = Invite { id: 5, lobby: 30 };
    let mut race = Race::invited(invite, 2, "Bob".into(), T0);
    let view = |race: &Race| {
        let view = RaceView {
            race,
            t: &t,
            no_view: false,
        };
        let _ = view.card();
        view.status()
    };
    assert!(view(&race).starts_with("Bob started Snack Race"));
    race.join();
    assert_eq!(view(&race), t.t("race.waiting"));
    race.start_received(2, Start { id: 5 }, T0 + 30);
    assert_eq!(view(&race), t.t("race.racing"));
    race.hops = GOAL - 1;
    race.hop(GOAL, 6_420, T0 + 37);
    assert_eq!(view(&race), t.t("race.done"));
    race.score_received(
        2,
        "Bob".into(),
        Score {
            id: 5,
            hops: 7,
            ms: 0,
        },
    );
    race.end();
    assert_eq!(view(&race), "1. You 6.4 s · 2. Bob 7/10");
    assert!(t.missing().is_empty(), "missing: {:?}", t.missing());
}
