//! Native tests of the EFT parser and the card/dialog logic against a recording fake host.

use mumcord_plugin_sdk::{
    events::Event,
    i18n::Strings,
    types::Error,
    ui::{Component, El},
};

use crate::app::{App, Host, KEPT_FITS, card_id};
use crate::eft::{self, Item, MAX_LINES, MAX_TEXT_BYTES, Section};
use crate::view;

/// The game's own "Copy to clipboard" export: empty slots, charges, drones and cargo.
const RIFTER: &str = "[Rifter, Tackle]
Damage Control II
Gyrostabilizer II
[Empty Low slot]

5MN Microwarpdrive II
Warp Scrambler II
Stasis Webifier II

200mm AutoCannon II, Republic Fleet EMP S
200mm AutoCannon II, Republic Fleet EMP S
200mm AutoCannon II, Republic Fleet EMP S
[Empty High slot]

Small Projectile Burst Aerator I
Small Projectile Burst Aerator I
[Empty Rig slot]


Warrior II x3


Republic Fleet EMP S x1000
Nanite Repair Paste x50
";

/// A Pyfa export of a T3 cruiser with subsystems, an offline module, implants and CRLF lines.
const LEGION: &str = "[Legion, Armor HAM]\r
Heat Sink II\r
Heat Sink II\r
1600mm Steel Plates II\r
Energized Adaptive Nano Membrane II\r
\r
10MN Afterburner II\r
Stasis Webifier II /OFFLINE\r
\r
Heavy Assault Missile Launcher II, Scourge Rage Heavy Assault Missile\r
Heavy Assault Missile Launcher II, Scourge Rage Heavy Assault Missile\r
\r
Medium Trimark Armor Pump I\r
Medium Trimark Armor Pump I\r
Medium Trimark Armor Pump I\r
\r
Legion Core - Augmented Antimatter Reactor\r
Legion Defensive - Covert Reconfiguration\r
Legion Offensive - Assault Optimization\r
Legion Propulsion - Intercalated Nanofibers\r
\r
\r
Hammerhead II x5\r
\r
High-grade Slave Alpha\r
\r
Scourge Rage Heavy Assault Missile x2000\r
";

fn group(fit: &eft::Fit, section: Section) -> &eft::Group {
    fit.group(section)
        .unwrap_or_else(|| panic!("no {section:?} in {fit:?}"))
}

fn item(name: &str, charge: Option<&str>, count: u32) -> Item {
    Item {
        name: name.into(),
        charge: charge.map(Into::into),
        count,
        offline: false,
    }
}

#[test]
fn parses_the_game_export() {
    let fit = eft::parse(RIFTER).expect("a fit");
    assert_eq!(fit.ship, "Rifter");
    assert_eq!(fit.name, "Tackle");
    let low = group(&fit, Section::Low);
    assert_eq!((low.filled(), low.empty), (2, 1));
    assert_eq!(group(&fit, Section::Mid).filled(), 3);
    let high = group(&fit, Section::High);
    assert_eq!(
        high.items,
        vec![item("200mm AutoCannon II", Some("Republic Fleet EMP S"), 3)]
    );
    assert_eq!(high.empty, 1);
    let rig = group(&fit, Section::Rig);
    assert_eq!((rig.filled(), rig.empty), (2, 1));
    assert_eq!(
        group(&fit, Section::Drones).items,
        vec![item("Warrior II", None, 3)]
    );
    assert_eq!(
        group(&fit, Section::Cargo).items,
        vec![
            item("Republic Fleet EMP S", None, 1000),
            item("Nanite Repair Paste", None, 50)
        ]
    );
}

