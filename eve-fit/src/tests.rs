//! Native tests of the EFT parser and the card/dialog logic against a recording fake host.

use mumcord_plugin_sdk::{
    events::Event,
    i18n::Strings,
    types::Error,
    ui::{Component, El},
};

use crate::app::{App, Host, KEPT_FITS, WEBVIEW_READY, card_id};
use crate::eft::{self, Item, MAX_LINES, MAX_TEXT_BYTES, Section};
use crate::history::{self, INDEX_KEY, MAX_ENTRIES, Store};
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
fn a_single_bay_is_split_by_the_data_or_stays_drones_or_cargo() {
    let fit = eft::parse(
        "[Vexor, Ratting]\nDrone Damage Amplifier II\n\n\nHammerhead II x5\nNanite Repair Paste x50\n",
    )
    .expect("a fit");
    assert_eq!(
        group(&fit, Section::Drones).items,
        [item("Hammerhead II", None, 5)]
    );
    assert_eq!(
        group(&fit, Section::Cargo).items,
        [item("Nanite Repair Paste", None, 50)]
    );
    assert!(fit.group(Section::DronesOrCargo).is_none());

    // Names the data does not know (here a made-up one) leave it open.
    let fit = eft::parse("[Vexor, Ratting]\nDrone Damage Amplifier II\n\n\nMystery Crate x5\n")
        .expect("a fit");
    assert_eq!(group(&fit, Section::DronesOrCargo).filled(), 5);
    assert!(fit.group(Section::Drones).is_none());
}

/// A real paste whose blank lines got lost in the chat composer.
const MAMBA_WITHOUT_BLANK_LINES: &str = "[Mamba, *Mamba Class 2/3]
Drone Damage Amplifier II
Drone Damage Amplifier II
Pithum C-Type Medium Shield Booster
Pithum C-Type EM Shield Amplifier
Small Capacitor Booster II
Pith X-Type Explosive Shield Hardener
Rocket Launcher II
Rocket Launcher II
Rocket Launcher II
Rocket Launcher II
Rocket Launcher II
Small EM Shield Reinforcer II
Small Explosive Shield Reinforcer II
Small Thermal Shield Reinforcer II
Federation Navy Hobgoblin x2
'Augmented' Hobgoblin x2
Inferno Rage Rocket x5000
Navy Cap Booster 400 x16
Inferno Javelin Rocket x5000
Pith X-Type Explosive Shield Hardener x1
Mobile Depot x1";

#[test]
fn sorts_a_fit_without_blank_lines_by_the_data() {
    let fit = eft::parse(MAMBA_WITHOUT_BLANK_LINES).expect("a fit");
    let sections: Vec<Section> = fit.groups.iter().map(|g| g.section).collect();
    assert_eq!(
        sections,
        [
            Section::Low,
            Section::Mid,
            Section::High,
            Section::Rig,
            Section::Drones,
            Section::Cargo
        ]
    );
    assert_eq!(
        group(&fit, Section::Low).items,
        [item("Drone Damage Amplifier II", None, 2)]
    );
    assert_eq!(group(&fit, Section::Mid).filled(), 4);
    assert_eq!(
        group(&fit, Section::High).items,
        [item("Rocket Launcher II", None, 5)]
    );
    assert_eq!(group(&fit, Section::Rig).filled(), 3);
    assert_eq!(
        group(&fit, Section::Drones).items,
        [
            item("Federation Navy Hobgoblin", None, 2),
            item("'Augmented' Hobgoblin", None, 2)
        ]
    );
    assert_eq!(
        group(&fit, Section::Cargo).items,
        [
            item("Inferno Rage Rocket", None, 5000),
            item("Navy Cap Booster 400", None, 16),
            item("Inferno Javelin Rocket", None, 5000),
            item("Pith X-Type Explosive Shield Hardener", None, 1),
            item("Mobile Depot", None, 1)
        ]
    );
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
    /// Whether the host shows web views; off by default, like a host without `ui.webview`.
    webview: bool,
    webview_shown: usize,
    posted: Vec<String>,
    store: std::collections::BTreeMap<String, Vec<u8>>,
    panel: Option<El>,
    popovers: usize,
    /// Seconds since 1970 and the UTC offset in minutes.
    now: (u64, i32),
    /// Whether events are replayed from chat history.
    replay: bool,
    /// Card ids that replaced a chat message, and how many cards were shown at that moment.
    replaced: Vec<(String, usize)>,
}

