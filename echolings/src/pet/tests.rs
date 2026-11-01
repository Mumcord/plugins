use super::balance::*;
use super::*;

const DAY: u64 = 86_400;
/// 2026-11-02 00:00 UTC, a Monday.
const T0: u64 = 1_793_577_600;

fn at(secs: u64) -> Now {
    Now {
        secs,
        utc_offset_minutes: 0,
    }
}

fn new_pet() -> Pet {
    Pet::hatch("Biscuit".into(), Species::Fizzle, 0, at(T0))
}

/// A session in voice with one other user.
fn in_voice() -> Session {
    let mut session = Session {
        me: Some(1),
        ..Session::default()
    };
    session.others.insert(2);
    session
}

/// Talks and ticks every minute for `minutes`, starting at `start`.
fn talk_for(pet: &mut Pet, session: &mut Session, start: u64, minutes: u64) {
    pet.on_own_talking(session, true, at(start));
    for m in 0..=minutes {
        pet.tick(session, at(start + m * 60));
    }
}

#[test]
fn stages_follow_bond() {
    assert_eq!(Stage::for_bond(0), Stage::Hum);
    assert_eq!(Stage::for_bond(2), Stage::Hum);
    assert_eq!(Stage::for_bond(3), Stage::Sprout);
    assert_eq!(Stage::for_bond(10), Stage::Buddy);
    assert_eq!(Stage::for_bond(25), Stage::Echo);
    assert_eq!(Stage::Echo.next_bond(), None);
    assert_eq!(Stage::Hum.next_bond(), Some(3));
}

#[test]
fn species_ids_round_trip_and_assets_exist() {
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/pets");
    for species in Species::ALL {
        assert_eq!(Species::from_id(species.id()), Some(species));
        for colour in 0..3 {
            for bond in [0, SPROUT_BOND, BUDDY_BOND, ECHO_BOND] {
                let mut pet = Pet::hatch("A".into(), species, 0, at(T0));
                pet.bond = bond;
                pet.colour = colour;
                for (small, asleep) in [(false, false), (true, false), (false, true), (true, true)]
                {
                    let file = pet.asset(small, asleep);
                    let name = file.strip_prefix("assets/pets/").unwrap();
                    assert!(assets.join(name).is_file(), "missing {file}");
                }
            }
        }
    }
    assert_eq!(Species::from_id("dragon"), None);
}

#[test]
fn hatching_gives_a_hum_with_a_one_day_streak() {
    let pet = Pet::hatch("Biscuit".into(), Species::Mossmo, 2, at(T0));
    assert_eq!(pet.stage(), Stage::Hum);
    assert_eq!(pet.colour, 1, "the third colour is locked at first");
    assert_eq!(pet.streak.count, 1);
    assert_eq!(pet.streak.days_together, 1);
    assert_eq!((pet.cheer, pet.energy), (START_CHEER, START_ENERGY));
}

#[test]
fn third_colour_unlocks_at_buddy() {
    let mut pet = new_pet();
    assert!(!pet.set_colour(2));
    pet.bond = THIRD_COLOUR_BOND;
    assert!(pet.set_colour(2));
    assert_eq!(pet.colour_id(), "frost");
}

#[test]
fn voice_with_friends_earns_notes_and_bond() {
    let mut pet = new_pet();
    let mut session = in_voice();
    talk_for(&mut pet, &mut session, T0 + 3600, 10);
    assert_eq!(pet.notes, 2 * VOICE_BLOCK_NOTES);
    assert_eq!(pet.bond_points, 2 * VOICE_BLOCK_POINTS);
    assert_eq!(pet.energy, START_ENERGY - 2 * VOICE_BLOCK_ENERGY_COST);
}

