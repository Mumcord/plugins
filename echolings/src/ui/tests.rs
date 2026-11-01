use mumcord_plugin_sdk::ui::{Animation, Component, Node};

use super::*;
use crate::pet::Stage;

fn now() -> Now {
    Now {
        secs: 1_793_577_600,
        utc_offset_minutes: 0,
    }
}

fn pet(bond: u32) -> Pet {
    let mut pet = Pet::hatch("Biscuit".into(), Species::Zapling, 1, now());
    pet.bond = bond;
    pet
}

const FULL: Motion = Motion {
    level: AnimationLevel::Full,
    reduced: false,
};

fn view<'a>(pet: &'a Pet, session: &'a Session, t: &'a I18n, motion: Motion) -> View<'a> {
    View {
        pet,
        session,
        now: now(),
        t,
        motion,
        reaction: None,
        refusal: None,
    }
}

/// Checks the host's tree rules this plugin can break: size, labels and text length.
fn check(nodes: &[Node]) {
    assert!(nodes.len() <= 500);
    for node in nodes {
        let (label, texts): (Option<&str>, Vec<&str>) = match &node.component {
            Component::Button(b) => (Some(&b.label), vec![&b.label]),
            Component::Select(s) => (
                Some(&s.label),
                s.options.iter().map(|o| o.label.as_str()).collect(),
            ),
            Component::TextInput(i) => (Some(&i.label), vec![&i.value]),
            Component::Meter(m) => (Some(&m.label), vec![]),
            Component::Progress(p) => (Some(&p.label), vec![]),
            Component::Text(t) => (None, vec![&t.text]),
            Component::Chip(c) => (None, vec![&c.text]),
            Component::Image(i) => {
                assert!(i.asset.starts_with("assets/pets/"));
                (Some(&i.alt), vec![&i.alt])
            }
            _ => (None, vec![]),
        };
        if let Some(label) = label {
            assert!(!label.is_empty());
        }
        for text in texts {
            assert!(text.chars().count() <= 200, "{text}");
        }
    }
}

fn animation_of_picture(nodes: &[Node]) -> Option<Animation> {
    nodes
        .iter()
        .find(|n| matches!(n.component, Component::Image(_)))
        .and_then(|n| n.animation)
}

#[test]
fn every_view_uses_known_strings_and_valid_trees() {
    let t = I18n::new("en");
    let mut session = Session::default();
    for bond in [0, 3, 10, 25] {
        for refusal in [
            None,
            Some(Refusal::NotEnoughNotes),
            Some(Refusal::TooTired),
            Some(Refusal::Asleep),
        ] {
            for dnd in [false, true] {
                session.do_not_disturb = dnd;
                let pet = pet(bond);
                let mut v = view(&pet, &session, &t, FULL);
                v.refusal = refusal;
                for tree in [
                    v.panel_inline(),
                    v.panel_expanded(),
                    v.badge(),
                    v.profile_card(),
                ] {
                    check(&tree.into_nodes());
                }
            }
        }
    }
    check(&hatch_prompt(&t).into_nodes());
    for species in Species::ALL {
        let draft = Draft {
            species,
            colour: 1,
            name: "Biscuit".into(),
        };
        check(&picker(&t, &draft, true, FULL).into_nodes());
    }
    for level in AnimationLevel::ALL {
        check(&settings(&t, level).into_nodes());
    }
    assert_eq!(t.missing(), Vec::<String>::new());
}

#[test]
fn label_names_pet_species_and_mood() {
    let t = I18n::new("en");
    let pet = pet(0);
    let session = Session::default();
    assert_eq!(
        view(&pet, &session, &t, FULL).label(),
        "Biscuit the Zapling, cheerful"
    );
}

#[test]
fn animation_follows_state_and_motion_setting() {
    let t = I18n::new("en");
    let pet = pet(0);
    let mut session = Session::default();
    let badge = |session: &Session, motion: Motion, reaction: Option<Reaction>| {
        let mut v = view(&pet, session, &t, motion);
        v.reaction = reaction;
        animation_of_picture(&v.badge().into_nodes())
    };
    assert_eq!(badge(&session, FULL, None), None);
    session.talking = true;
    assert_eq!(badge(&session, FULL, None), Some(Animation::Bob));
    assert_eq!(
        badge(&session, FULL, Some(Reaction::Hop)),
        Some(Animation::Hop)
    );
    let subtle = Motion {
        level: AnimationLevel::Subtle,
        reduced: false,
    };
    assert_eq!(badge(&session, subtle, Some(Reaction::Hop)), None);
    assert_eq!(badge(&session, subtle, None), Some(Animation::Bob));
    let still = Motion {
        level: AnimationLevel::Still,
        reduced: false,
    };
    assert_eq!(badge(&session, still, None), None);
    let reduced = Motion {
        level: AnimationLevel::Full,
        reduced: true,
    };
    assert_eq!(badge(&session, reduced, None), None);
    session.do_not_disturb = true;
    assert_eq!(
        badge(&session, FULL, Some(Reaction::Hop)),
        Some(Animation::Nap)
    );
}

#[test]
fn echo_sparkles_and_locked_colour_is_marked() {
    let t = I18n::new("en");
    let session = Session::default();
    let echo = pet(25);
    assert_eq!(echo.stage(), Stage::Echo);
    let v = view(&echo, &session, &t, FULL);
    assert_eq!(
        animation_of_picture(&v.badge().into_nodes()),
        Some(Animation::Sparkle)
    );

    let young = pet(0);
    let nodes = view(&young, &session, &t, FULL)
        .panel_expanded()
        .into_nodes();
    let colours = nodes
        .iter()
        .find_map(|n| match &n.component {
            Component::Select(s) if s.action == action::COLOUR => Some(s),
            _ => None,
        })
        .unwrap();
    assert_eq!(colours.options[2].label, "Midnight (unlocks at Bond 10)");
    assert_eq!(colours.selected.as_deref(), Some("1"));
}

#[test]
fn treat_and_play_buttons_are_disabled_when_refused() {
    let t = I18n::new("en");
    let session = Session::default();
    let mut pet = pet(0);
    pet.energy = 0;
    let nodes = view(&pet, &session, &t, FULL).panel_expanded().into_nodes();
    let disabled = |name: &str| {
        nodes
            .iter()
            .any(|n| matches!(&n.component, Component::Button(b) if b.action == name && b.disabled))
    };
    assert!(disabled(action::TREAT));
    assert!(disabled(action::PLAY));
    assert!(!disabled(action::PAT));
}