impl Store for Fake {
    fn load(&mut self, key: &str) -> Option<Vec<u8>> {
        self.store.get(key).cloned()
    }

    fn save(&mut self, key: &str, value: &[u8]) {
        self.store.insert(key.to_owned(), value.to_owned());
    }

    fn remove(&mut self, key: &str) {
        self.store.remove(key);
    }
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

    fn show_webview(&mut self) -> Result<(), Error> {
        if !self.webview {
            return Err(Error::NotPermitted);
        }
        self.webview_shown += 1;
        Ok(())
    }

    fn post_webview(&mut self, message: &str) -> Result<(), Error> {
        self.posted.push(message.to_owned());
        Ok(())
    }

    fn show_panel(&mut self, tree: El) {
        self.panel = Some(tree);
    }

    fn open_popover(&mut self) {
        self.popovers += 1;
    }

    fn now(&mut self) -> (u64, i32) {
        self.now
    }

    fn warn(&mut self, message: &str) {
        self.warnings.push(message.to_owned());
    }

    fn is_replay(&mut self) -> bool {
        self.replay
    }

    fn replace_message(&mut self, id: &str) {
        self.replaced.push((id.to_owned(), self.cards.len()));
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
        Component::CopyButton(b) => out.push(format!("[{}: {} bytes]", b.label, b.text.len())),
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
            &format!("[Copy fitting: {} bytes]", RIFTER.len()),
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

/// The text of the first copy button in a tree.
fn copy_text(el: &El) -> Option<&str> {
    if let Component::CopyButton(b) = &el.component {
        return Some(&b.text);
    }
    el.children.iter().find_map(copy_text)
}

#[test]
fn copy_fitting_copies_the_text_as_pasted() {
    let mut app = App::default();
    let mut host = Fake::default();
    app.on_chat_match(&mut host, "Alice", LEGION);
    assert_eq!(copy_text(host.last_card(&card_id(1))), Some(LEGION));
    app.on_card_action(&mut host, &card_id(1), view::ACTION_OPEN);
    assert_eq!(
        copy_text(host.dialog.as_ref().expect("dialog")),
        Some(LEGION)
    );
}

#[test]
fn text_that_is_no_fit_shows_nothing() {
    let mut app = App::default();
    let mut host = Fake::default();
    app.on_chat_match(&mut host, "Alice", "[Rifter, Tackle]\n\n");
    assert!(host.cards.is_empty());
    assert!(host.replaced.is_empty());
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
    assert!(!first.iter().any(|t| t.starts_with('[')), "{first:?}");
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

// --- Fitting wheel web view -------------------------------------------------------------------

fn web_host() -> Fake {
    Fake {
        webview: true,
        ..Fake::default()
    }
}

fn posted(host: &Fake) -> serde_json::Value {
    serde_json::from_str(host.posted.last().expect("a message to the page")).expect("valid JSON")
}

#[test]
fn open_shows_the_fitting_wheel_and_sends_the_fit_when_the_page_is_ready() {
    let mut app = App::default();
    let mut host = web_host();
    app.on_chat_match(&mut host, "Bob", RIFTER);
    app.on_card_action(&mut host, &card_id(1), view::ACTION_OPEN);
    assert_eq!(host.webview_shown, 1);
    assert_eq!(host.dialog_titles.len(), 1);
    // The native list is only the fallback; nothing goes to the page before it asks.
    assert!(host.dialog.is_none());
    assert!(host.posted.is_empty());

    app.on_webview_message(&mut host, WEBVIEW_READY);
    let message = posted(&host);
    assert_eq!(message["fit"], RIFTER);
    assert_eq!(message["strings"]["lang"], "en");
    assert_eq!(message["strings"]["loading"], "Loading the fitting wheel…");
    assert_eq!(message["strings"]["wheel"], "Fitting wheel");
}

#[test]
fn without_a_web_view_the_dialog_shows_the_list() {
    let mut app = App::default();
    let mut host = Fake::default();
    app.on_chat_match(&mut host, "Bob", RIFTER);
    app.on_card_action(&mut host, &card_id(1), view::ACTION_OPEN);
    assert!(host.dialog.is_some());
    assert_eq!(host.warnings.len(), 1);
    // A page that is not there cannot ask for the fit.
    app.on_webview_message(&mut host, WEBVIEW_READY);
    assert!(host.posted.is_empty());
}

#[test]
fn the_page_gets_nothing_for_other_messages_or_after_the_dialog_closed() {
    let mut app = App::default();
    let mut host = web_host();
    app.on_chat_match(&mut host, "Bob", RIFTER);
    app.on_webview_message(&mut host, WEBVIEW_READY);
    app.on_card_action(&mut host, &card_id(1), view::ACTION_OPEN);
    app.on_webview_message(&mut host, "copy");
    app.on_webview_message(&mut host, "{\"fit\":\"x\"}");
    assert!(host.posted.is_empty());
    app.on_dialog_closed();
    app.on_webview_message(&mut host, WEBVIEW_READY);
    assert!(host.posted.is_empty());
}

#[test]
fn a_language_change_sends_the_page_its_new_texts() {
    let mut app = App::default();
    let mut host = web_host();
    app.on_chat_match(&mut host, "Bob", RIFTER);
    app.on_card_action(&mut host, &card_id(1), view::ACTION_OPEN);
    app.on_event(&mut host, &Event::LanguageChanged("de".into()));
    let message = posted(&host);
    assert_eq!(message["strings"]["lang"], "de");
    assert_eq!(message["strings"]["licences"], "Lizenzen");
    assert_eq!(message["fit"], RIFTER);
    // The page stays; only the title is set again.
    assert_eq!(host.webview_shown, 1);
    assert_eq!(host.dialog_titles.len(), 2);
    assert!(host.dialog.is_none());
}

#[test]
fn the_page_message_escapes_the_pasted_text_and_fits_the_bridge() {
    let strings = Strings::new(mumcord_plugin_sdk::i18n_all!());
    let odd = "[Rifter, \"Q\\uote\"]\r\n\tDamage Control II\u{1}\u{7f} é 漢";
    let message = view::webview_message(&strings, "en", odd);
    let value: serde_json::Value = serde_json::from_str(&message).expect("valid JSON");
    assert_eq!(value["fit"], odd);
    // A fit is at most 4 KiB; even all newlines stay well within the 16 KiB bridge limit.
    let long = "\n".repeat(4096);
    assert!(view::webview_message(&strings, "en", &long).len() < 16 * 1024);
}

// --- Fit history --------------------------------------------------------------------------------

/// 2026-11-01 14:32 UTC.
const NOW: u64 = 1_793_543_520;

fn panel_host() -> Fake {
    Fake {
        now: (NOW, 0),
        ..Fake::default()
    }
}

/// An app with the panel granted and shown as the large popover.
fn panel_app(host: &mut Fake) -> App {
    let mut app = App::default();
    app.start(host);
    app.set_granted(host, &["ui.panel".to_owned()]);
    app.on_panel_visibility(host, true);
    app
}

fn panel_texts(host: &Fake) -> Vec<String> {
    texts(host.panel.as_ref().expect("a panel"))
}

/// A small fit whose name is `name`.
fn small_fit(name: &str) -> String {
    format!("[Rifter, {name}]\nDamage Control II\n")
}

#[test]
fn the_history_is_collapsed_until_unfolded() {
    let mut host = panel_host();
    let mut app = panel_app(&mut host);
    app.on_chat_match(&mut host, "Bob", RIFTER);
    assert_eq!(panel_texts(&host), ["[\u{25b8} Fit history (1)]"]);
    app.on_panel_action(&mut host, view::ACTION_TOGGLE_HISTORY);
    assert_eq!(
        panel_texts(&host),
        [
            "[\u{25be} Fit history (1)]".to_owned(),
            "[Rifter · Tackle]".to_owned(),
            "Bob · 14:32".to_owned(),
            format!("[Copy fitting: {} bytes]", RIFTER.len()),
        ]
    );
    app.on_panel_action(&mut host, view::ACTION_TOGGLE_HISTORY);
    assert_eq!(panel_texts(&host).len(), 1);
}

#[test]
fn the_header_announces_whether_the_history_is_open() {
    let mut host = panel_host();
    let mut app = panel_app(&mut host);
    let expanded = |host: &Fake| match &host.panel.as_ref().expect("a panel").children[0].component
    {
        Component::Button(b) => b.expanded,
        _ => panic!("no header button"),
    };
    assert_eq!(expanded(&host), Some(false));
    app.on_panel_action(&mut host, view::ACTION_TOGGLE_HISTORY);
    assert_eq!(expanded(&host), Some(true));
}

#[test]
fn the_newest_fit_comes_first_and_duplicates_move_up() {
    let mut host = panel_host();
    let mut app = panel_app(&mut host);
    app.on_panel_action(&mut host, view::ACTION_TOGGLE_HISTORY);
    app.on_chat_match(&mut host, "Alice", &small_fit("One"));
    app.on_chat_match(&mut host, "Bob", &small_fit("Two"));
    // The same fit again, with other line endings and trailing spaces.
    app.on_chat_match(
        &mut host,
        "Carol",
        "[Rifter, One]  \r\nDamage Control II \r\n",
    );
    let shown: Vec<String> = panel_texts(&host)
        .into_iter()
        .filter(|t| !t.starts_with("[Copy"))
        .collect();
    assert_eq!(
        shown,
        [
            "[\u{25be} Fit history (2)]",
            "[Rifter · One]",
            "Carol · 14:32",
            "[Rifter · Two]",
            "Bob · 14:32",
        ]
    );
}

#[test]
fn opened_fits_join_the_history() {
    let mut host = panel_host();
    let mut app = panel_app(&mut host);
    app.on_chat_match(&mut host, "Alice", &small_fit("One"));
    app.on_chat_match(&mut host, "Bob", &small_fit("Two"));
    host.now.0 += 60;
    app.on_card_action(&mut host, &card_id(1), view::ACTION_OPEN);
    app.on_panel_action(&mut host, view::ACTION_TOGGLE_HISTORY);
    assert_eq!(
        panel_texts(&host)[1..3],
        ["[Rifter · One]", "Alice · 14:33"]
    );
}

#[test]
fn clicking_an_entry_opens_the_fit() {
    let mut host = Fake {
        webview: true,
        ..panel_host()
    };
    let mut app = panel_app(&mut host);
    app.on_chat_match(&mut host, "Bob", RIFTER);
    let action = format!(
        "{}{:016x}",
        view::HISTORY_OPEN_PREFIX,
        history::hash(RIFTER)
    );
    app.on_panel_action(&mut host, &action);
    assert_eq!(
        host.dialog_titles,
        [(
            "Tackle".to_owned(),
            Some("Rifter · pasted by Bob".to_owned())
        )]
    );
    app.on_webview_message(&mut host, WEBVIEW_READY);
    assert_eq!(posted(&host)["fit"], RIFTER);
}

#[test]
fn unknown_entries_and_actions_open_nothing() {
    let mut host = panel_host();
    let mut app = panel_app(&mut host);
    app.on_chat_match(&mut host, "Bob", RIFTER);
    for action in [
        "history:open:ffff",
        "history:open:zz",
        "history:open:",
        "nope",
    ] {
        app.on_panel_action(&mut host, action);
    }
    assert!(host.dialog_titles.is_empty());
}

#[test]
fn the_history_survives_a_restart() {
    let mut host = panel_host();
    let mut app = panel_app(&mut host);
    app.on_chat_match(&mut host, "Alice", &small_fit("One"));
    app.on_chat_match(&mut host, "Bob", RIFTER);
    let mut host = Fake {
        store: host.store,
        ..panel_host()
    };
    let mut app = panel_app(&mut host);
    app.on_panel_action(&mut host, view::ACTION_TOGGLE_HISTORY);
    let shown = panel_texts(&host);
    assert_eq!(shown[1..3], ["[Rifter · Tackle]", "Bob · 14:32"]);
    assert_eq!(shown[4..6], ["[Rifter · One]", "Alice · 14:32"]);
}

#[test]
fn the_history_keeps_the_newest_fifty_and_their_texts_only() {
    let mut host = panel_host();
    let mut app = panel_app(&mut host);
    for i in 0..MAX_ENTRIES + 5 {
        app.on_chat_match(&mut host, "Alice", &small_fit(&i.to_string()));
    }
    // One key per fit plus the index.
    assert_eq!(host.store.len(), MAX_ENTRIES + 1);
    let mut host = Fake {
        store: host.store,
        ..panel_host()
    };
    let mut app = panel_app(&mut host);
    app.on_panel_action(&mut host, view::ACTION_TOGGLE_HISTORY);
    assert_eq!(panel_texts(&host)[0], "[\u{25be} Fit history (50)]");
    assert_eq!(panel_texts(&host)[1], "[Rifter · 54]");
}

#[test]
fn a_broken_store_is_skipped() {
    let mut host = panel_host();
    host.save(INDEX_KEY, b"[{\"hash\":\"zz\",\"sender\":\"x\",\"time\":1},{\"hash\":\"00000000000000ff\",\"sender\":\"x\",\"time\":1}]");
    host.save("fit/00000000000000ff", b"not a fit");
    let mut app = panel_app(&mut host);
    app.on_panel_action(&mut host, view::ACTION_TOGGLE_HISTORY);
    assert_eq!(panel_texts(&host)[0], "[\u{25be} Fit history (0)]");
    host.save(INDEX_KEY, b"{garbage");
    let mut app = panel_app(&mut host);
    app.on_panel_action(&mut host, view::ACTION_TOGGLE_HISTORY);
    assert!(
        panel_texts(&host)
            .contains(&"No fits yet. Fits pasted in the chat appear here.".to_owned())
    );
}

#[test]
fn large_histories_are_paged_to_fit_the_slot() {
    let mut host = panel_host();
    let mut app = panel_app(&mut host);
    for i in 0..MAX_ENTRIES {
        // About 1.5 KiB each, like a real fit.
        let fit = format!("[Rifter, {i}]\n{}", "Damage Control II\n".repeat(80));
        app.on_chat_match(&mut host, "Alice", &fit);
    }
    app.on_panel_action(&mut host, view::ACTION_TOGGLE_HISTORY);
    let mut seen = 0;
    let mut pages = 0;
    loop {
        let shown = panel_texts(&host);
        let copies: usize = shown.iter().filter(|t| t.starts_with("[Copy")).count();
        let bytes: usize = copy_bytes(host.panel.as_ref().expect("a panel"));
        assert!(
            copies >= 1 && bytes <= 10 * 1024,
            "{copies} fits, {bytes} bytes"
        );
        seen += copies;
        pages += 1;
        if !shown.contains(&"[Older fits]".to_owned()) {
            break;
        }
        app.on_panel_action(&mut host, view::ACTION_OLDER);
        assert!(panel_texts(&host).contains(&"[Newer fits]".to_owned()));
    }
    assert_eq!(seen, MAX_ENTRIES);
    for _ in 1..pages {
        app.on_panel_action(&mut host, view::ACTION_NEWER);
    }
    assert!(!panel_texts(&host).contains(&"[Newer fits]".to_owned()));
    assert_eq!(panel_texts(&host)[1], "[Rifter · 49]");
}

fn copy_bytes(el: &El) -> usize {
    let own = match &el.component {
        Component::CopyButton(b) => b.text.len(),
        _ => 0,
    };
    own + el.children.iter().map(copy_bytes).sum::<usize>()
}

#[test]
fn the_inline_panel_shows_the_last_fit_and_opens_the_history() {
    let mut host = panel_host();
    let mut app = App::default();
    app.start(&mut host);
    app.set_granted(&mut host, &["ui.panel".to_owned()]);
    assert_eq!(
        panel_texts(&host),
        [
            "No fits yet. Fits pasted in the chat appear here.",
            "[Fit history]"
        ]
    );
    app.on_chat_match(&mut host, "Bob", RIFTER);
    assert_eq!(panel_texts(&host)[0], "Last fit: Rifter · Tackle");
    app.on_panel_action(&mut host, view::ACTION_SHOW_HISTORY);
    assert_eq!(host.popovers, 1);
    app.on_panel_visibility(&mut host, true);
    assert_eq!(panel_texts(&host)[0], "[\u{25be} Fit history (1)]");
}

#[test]
fn without_the_panel_permission_nothing_is_shown_but_fits_are_kept() {
    let mut host = panel_host();
    let mut app = App::default();
    app.start(&mut host);
    app.on_chat_match(&mut host, "Bob", RIFTER);
    assert!(host.panel.is_none());
    assert!(host.store.contains_key(INDEX_KEY));
}

#[test]
fn times_from_earlier_days_show_the_date_in_local_time() {
    assert_eq!(history::format_time(NOW, NOW, 0), "14:32");
    assert_eq!(history::format_time(NOW, NOW, 60), "15:32");
    assert_eq!(
        history::format_time(NOW - 86_400, NOW, 0),
        "2026-10-31 14:32"
    );
    assert_eq!(history::format_time(0, NOW, 0), "1970-01-01 00:00");
    assert_eq!(
        history::format_time(951_782_400, NOW, 0),
        "2000-02-29 00:00"
    );
}

// --- Card in place of the message and replay ---------------------------------------------------

#[test]
fn the_card_replaces_the_pasted_message_before_it_is_shown() {
    let mut app = App::default();
    let mut host = Fake::default();
    app.on_chat_match(&mut host, "Alice", RIFTER);
    assert_eq!(host.replaced, [(card_id(1), 0)]);
    assert_eq!(host.cards.len(), 1);
}

#[test]
fn a_replayed_fit_gets_a_card_but_stays_out_of_the_history() {
    let mut host = panel_host();
    let mut app = panel_app(&mut host);
    app.on_chat_match(&mut host, "Bob", &small_fit("First"));
    app.on_chat_match(&mut host, "Bob", &small_fit("Second"));
    let stored = host.store.clone();

    host.replay = true;
    app.on_chat_match(&mut host, "Bob", &small_fit("First"));
    app.on_chat_match(&mut host, "Carol", &small_fit("Old"));
    assert_eq!(host.store, stored);
    assert_eq!(panel_texts(&host), ["[\u{25b8} Fit history (2)]"]);
    assert_eq!(
        host.replaced
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>(),
        ["fit-1", "fit-2", "fit-3", "fit-4"]
    );
    assert!(texts(host.last_card("fit-4")).contains(&"Old · pasted by Carol".to_owned()));
}