#[test]
fn idling_alone_deafened_or_silent_earns_nothing() {
    let start = T0 + 3600;
    for setup in [
        |s: &mut Session| s.others.clear(),
        |s: &mut Session| s.deafened = true,
        |s: &mut Session| s.me = None,
    ] {
        let mut pet = new_pet();
        let mut session = in_voice();
        setup(&mut session);
        talk_for(&mut pet, &mut session, start, 30);
        assert_eq!(pet.notes, 0);
    }
    // In company but silent for longer than the talk window: only the first minutes count.
    let mut pet = new_pet();
    let mut session = in_voice();
    pet.on_own_talking(&mut session, true, at(start));
    pet.on_own_talking(&mut session, false, at(start));
    for m in 0..=60 {
        pet.tick(&mut session, at(start + m * 60));
    }
    assert_eq!(pet.notes, 1);
}

#[test]
fn voice_notes_and_bond_points_are_capped_per_day() {
    let mut pet = new_pet();
    let mut session = in_voice();
    talk_for(&mut pet, &mut session, T0 + 60, 12 * 60);
    assert_eq!(pet.today.voice_notes, VOICE_NOTES_PER_DAY);
    assert_eq!(pet.notes, VOICE_NOTES_PER_DAY);
    assert_eq!(pet.bond, 1, "at most one Bond per day");
    assert_eq!(pet.bond_points, 0);
    // A new local day resets the caps.
    talk_for(&mut pet, &mut session, T0 + DAY + 60, 10);
    assert_eq!(pet.today.voice_notes, 2);
}

#[test]
fn echo_takes_at_least_twenty_five_days() {
    let mut pet = new_pet();
    let mut session = in_voice();
    for day in 0..24 {
        talk_for(&mut pet, &mut session, T0 + day * DAY + 60, 9 * 60);
    }
    assert_eq!(pet.bond, 24);
    assert_eq!(pet.stage(), Stage::Buddy);
    talk_for(&mut pet, &mut session, T0 + 24 * DAY + 60, 9 * 60);
    assert_eq!(pet.stage(), Stage::Echo);
    assert_eq!(pet.streak.count, 25);
}

#[test]
fn own_chat_is_capped_and_others_only_cheer() {
    let mut pet = new_pet();
    let mut session = in_voice();
    for i in 0..20 {
        pet.on_chat(&mut session, true, at(T0 + i));
    }
    assert_eq!(pet.notes, CHAT_NOTES_PER_DAY);
    pet.cheer = 50;
    pet.on_chat(&mut session, false, at(T0 + 100));
    assert_eq!(pet.notes, CHAT_NOTES_PER_DAY);
    assert_eq!(pet.cheer, 51);
}

#[test]
fn answering_a_mention_counts_as_helping() {
    let mut pet = new_pet();
    let mut session = in_voice();
    session.mentioned(at(T0));
    pet.on_chat(&mut session, true, at(T0 + HELP_WINDOW_SECS));
    assert_eq!(pet.today.help_notes, HELP_NOTES);
    // Too late: only the chat Note.
    session.mentioned(at(T0 + 1000));
    pet.on_chat(&mut session, true, at(T0 + 1000 + HELP_WINDOW_SECS + 1));
    assert_eq!(pet.today.help_notes, HELP_NOTES);
    for i in 0..10 {
        session.mentioned(at(T0 + 5000 + i * 10));
        pet.on_chat(&mut session, true, at(T0 + 5001 + i * 10));
    }
    assert_eq!(pet.today.help_notes, HELPS_PER_DAY * HELP_NOTES);
}

#[test]
fn talking_soon_after_a_join_is_a_greeting() {
    let mut pet = new_pet();
    let mut session = in_voice();
    session.joined(3, at(T0));
    assert!(pet.on_own_talking(&mut session, true, at(T0 + GREET_WINDOW_SECS)));
    assert_eq!(pet.notes, GREET_NOTES);
    // Only once per join.
    assert!(!pet.on_own_talking(&mut session, true, at(T0 + 61)));
    session.joined(4, at(T0 + 100));
    assert!(!pet.on_own_talking(&mut session, true, at(T0 + 100 + GREET_WINDOW_SECS + 1)));
    for i in 0..10 {
        session.joined(10 + i, at(T0 + 1000 + u64::from(i)));
        pet.on_own_talking(&mut session, true, at(T0 + 1000 + u64::from(i)));
    }
    assert_eq!(pet.today.greet_notes, GREETS_PER_DAY * GREET_NOTES);
    session.left(3);
    assert!(!session.others.contains(&3));
}

