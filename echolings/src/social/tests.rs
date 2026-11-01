use mumcord_plugin_sdk::ui::{Animation, Component, Node};

use super::card::{Card, MAX_BYTES};
use super::throttle::{CARD_INTERVAL_SECS, CardThrottle, Decision};
use super::visits::{Ledger, visitor_id};
use super::*;
use crate::pet::{Mood, Now, Pet, Session, Species, Stage, balance};
use crate::storage::AnimationLevel;
use crate::ui::{Motion, i18n::I18n};

const T0: u64 = 1_793_577_600;

fn now(secs: u64) -> Now {
    Now {
        secs,
        utc_offset_minutes: 0,
    }
}

fn card(name: &str) -> Card {
    Card {
        species: Species::Hushmoth,
        colour: 2,
        mood: Mood::Cheerful,
        bond: 26,
        days_together: 40,
        streak: 12,
        name: name.into(),
    }
}

const FULL: Motion = Motion {
    level: AnimationLevel::Full,
    reduced: false,
};

#[test]
fn card_round_trips_and_stays_under_100_bytes() {
    let longest = card("ẞẞẞẞẞẞẞẞẞẞẞẞẞẞẞẞ");
    let bytes = longest.encode();
    assert!(bytes.len() <= MAX_BYTES, "{} bytes", bytes.len());
    assert_eq!(Card::decode(&bytes), Some(longest));
    let c = card("Biscuit");
    assert_eq!(c.stage(), Stage::Echo);
    assert_eq!(c.asset(), "assets/pets/hushmoth-echo-aurora.svg");
}

#[test]
fn card_of_a_pet_clamps_and_reads_the_pet() {
    let mut pet = Pet::hatch("Biscuit".into(), Species::Fizzle, 1, now(T0));
    pet.bond = 100_000;
    let c = Card::of(&pet, &Session::default(), now(T0));
    assert_eq!(c.bond, u16::MAX);
    assert_eq!(c.species, Species::Fizzle);
    assert_eq!(c.name, "Biscuit");
    assert_eq!(c.days_together, 1);
}

#[test]
fn untrusted_cards_are_dropped_or_cleaned() {
    let good = card("Biscuit").encode();
    let with = |i: usize, v: u8| {
        let mut b = good.clone();
        b[i] = v;
        b
    };
    assert_eq!(Card::decode(&with(0, 0)), None, "version 0");
    assert_eq!(Card::decode(&with(1, 5)), None, "unknown species");
    assert_eq!(Card::decode(&with(2, 3)), None, "unknown colour");
    assert_eq!(Card::decode(&with(3, 9)), None, "unknown mood");
    assert_eq!(
        Card::decode(&with(10, 50)),
        None,
        "name longer than the payload"
    );
    assert_eq!(Card::decode(&good[..5]), None, "truncated");
    assert_eq!(Card::decode(&[0xff; 101]), None, "over 100 bytes");

    // A later version may append fields.
    let mut newer = with(0, 2);
    newer.extend_from_slice(&[1, 2, 3]);
    assert_eq!(Card::decode(&newer), Some(card("Biscuit")));

    // Names are cleaned like the user's own; invalid UTF-8 or an empty name drops the card.
    let mut raw = good[..10].to_vec();
    let name = "  Bis\u{7}cuit   the  great and terrible ";
    raw.push(name.len() as u8);
    raw.extend_from_slice(name.as_bytes());
    assert_eq!(Card::decode(&raw).unwrap().name, "Biscuit the grea");
    let mut bad = good[..10].to_vec();
    bad.extend_from_slice(&[2, 0xff, 0xfe]);
    assert_eq!(Card::decode(&bad), None);
    let mut blank = good[..10].to_vec();
    blank.extend_from_slice(&[2, b' ', b' ']);
    assert_eq!(Card::decode(&blank), None);
}

#[test]
fn card_is_sent_at_most_once_a_minute() {
    let mut throttle = CardThrottle::default();
    let a = card("A").encode();
    let b = card("B").encode();
    assert_eq!(throttle.decide(&a, T0), Decision::Send);
    throttle.attempted(&a, true, T0);
    assert_eq!(throttle.decide(&a, T0 + 5), Decision::Idle);
    // A change waits for the rest of the minute.
    assert_eq!(throttle.decide(&b, T0 + 20), Decision::Wait(40));
    assert_eq!(throttle.decide(&b, T0 + CARD_INTERVAL_SECS), Decision::Send);
    throttle.attempted(&b, true, T0 + 60);
    // Someone joined: the same card goes out again, still within the limit.
    throttle.invalidate();
    assert_eq!(throttle.decide(&b, T0 + 61), Decision::Wait(59));
    assert_eq!(throttle.decide(&b, T0 + 120), Decision::Send);
    // A refused send is retried, but not before the minute is over.
    throttle.attempted(&b, false, T0 + 120);
    assert_eq!(throttle.decide(&b, T0 + 130), Decision::Wait(50));
}