#[test]
fn parses_subsystems_offline_modules_implants_and_crlf() {
    let fit = eft::parse(LEGION).expect("a fit");
    assert_eq!(group(&fit, Section::Low).filled(), 4);
    let mid = group(&fit, Section::Mid);
    assert_eq!(mid.items.len(), 2);
    assert!(mid.items[1].offline);
    assert_eq!(mid.items[1].name, "Stasis Webifier II");
    assert_eq!(group(&fit, Section::Rig).filled(), 3);
    assert_eq!(group(&fit, Section::Subsystem).filled(), 4);
    assert_eq!(group(&fit, Section::Drones).filled(), 5);
    assert_eq!(group(&fit, Section::Cargo).filled(), 2000);
    assert_eq!(
        group(&fit, Section::Other).items,
        vec![item("High-grade Slave Alpha", None, 1)]
    );
    // Bays come after the slots and the implants last.
    let order: Vec<Section> = fit.groups.iter().map(|g| g.section).collect();
    assert_eq!(
        order,
        [
            Section::Low,
            Section::Mid,
            Section::High,
            Section::Rig,
            Section::Subsystem,
            Section::Drones,
            Section::Cargo,
            Section::Other
        ]
    );
}

#[test]
fn a_single_bay_is_drones_or_cargo() {
    let fit = eft::parse("[Vexor, Ratting]\nDrone Damage Amplifier II\n\n\nHammerhead II x5\n")
        .expect("a fit");
    assert_eq!(group(&fit, Section::DronesOrCargo).filled(), 5);
    assert!(fit.group(Section::Drones).is_none());
}

#[test]
fn empty_slot_markers_place_sections_when_one_is_missing() {
    // No low slots at all: the first block is the mids, as its marker says.
    let fit = eft::parse(
        "[Ibis, Starter]\n[Empty Med slot]\nCivilian Afterburner\n\nCivilian Gatling Railgun\n",
    )
    .expect("a fit");
    assert!(fit.group(Section::Low).is_none());
    let mid = group(&fit, Section::Mid);
    assert_eq!((mid.filled(), mid.empty), (1, 1));
    assert_eq!(group(&fit, Section::High).filled(), 1);
}

#[test]
fn skips_mutated_module_details_and_their_references() {
    let fit = eft::parse(
        "[Rifter, Abyssal]\nGyrostabilizer II [1]\n\n\n\n\n\n[1] Abyssal Gyrostabilizer\n  Damage Modifier 1.12\n",
    )
    .expect("a fit");
    assert_eq!(
        group(&fit, Section::Low).items,
        vec![item("Gyrostabilizer II", None, 1)]
    );
    assert_eq!(fit.groups.len(), 1);
}

#[test]
fn tolerates_spacing_and_odd_header_names() {
    let fit =
        eft::parse("[ Rifter ,  PvP, solo, v2 ]  \n\u{a0}Damage Control II  \n").expect("a fit");
    assert_eq!(fit.ship, "Rifter");
    assert_eq!(fit.name, "PvP, solo, v2");
    assert_eq!(group(&fit, Section::Low).filled(), 1);
    let unnamed = eft::parse("[Rifter,]\nDamage Control II\n").expect("a fit");
    assert_eq!(unnamed.name, "");
}

#[test]
fn rejects_text_that_is_not_a_fit() {
    for text in [
        "",
        "hello",
        "[Rifter, Tackle]",
        "[Rifter, Tackle]\n\n\n",
        "[Rifter]\nDamage Control II",
        "[, Tackle]\nDamage Control II",
        "[Empty Low slot]\nDamage Control II",
        "look at this [Rifter, Tackle]\nDamage Control II",
    ] {
        assert!(eft::parse(text).is_none(), "{text:?}");
    }
}

#[test]
fn is_bounded() {
    let long = format!("[Rifter, x]\n{}", "Damage Control II\n".repeat(300));
    assert!(long.len() > MAX_TEXT_BYTES || long.lines().count() > MAX_LINES);
    assert!(eft::parse(&long).is_none());
    let names = format!(
        "[{}, {}]\n{}\n",
        "S".repeat(60),
        "N".repeat(150),
        "M".repeat(150)
    );
    let fit = eft::parse(&names).expect("a fit");
    assert_eq!(fit.name.chars().count(), eft::MAX_NAME_CHARS);
    assert_eq!(
        fit.groups[0].items[0].name.chars().count(),
        eft::MAX_NAME_CHARS
    );
    // Huge quantities saturate instead of overflowing.
    let fit = eft::parse("[Rifter, x]\nA\n\nB x99999999999\n\nB x4294967295\n").expect("a fit");
    assert_eq!(group(&fit, Section::Drones).filled(), u32::MAX);
}