#[test]
fn pat_is_free_and_its_bond_points_are_capped() {
    let mut pet = new_pet();
    let session = Session::default();
    pet.cheer = 50;
    for i in 0..20 {
        assert_eq!(pet.pat(&session, at(T0 + i)), Some(Reaction::Hop));
    }
    assert_eq!(pet.cheer, METER_MAX);
    assert_eq!(pet.bond_points, PAT_POINTS_PER_DAY);
    assert_eq!(pet.notes, 0);
}

#[test]
fn treat_costs_notes() {
    let mut pet = new_pet();
    let session = Session::default();
    assert_eq!(pet.treat(&session, at(T0)), Err(Refusal::NotEnoughNotes));
    pet.notes = TREAT_COST + 1;
    pet.energy = 10;
    assert_eq!(pet.treat(&session, at(T0)), Ok(Some(Reaction::Sparkle)));
    assert_eq!(pet.notes, 1);
    assert_eq!(pet.energy, 10 + TREAT_ENERGY);
}

#[test]
fn play_needs_energy_and_an_awake_pet() {
    let mut pet = new_pet();
    let mut session = Session::default();
    pet.energy = PLAY_ENERGY_COST - 1;
    assert_eq!(pet.play(&session, at(T0)), Err(Refusal::TooTired));
    pet.energy = 50;
    assert_eq!(pet.play(&session, at(T0)), Ok(Reaction::Wave));
    assert_eq!(pet.energy, 50 - PLAY_ENERGY_COST);
    session.do_not_disturb = true;
    assert_eq!(pet.play(&session, at(T0)), Err(Refusal::Asleep));
}

#[test]
fn a_nap_refills_energy_and_ends_by_itself() {
    let mut pet = new_pet();
    let mut session = Session::default();
    pet.energy = 10;
    assert_eq!(pet.nap(&session, at(T0)), Ok(Reaction::Nap));
    assert_eq!(pet.mood(&session, at(T0 + 60)), Mood::Sleeping);
    assert_eq!(pet.nap(&session, at(T0 + 60)), Err(Refusal::Asleep));
    for m in 0..=31 {
        pet.tick(&mut session, at(T0 + m * 60));
    }
    assert_eq!(pet.energy, 10 + 60);
    assert_eq!(pet.nap_until, 0);
    assert!(!pet.asleep(&session, at(T0 + 31 * 60)));
}

#[test]
fn do_not_disturb_puts_the_pet_to_sleep_without_rewards() {
    let mut pet = new_pet();
    let mut session = in_voice();
    session.do_not_disturb = true;
    pet.energy = 10;
    talk_for(&mut pet, &mut session, T0 + 60, 30);
    assert_eq!(pet.notes, 0);
    assert_eq!(pet.energy, 20);
    assert_eq!(pet.mood(&session, at(T0 + 60)), Mood::Sleeping);
    assert_eq!(
        pet.pat(&session, at(T0 + 3600)),
        None,
        "no reactions in DND"
    );
}

#[test]
fn time_away_only_makes_the_pet_miss_you() {
    let mut pet = new_pet();
    pet.energy = 0;
    pet.bond = 7;
    pet.catch_up(at(T0 + 60 * DAY));
    assert_eq!(pet.cheer, CHEER_FLOOR);
    assert_eq!(pet.energy, METER_MAX);
    assert_eq!(pet.bond, 7);
    assert_eq!(
        pet.mood(&Session::default(), at(T0 + 60 * DAY)),
        Mood::MissesYou
    );
    // A short break changes nothing.
    let mut pet = new_pet();
    pet.catch_up(at(T0 + 60));
    assert_eq!(pet.cheer, START_CHEER);
    // Alone with the app open: 2 Cheer per hour.
    let mut pet = new_pet();
    let mut session = Session::default();
    for m in 0..=60 {
        pet.tick(&mut session, at(T0 + m * 60));
    }
    assert_eq!(pet.cheer, START_CHEER - 2);
    // A long gap between ticks (computer asleep) counts as time away.
    pet.tick(&mut session, at(T0 + 3600 + 10 * 3600));
    assert_eq!(pet.cheer, START_CHEER - 2 - 20);
}