fn social_with_pet(session: u32, key: Option<&str>) -> Social {
    let mut social = Social::default();
    assert!(social.card_received(session, key.map(str::to_owned), &card("Biscuit").encode()));
    social
}

#[test]
fn other_pets_follow_the_channel() {
    let mut social = social_with_pet(7, Some("k7"));
    assert!(!social.card_received(7, Some("k7".into()), &card("Biscuit").encode()));
    assert!(!social.card_received(8, None, b"garbage"));
    assert!(social.talking(7, true));
    assert!(!social.talking(7, true), "unchanged");
    assert!(!social.talking(8, true), "no pet");
    // A new card keeps the talking state.
    assert!(social.card_received(7, Some("k7".into()), &card("Bun").encode()));
    assert!(social.pets[&7].talking);
    assert!(social.left(7));
    assert!(!social.left(7));

    let mut social = social_with_pet(7, None);
    social.open_card = Some(7);
    assert_eq!(social.moved(T0), vec![7]);
    assert!(social.pets.is_empty());
    assert_eq!(social.open_card, None);
}

#[test]
fn one_pat_per_pet_per_day() {
    let day = now(T0).day();
    let mut social = social_with_pet(7, Some("k7"));
    assert!(social.can_pat(7, day));
    assert!(social.give_pat(7, day));
    assert!(!social.can_pat(7, day));
    assert!(!social.give_pat(7, day));
    // The same person in a new session is still the same key.
    social.left(7);
    assert!(social.card_received(9, Some("k7".into()), &card("B").encode()));
    assert!(!social.can_pat(9, day));
    // Tomorrow it works again.
    assert!(social.can_pat(9, day + 1));
    assert!(social.give_pat(9, day + 1));
    // No pet, no pat.
    assert!(!social.give_pat(42, day + 1));
}

#[test]
fn received_pats_count_once_per_visitor_per_day() {
    let day = now(T0).day();
    let mut social = Social::default();
    assert!(social.pat_received(3, Some("k3"), &PAT_PAYLOAD, day));
    assert!(
        !social.pat_received(4, Some("k3"), &PAT_PAYLOAD, day),
        "same key"
    );
    assert!(social.pat_received(5, None, &PAT_PAYLOAD, day));
    assert!(
        !social.pat_received(5, None, &PAT_PAYLOAD, day),
        "same session"
    );
    assert!(
        !social.pat_received(6, Some("k6"), &[], day),
        "empty payload"
    );
    assert!(
        !social.pat_received(6, Some("k6"), &[0], day),
        "unknown version"
    );
    assert!(social.pat_received(3, Some("k3"), &PAT_PAYLOAD, day + 1));
}

#[test]
fn ledger_is_bounded_and_serialisable() {
    let mut ledger = Ledger::default();
    for i in 0..300 {
        ledger.receive(&visitor_id(None, i), 1);
    }
    assert_eq!(ledger.received.len(), 200);
    let json = serde_json::to_string(&ledger).unwrap();
    assert_eq!(serde_json::from_str::<Ledger>(&json).unwrap(), ledger);
}

#[test]
fn visits_and_games_give_capped_notes() {
    let t = now(T0);
    let session = Session::default();
    let mut pet = Pet::hatch("Biscuit".into(), Species::Mossmo, 0, t);
    let start = pet.notes;
    for _ in 0..20 {
        pet.visit(t);
    }
    assert_eq!(pet.notes - start, balance::VISIT_NOTES_PER_DAY);
    let cheer = pet.cheer;
    assert_eq!(pet.visited(&session, t), Some(crate::pet::Reaction::Hop));
    assert!(pet.cheer > cheer || pet.cheer == balance::METER_MAX);
    for _ in 0..20 {
        pet.visited(&session, t);
    }
    for _ in 0..20 {
        pet.game_finished(t);
    }
    assert_eq!(
        pet.notes - start,
        balance::VISIT_NOTES_PER_DAY + balance::VISITED_NOTES_PER_DAY + balance::GAME_NOTES_PER_DAY
    );
    // A sleeping pet takes the pat without hopping.
    let mut dnd = Session::default();
    dnd.do_not_disturb = true;
    assert_eq!(pet.visited(&dnd, t), None);
}