#[test]
fn never_panics_on_odd_input() {
    let bits = [
        "[",
        "]",
        ",",
        " x",
        "x5",
        "\n",
        "\r\n",
        "/OFFLINE",
        "[Empty Low slot]",
        " [1]",
        "é",
        "[1]",
        "Ü x",
        "\u{a0}",
    ];
    // Every pair after a valid header, plus each bit as the whole text.
    for a in bits {
        assert!(eft::parse(a).is_none() || a.starts_with('['));
        for b in bits {
            let _ = eft::parse(&format!("[Rifter, x]\n{a}{b}\n{b}{a}"));
            let _ = eft::parse(&format!("{a}{b}"));
        }
    }
}

// --- Card and dialog -------------------------------------------------------------------------

#[derive(Default)]
struct Fake {
    cards: Vec<(String, El)>,
    dialog: Option<El>,
    dialog_titles: Vec<(String, Option<String>)>,
    open_refused: bool,
    warnings: Vec<String>,
}

impl Host for Fake {
    fn show_card(&mut self, id: &str, tree: El) {
        self.cards.push((id.to_owned(), tree));
    }

    fn show_dialog(&mut self, tree: El) {
        self.dialog = Some(tree);
    }

    fn open_dialog(&mut self, title: &str, subtitle: Option<&str>) -> Result<(), Error> {
        if self.open_refused {
            return Err(Error::NotPermitted);
        }
        self.dialog_titles
            .push((title.to_owned(), subtitle.map(str::to_owned)));
        Ok(())
    }

    fn warn(&mut self, message: &str) {
        self.warnings.push(message.to_owned());
    }
}

impl Fake {
    fn last_card(&self, id: &str) -> &El {
        &self
            .cards
            .iter()
            .rev()
            .find(|(i, _)| i == id)
            .unwrap_or_else(|| panic!("no card {id}"))
            .1
    }
}

/// All texts, chip labels and button labels of a tree, in order.
fn texts(el: &El) -> Vec<String> {
    let mut out = Vec::new();
    collect(el, &mut out);
    out
}

fn collect(el: &El, out: &mut Vec<String>) {
    match &el.component {
        Component::Text(t) => out.push(t.text.clone()),
        Component::Chip(c) => out.push(c.text.clone()),
        Component::Button(b) => out.push(format!("[{}]", b.label)),
        _ => {}
    }
    for child in &el.children {
        collect(child, out);
    }
}

#[test]
fn a_pasted_fit_shows_a_card_with_the_summary() {
    let mut app = App::default();
    let mut host = Fake::default();
    app.on_chat_match(&mut host, "Alice", RIFTER);
    assert_eq!(host.cards.len(), 1);
    assert_eq!(
        texts(host.last_card(&card_id(1))),
        [
            "Rifter",
            "Tackle · pasted by Alice",
            "[Open]",
            "Low 2/3",
            "Mid 3/3",
            "High 3/4",
            "Rigs 2/3",
            "Drones 3",
            "Cargo 1050",
            "3× 200mm AutoCannon II · 2× Small Projectile Burst Aerator I",
        ]
    );
}

#[test]
fn text_that_is_no_fit_shows_nothing() {
    let mut app = App::default();
    let mut host = Fake::default();
    app.on_chat_match(&mut host, "Alice", "[Rifter, Tackle]\n\n");
    assert!(host.cards.is_empty());
}

#[test]
fn open_shows_the_full_fit_in_the_dialog() {
    let mut app = App::default();
    let mut host = Fake::default();
    app.on_chat_match(&mut host, "Alice", LEGION);
    app.on_card_action(&mut host, &card_id(1), view::ACTION_OPEN);
    assert_eq!(
        host.dialog_titles,
        [(
            "Armor HAM".to_owned(),
            Some("Legion · pasted by Alice".to_owned())
        )]
    );
    let lines = texts(host.dialog.as_ref().expect("dialog content"));
    for expected in [
        "Low 4/4",
        "2× Heat Sink II",
        "1× Stasis Webifier II (offline)",
        "2× Heavy Assault Missile Launcher II · Scourge Rage Heavy Assault Missile",
        "Subsystems 4/4",
        "Drones 5",
        "5× Hammerhead II",
        "Other 1",
        "2000× Scourge Rage Heavy Assault Missile",
    ] {
        assert!(
            lines.iter().any(|l| l == expected),
            "{expected} in {lines:?}"
        );
    }
}