#[test]
fn bond_never_decreases() {
    let mut pet = new_pet();
    let mut session = in_voice();
    let mut bond = pet.bond;
    let mut t = T0;
    for step in 0u64..20_000 {
        t += [7, 60, 61, 300, 3600, 3 * DAY][usize::try_from(step % 6).unwrap()];
        let now = at(t);
        match step % 11 {
            0 => drop(pet.pat(&session, now)),
            1 => drop(pet.treat(&session, now)),
            2 => drop(pet.play(&session, now)),
            3 => drop(pet.nap(&session, now)),
            4 => drop(pet.on_own_talking(&mut session, step % 3 == 0, now)),
            5 => pet.on_chat(&mut session, step % 2 == 0, now),
            6 => session.do_not_disturb = !session.do_not_disturb,
            7 => pet.catch_up(now),
            8 => session.joined(5, now),
            9 => session.mentioned(now),
            _ => pet.tick(&mut session, now),
        }
        assert!(pet.bond >= bond);
        assert!(pet.cheer <= METER_MAX && pet.energy <= METER_MAX);
        assert!(pet.bond_points < POINTS_PER_BOND);
        bond = pet.bond;
    }
    assert!(pet.bond > 0);
}

#[test]
fn streak_bridges_one_rest_day_per_week() {
    let mut streak = Streak::default();
    streak.mark(100);
    streak.mark(100);
    streak.mark(101);
    assert_eq!(streak.count, 2);
    streak.mark(103); // day 102 rested
    assert_eq!(streak.count, 3);
    assert_eq!(streak.current(104), 3);
    streak.mark(105); // second rest within a week breaks it
    assert_eq!(streak.count, 1);
    assert_eq!(streak.days_together, 4);
    streak.mark(106);
    streak.mark(108); // 107 rested, 5 days after the last rest: too soon
    assert_eq!(streak.count, 1);
    streak.mark(109);
    streak.mark(111); // 110 rested, 8 days after day 102
    assert_eq!(streak.count, 3);
    assert_eq!(streak.current(112), 3);
    assert_eq!(streak.current(114), 0);
    streak.mark(50); // clock went back
    assert_eq!(streak.last_day, Some(111));
}

#[test]
fn rest_day_shows_in_current_streak() {
    let mut streak = Streak::default();
    streak.mark(10);
    streak.mark(11);
    assert_eq!(streak.current(13), 2, "one missed day can still be bridged");
}

#[test]
fn local_day_uses_the_utc_offset() {
    let late = Now {
        secs: T0 - 1800,
        utc_offset_minutes: 60,
    };
    let early = Now {
        secs: T0 + 1800,
        utc_offset_minutes: -60,
    };
    assert_eq!(late.day(), at(T0).day());
    assert_eq!(early.day(), at(T0).day() - 1);
}

#[test]
fn names_are_cleaned_and_limited() {
    assert_eq!(clean_name("  Bis\u{0}cuit  "), Some("Biscuit".into()));
    assert_eq!(clean_name("a   b\n c"), Some("a b c".into()));
    assert_eq!(
        clean_name("ABCDEFGHIJKLMNOPQRSTUVWXYZ"),
        Some("ABCDEFGHIJKLMNOP".into())
    );
    assert_eq!(clean_name(" \t "), None);
    assert_eq!(clean_name("ééééééééééééééééé").unwrap().chars().count(), 16);
}