#[test]
fn reactions_skip_replayed_joins() {
    let quiet = T0 + JOIN_QUIET_SECS;
    assert_eq!(reaction(Happening::Joined, T0, quiet), None);
    assert_eq!(
        reaction(Happening::Joined, quiet, quiet),
        Some(crate::pet::Reaction::Wave)
    );
    assert_eq!(
        reaction(Happening::ScreenShare, T0, quiet),
        Some(crate::pet::Reaction::Sparkle)
    );
    assert_eq!(
        reaction(Happening::Mentioned, T0, quiet),
        Some(crate::pet::Reaction::Hop)
    );
}

#[test]
fn pat_actions_round_trip() {
    assert_eq!(action::parse_pat(&action::pat(42)), Some(42));
    assert_eq!(action::parse_pat("visit-pat:x"), None);
    assert_eq!(action::parse_pat("pat"), None);
}

#[test]
fn display_names_are_made_safe() {
    assert_eq!(display_name(" Ana\u{1b}[31m "), "Ana[31m");
    assert_eq!(display_name(&"x".repeat(100)).len(), 32);
}

fn texts(nodes: &[Node]) -> Vec<String> {
    nodes
        .iter()
        .filter_map(|n| match &n.component {
            Component::Text(t) => Some(t.text.clone()),
            Component::Button(b) => Some(b.label.clone()),
            Component::Chip(c) => Some(c.text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn other_pets_badge_bobs_while_talking_and_respects_motion() {
    let t = I18n::new("en");
    let mut pet = OtherPet {
        card: card("Biscuit"),
        key: None,
        talking: true,
    };
    pet.card.bond = 3;
    let nodes = OtherView {
        pet: &pet,
        t: &t,
        motion: FULL,
    }
    .badge()
    .into_nodes();
    assert_eq!(nodes[0].animation, Some(Animation::Bob));
    let Component::Image(image) = &nodes[0].component else {
        panic!("badge is an image");
    };
    assert_eq!(image.alt, "Biscuit the Hushmoth, cheerful");
    let still = Motion {
        level: AnimationLevel::Full,
        reduced: true,
    };
    let nodes = OtherView {
        pet: &pet,
        t: &t,
        motion: still,
    }
    .badge()
    .into_nodes();
    assert_eq!(nodes[0].animation, None);
    assert!(t.missing().is_empty(), "{:?}", t.missing());
}

#[test]
fn profile_card_offers_one_pat() {
    let t = I18n::new("en");
    let pet = OtherPet {
        card: card("Biscuit"),
        key: None,
        talking: false,
    };
    let view = OtherView {
        pet: &pet,
        t: &t,
        motion: FULL,
    };
    let button = |nodes: &[Node]| {
        nodes
            .iter()
            .find_map(|n| match &n.component {
                Component::Button(b) => Some(b.clone()),
                _ => None,
            })
            .unwrap()
    };
    let open = view.profile_card(7, false, true, None).into_nodes();
    let b = button(&open);
    assert_eq!(
        (b.label.as_str(), b.action.as_str()),
        ("Pat Biscuit", "visit-pat:7")
    );
    assert!(!b.disabled);
    assert!(texts(&open).contains(&"Bond 26".to_owned()));

    let done = view
        .profile_card(7, true, true, Some(PatNote::Given))
        .into_nodes();
    assert!(button(&done).disabled);
    assert!(texts(&done).contains(&"You patted Biscuit. See you tomorrow!".to_owned()));

    let later = view.profile_card(7, true, true, None).into_nodes();
    assert!(texts(&later).contains(&"You already patted Biscuit today.".to_owned()));

    let no_messages = view.profile_card(7, false, false, None).into_nodes();
    assert!(button(&no_messages).disabled);
    let failed = view
        .profile_card(7, false, true, Some(PatNote::Failed))
        .into_nodes();
    assert!(
        texts(&failed)
            .iter()
            .any(|s| s.contains("could not be reached"))
    );
    assert!(t.missing().is_empty(), "{:?}", t.missing());
}