#[test]
fn the_dialog_lists_empty_slots() {
    let mut app = App::default();
    let mut host = Fake::default();
    app.on_chat_match(&mut host, "Alice", RIFTER);
    app.on_card_action(&mut host, &card_id(1), view::ACTION_OPEN);
    let lines = texts(host.dialog.as_ref().expect("dialog content"));
    assert_eq!(lines.iter().filter(|l| *l == "1 empty slot").count(), 3);
}

#[test]
fn a_refused_dialog_is_logged_and_shows_nothing() {
    let mut app = App::default();
    let mut host = Fake {
        open_refused: true,
        ..Fake::default()
    };
    app.on_chat_match(&mut host, "Alice", RIFTER);
    app.on_card_action(&mut host, &card_id(1), view::ACTION_OPEN);
    assert!(host.dialog.is_none());
    assert_eq!(host.warnings.len(), 1);
}

#[test]
fn old_cards_lose_their_open_button() {
    let mut app = App::default();
    let mut host = Fake::default();
    for _ in 0..=KEPT_FITS {
        app.on_chat_match(&mut host, "Alice", RIFTER);
    }
    let first = texts(host.last_card(&card_id(1)));
    assert!(!first.iter().any(|t| t == "[Open]"), "{first:?}");
    app.on_card_action(&mut host, &card_id(1), view::ACTION_OPEN);
    assert!(host.dialog.is_none());
    app.on_card_action(&mut host, &card_id(2), view::ACTION_OPEN);
    assert!(host.dialog.is_some());
}

#[test]
fn unknown_cards_and_actions_are_ignored() {
    let mut app = App::default();
    let mut host = Fake::default();
    app.on_chat_match(&mut host, "Alice", RIFTER);
    app.on_card_action(&mut host, &card_id(1), "delete");
    app.on_card_action(&mut host, "poll-1", view::ACTION_OPEN);
    app.on_card_action(&mut host, &card_id(9), view::ACTION_OPEN);
    assert!(host.dialog.is_none());
}

#[test]
fn a_language_change_renders_cards_and_the_open_dialog_again() {
    let mut app = App::default();
    let mut host = Fake::default();
    app.on_chat_match(&mut host, "Alice", RIFTER);
    app.on_card_action(&mut host, &card_id(1), view::ACTION_OPEN);
    app.on_event(&mut host, &Event::LanguageChanged("de".into()));
    assert_eq!(host.cards.len(), 2);
    assert!(texts(host.last_card(&card_id(1))).contains(&"[Öffnen]".to_owned()));
    assert_eq!(host.dialog_titles.len(), 2);
    app.on_dialog_closed();
    app.on_event(&mut host, &Event::LanguageChanged("en".into()));
    assert_eq!(host.dialog_titles.len(), 2);
}

#[test]
fn all_languages_have_every_key() {
    let english: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(include_str!("../i18n/en.json")).expect("en.json");
    for tag in mumcord_plugin_sdk::i18n::LANGUAGES {
        let path = format!("{}/i18n/{tag}.json", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).expect("language file");
        let map: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&text).expect("valid JSON");
        let mut strings = Strings::new(&[("en", include_str!("../i18n/en.json")), (tag, &text)]);
        strings.set_language(tag);
        for key in english.keys().filter(|k| !k.starts_with("item.empty_")) {
            assert!(map.contains_key(key), "{tag} lacks {key}");
        }
        for count in [0, 1, 2, 3, 5, 11, 21, 101] {
            let line = strings.tn("item.empty", count, &[]);
            assert!(line.contains(&count.to_string()), "{tag} {count}: {line}");
        }
    }
}
