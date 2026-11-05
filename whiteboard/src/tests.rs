//! Glue tests against a recording fake host. The mock host of `mumcord-plugin test` cannot send
//! stage, app-menu or web view events yet, so the flows around the board run here: two plugin
//! instances wired through a simulated channel.

use std::collections::HashMap;

use mumcord_plugin_sdk::chat::PostOutcome;
use mumcord_plugin_sdk::events::{
    ActionSource, ActionValue, Event, PeerMessage, PluginSettings, UiAction, Visibility,
};
use mumcord_plugin_sdk::types::{Error, Peer};
use mumcord_plugin_sdk::ui::{Component, El, Slot};
use mumcord_plugin_sdk::webview::Place;
use serde_json::Value;

use crate::app::{App, Host, KIND_LINE, Level, MINI_ENTRY, STAGE_ENTRY, TICK_MS};
use crate::codec;
use crate::store::Storage;
use crate::sync::{PeerInfo, SessionId, Target};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest-of-a-picture";

#[derive(Default)]
struct Fake {
    wall: u64,
    store: HashMap<String, Vec<u8>>,
    rng: u8,
    me: Option<PeerInfo>,
    others: Vec<PeerInfo>,
    sent: Vec<(Target, u8, Vec<u8>)>,
    stage_started: Vec<String>,
    stage_error: Option<Error>,
    joins: Vec<String>,
    web: Vec<(Place, String)>,
    posted: Vec<String>,
    /// Slot and the texts of its tree, newest last.
    trees: Vec<(Slot, Vec<String>)>,
    dialogs: Vec<String>,
    chat: Vec<String>,
    chat_error: Option<Error>,
    images: Vec<usize>,
    image_results: Vec<Result<PostOutcome, Error>>,
    files: Vec<(String, Vec<u8>)>,
    save_error: Option<Error>,
    supersedes: Vec<(String, SessionId, u32)>,
    logs: Vec<String>,
}

fn texts(el: El) -> Vec<String> {
    el.into_nodes()
        .into_iter()
        .filter_map(|n| match n.component {
            Component::Text(t) => Some(t.text),
            Component::Button(b) => Some(b.label),
            Component::Chip(c) => Some(c.text),
            Component::Toggle(t) => Some(t.label),
            _ => None,
        })
        .collect()
}

impl Storage for Fake {
    fn get(&mut self, key: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(self.store.get(key).cloned())
    }
    fn set(&mut self, key: &str, value: &[u8]) -> Result<(), String> {
        self.store.insert(key.to_owned(), value.to_vec());
        Ok(())
    }
    fn delete(&mut self, key: &str) -> Result<(), String> {
        self.store.remove(key);
        Ok(())
    }
    fn list_keys(&mut self, prefix: &str) -> Result<Vec<String>, String> {
        let mut keys: Vec<String> = self
            .store
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        keys.sort();
        Ok(keys)
    }
}

impl Host for Fake {
    fn wall_ms(&mut self) -> u64 {
        // The real clock has a 1 s resolution.
        self.wall / 1000 * 1000
    }
    fn random(&mut self, len: u32) -> Option<Vec<u8>> {
        let me = self.me.as_ref().map_or(0, |m| m.session as u8);
        Some(
            (0..len)
                .map(|i| {
                    self.rng = self.rng.wrapping_add(1);
                    self.rng ^ (i as u8) ^ me.wrapping_mul(97)
                })
                .collect(),
        )
    }
    fn presence(&mut self) -> Vec<PeerInfo> {
        self.me.iter().chain(&self.others).cloned().collect()
    }
    fn peers_send(&mut self, target: Target, kind: u8, payload: &[u8]) -> Result<(), Error> {
        self.sent.push((target, kind, payload.to_vec()));
        Ok(())
    }
    fn game_burst(&mut self) -> bool {
        true
    }
    fn relay_available(&mut self) -> bool {
        false
    }
    fn relay_send(&mut self, _target: Target, _payload: &[u8]) -> Result<(), Error> {
        Err(Error::RelayUnavailable)
    }
    fn stage_start(&mut self, title: &str, entry: &str) -> Result<String, Error> {
        if let Some(e) = self.stage_error {
            return Err(e);
        }
        assert_eq!(entry, STAGE_ENTRY);
        self.stage_started.push(title.to_owned());
        Ok("activity-1".to_owned())
    }
    fn stage_join(&mut self, activity: &str) -> Result<(), Error> {
        self.joins.push(activity.to_owned());
        Ok(())
    }
    fn webview_show(&mut self, place: Place, entry: &str) -> Result<(), Error> {
        self.web.push((place, entry.to_owned()));
        Ok(())
    }
    fn webview_post(&mut self, message: &str) -> Result<(), Error> {
        assert!(message.len() <= 64 * 1024);
        self.posted.push(message.to_owned());
        Ok(())
    }
    fn show(&mut self, slot: Slot, tree: El) -> Result<(), Error> {
        self.trees.push((slot, texts(tree)));
        Ok(())
    }
    fn open_dialog(&mut self, title: &str) -> Result<(), Error> {
        self.dialogs.push(title.to_owned());
        Ok(())
    }
    fn close_dialog(&mut self) {}
    fn chat_send(&mut self, text: &str) -> Result<(), Error> {
        match self.chat_error {
            Some(e) => Err(e),
            None => {
                self.chat.push(text.to_owned());
                Ok(())
            }
        }
    }
    fn line_hash(&mut self, text: &str) -> u32 {
        text.len() as u32
    }
    fn supersede(&mut self, card: &str, sender: SessionId, hash: u32) -> Result<(), Error> {
        self.supersedes.push((card.to_owned(), sender, hash));
        Ok(())
    }
    fn post_image(&mut self, png: &[u8], _alt: &str) -> Result<PostOutcome, Error> {
        self.images.push(png.len());
        if self.image_results.is_empty() {
            Ok(PostOutcome::Image)
        } else {
            self.image_results.remove(0)
        }
    }
    fn save_file(&mut self, name: &str, data: &[u8]) -> Result<bool, Error> {
        match self.save_error {
            Some(e) => Err(e),
            None => {
                self.files.push((name.to_owned(), data.to_vec()));
                Ok(true)
            }
        }
    }
    fn start_timer(&mut self, _ms: u32) -> Option<u32> {
        Some(7)
    }
    fn cancel_timer(&mut self, _id: u32) {}
    fn log(&mut self, _level: Level, message: &str) {
        self.logs.push(message.to_owned());
    }
}

fn info(session: u32, name: &str) -> PeerInfo {
    PeerInfo {
        session,
        name: name.to_owned(),
        key: Some(format!("{session:064x}")),
    }
}

fn peer(p: &PeerInfo) -> Peer {
    Peer {
        session: p.session,
        name: p.name.clone(),
        key: p.key.clone(),
    }
}

/// One user: the plugin and its host.
struct User {
    app: App,
    host: Fake,
}

impl User {
    fn new(session: u32, name: &str, others: &[(u32, &str)]) -> User {
        let host = Fake {
            wall: 1_700_000_000_000,
            me: Some(info(session, name)),
            others: others.iter().map(|(s, n)| info(*s, n)).collect(),
            ..Fake::default()
        };
        User {
            app: App::default(),
            host,
        }
    }

    fn me(&self) -> PeerInfo {
        self.host.me.clone().expect("me")
    }

    fn event(&mut self, e: Event) {
        self.app.on_event(&mut self.host, e);
    }

    /// A message from the stage page (pages tag their place in `p`).
    fn page(&mut self, msg: &str) {
        self.page_at("stage", msg);
    }

    fn page_at(&mut self, place: &str, msg: &str) {
        let tagged = match msg.strip_prefix('{') {
            Some(rest) => format!(r#"{{"p":"{place}",{rest}"#),
            None => msg.to_owned(),
        };
        self.event(Event::WebviewMessage(tagged));
    }

    fn click(&mut self, slot: Slot, action: &str, value: ActionValue) {
        self.event(Event::UiAction(UiAction {
            source: ActionSource::Slot(slot),
            action: action.to_owned(),
            value,
        }));
    }

    fn tick(&mut self) {
        self.host.wall += TICK_MS;
        self.event(Event::Timer(7));
    }

    fn posted_of(&self, t: &str) -> Vec<Value> {
        self.host
            .posted
            .iter()
            .filter_map(|m| serde_json::from_str::<Value>(m).ok())
            .filter(|v| v["t"] == t)
            .collect()
    }

    fn last_tree(&self, pred: impl Fn(&Slot) -> bool) -> Option<&Vec<String>> {
        self.host
            .trees
            .iter()
            .rev()
            .find(|(s, _)| pred(s))
            .map(|(_, t)| t)
    }

    fn starts_board(&mut self) {
        self.event(Event::AppMenuActivated("start".into()));
        self.page(r#"{"t":"ready"}"#);
    }
}

/// Moves every message `from` sent to the channel (or to `to`) into `to` as events.
fn deliver(from: &mut User, to: &mut User) {
    let sender = peer(&from.me());
    let me = to.me().session;
    for (target, kind, payload) in std::mem::take(&mut from.host.sent) {
        let direct = match target {
            Target::Channel => false,
            Target::Direct(s) if s == me => true,
            Target::Direct(_) => continue,
        };
        to.event(Event::PeerMessage(PeerMessage {
            sender: sender.clone(),
            kind,
            payload,
            direct,
        }));
    }
}

/// Alice starts a board, Bob joins it from the invite card and loads it.
fn two_on_a_board() -> (User, User) {
    let mut alice = User::new(1, "Alice", &[(2, "Bob")]);
    let mut bob = User::new(2, "Bob", &[(1, "Alice")]);
    alice.starts_board();
    bob.event(Event::StageActivityStarted((
        peer(&alice.me()),
        "activity-1".into(),
    )));
    deliver(&mut alice, &mut bob);
    let card = match bob.last_tree(|s| matches!(s, Slot::ChatCard(_))) {
        Some(_) => bob
            .host
            .trees
            .iter()
            .rev()
            .find_map(|(s, _)| match s {
                Slot::ChatCard(c) => Some(c.clone()),
                _ => None,
            })
            .expect("card"),
        None => panic!("Bob got no invite card"),
    };
    bob.click(Slot::ChatCard(card), "join", ActionValue::None);
    assert_eq!(bob.host.joins, vec!["activity-1".to_owned()]);
    let bob_peer = peer(&bob.me());
    bob.event(Event::StageJoined(bob_peer.clone()));
    alice.event(Event::StageJoined(bob_peer));
    bob.page(r#"{"t":"ready"}"#);
    settle(&mut alice, &mut bob, 40);
    (alice, bob)
}

/// Lets time pass and messages flow both ways.
fn settle(a: &mut User, b: &mut User, ticks: usize) {
    for _ in 0..ticks {
        a.tick();
        b.tick();
        deliver(a, b);
        deliver(b, a);
    }
}

fn upserted_texts(u: &User) -> Vec<String> {
    u.posted_of("objs")
        .iter()
        .flat_map(|m| m["upsert"].as_array().cloned().unwrap_or_default())
        .filter_map(|o| o["text"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn start_from_the_apps_menu_opens_the_stage_and_posts_an_invite_card() {
    let mut alice = User::new(1, "Alice", &[(2, "Bob")]);
    alice.event(Event::AppMenuActivated("start".into()));
    assert_eq!(alice.host.stage_started, vec!["Whiteboard".to_owned()]);
    let card = alice
        .last_tree(|s| matches!(s, Slot::ChatCard(_)))
        .expect("invite card");
    assert!(
        card.contains(&"You started a whiteboard".to_owned()),
        "{card:?}"
    );
    // Nothing goes to the page before it says it is ready.
    assert!(alice.host.posted.is_empty());
    alice.page(r#"{"t":"ready"}"#);
    let init = &alice.posted_of("init")[0];
    assert_eq!(init["board"]["title"], "Untitled board");
    assert_eq!(init["strings"]["app.title"], "Whiteboard");
    assert_eq!(init["settings"]["canDraw"], true);
    // The new board is announced to the channel (control messages share the send budget).
    for _ in 0..12 {
        alice.tick();
    }
    assert!(
        alice
            .host
            .sent
            .iter()
            .any(|(t, k, _)| *t == Target::Channel && *k == codec::KIND_BOARD)
    );
}

#[test]
fn the_slash_command_starts_too_and_a_refusal_is_explained() {
    let mut alice = User::new(1, "Alice", &[]);
    alice.host.stage_error = Some(Error::NotPermitted);
    alice.event(Event::UiAction(UiAction {
        source: ActionSource::Command,
        action: "whiteboard".into(),
        value: ActionValue::Text(String::new()),
    }));
    assert_eq!(
        alice.host.dialogs,
        vec!["Couldn't start the whiteboard".to_owned()]
    );
    let tree = alice
        .last_tree(|s| matches!(s, Slot::Dialog))
        .expect("dialog");
    assert!(tree[0].starts_with("Join a voice channel first"));
}

#[test]
fn a_peers_ops_update_the_page() {
    let (mut alice, mut bob) = two_on_a_board();
    assert!(
        bob.posted_of("init")
            .iter()
            .any(|i| i["board"]["title"] == "Untitled board"),
        "Bob never loaded the board"
    );
    alice.page(
        r#"{"t":"create","obj":{"k":"note","g":[10,20,156,112],"fill":2,"text":"Docs: update"}}"#,
    );
    // Alice's own page gets the note back from the plugin.
    assert!(upserted_texts(&alice).contains(&"Docs: update".to_owned()));
    settle(&mut alice, &mut bob, 20);
    assert!(upserted_texts(&bob).contains(&"Docs: update".to_owned()));
    let a = alice.posted_of("objs");
    let note = a
        .iter()
        .flat_map(|m| m["upsert"].as_array().cloned().unwrap_or_default())
        .find(|o| o["text"] == "Docs: update")
        .expect("note");
    assert_eq!(note["k"], "note");
    assert_eq!(note["g"][0], 10.0);
    assert_eq!(note["fill"], 2);
}

#[test]
fn page_edits_produce_peer_messages_within_the_rate_limit() {
    let (mut alice, mut bob) = two_on_a_board();
    alice.host.sent.clear();
    for i in 0..30 {
        alice.page(&format!(
            r#"{{"t":"create","obj":{{"k":"rect","g":[{},0,40,40],"ink":1,"w":1}}}}"#,
            i * 50
        ));
    }
    // Without a relay `peers.messages` allows a burst of 3, then one per 2 s.
    assert!(
        alice.host.sent.len() <= 3,
        "{} messages at once",
        alice.host.sent.len()
    );
    settle(&mut alice, &mut bob, 64);
    assert_eq!(bob.app_objects(), alice.app_objects());
    assert_eq!(alice.app_objects(), 30);
}

impl User {
    /// Objects the page holds, from the `objs` messages it got.
    fn app_objects(&self) -> usize {
        let mut ids = std::collections::BTreeSet::new();
        for m in self.posted_of("objs") {
            if m["reset"] == true {
                ids.clear();
            }
            for o in m["upsert"].as_array().cloned().unwrap_or_default() {
                ids.insert(o["id"].as_str().unwrap_or_default().to_owned());
            }
            for r in m["remove"].as_array().cloned().unwrap_or_default() {
                ids.remove(r.as_str().unwrap_or_default());
            }
        }
        ids.len()
    }
}

#[test]
fn the_policy_blocks_an_author_who_may_not_draw() {
    let (mut alice, mut bob) = two_on_a_board();
    alice.page(r#"{"t":"menu","item":"settings"}"#);
    assert_eq!(
        alice.host.dialogs.last().map(String::as_str),
        Some("Board settings")
    );
    alice.click(Slot::Dialog, "policy", ActionValue::Text("host".into()));
    settle(&mut alice, &mut bob, 12);
    assert_eq!(
        bob.posted_of("status").last().expect("status")["canDraw"],
        false
    );
    bob.host.sent.clear();
    bob.page(r#"{"t":"create","obj":{"k":"note","g":[0,0,156,112],"text":"sneaky"}}"#);
    settle(&mut alice, &mut bob, 12);
    assert!(!upserted_texts(&alice).contains(&"sneaky".to_owned()));
    assert!(!upserted_texts(&bob).contains(&"sneaky".to_owned()));
    let said: Vec<String> = bob
        .posted_of("announce")
        .iter()
        .filter_map(|a| a["text"].as_str().map(str::to_owned))
        .collect();
    assert!(
        said.iter()
            .any(|s| s.starts_with("You can't change the board")),
        "{said:?}"
    );
}

#[test]
fn a_picture_nobody_asked_for_is_ignored() {
    let (mut alice, _bob) = two_on_a_board();
    let data = base64(PNG);
    alice.page(&format!(
        r#"{{"t":"png","req":1,"part":0,"of":1,"data":"{data}"}}"#
    ));
    alice.page(&format!(
        r#"{{"t":"png","req":0,"part":0,"of":1,"data":"{data}"}}"#
    ));
    assert!(alice.host.images.is_empty());
    assert!(alice.host.chat.is_empty());
    assert!(alice.host.files.is_empty());
}

fn last_note(app: &User) -> String {
    app.posted_of("note")
        .last()
        .and_then(|n| n["text"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[test]
fn post_to_chat_renders_and_falls_back_to_1x() {
    let (mut alice, _bob) = two_on_a_board();
    // The in-board Export dialog's "Post to #General" (mockup 5).
    alice.page(
        r#"{"t":"export","dest":"chat","format":"png","area":"board","scale":2,"view":[0,0,800,600],"sel":[]}"#,
    );
    let render = alice.posted_of("render").pop().expect("render request");
    assert_eq!(render["scale"], 2);
    assert_eq!(render["scope"], "board");
    assert!(
        alice
            .host
            .posted
            .last()
            .expect("posted")
            .starts_with(r#"{"to":"stage","#)
    );
    alice.host.image_results = vec![Err(Error::TooLarge), Ok(PostOutcome::FileOffer)];
    let data = base64(PNG);
    let req = render["req"].as_u64().expect("req");
    alice.page(&format!(
        r#"{{"t":"png","req":{req},"part":0,"of":1,"data":"{data}"}}"#
    ));
    let retry = alice.posted_of("render").pop().expect("second render");
    assert_eq!(retry["scale"], 1);
    let req = retry["req"].as_u64().expect("req");
    let half = data.len() / 8 * 4;
    alice.page(&format!(
        r#"{{"t":"png","req":{req},"part":1,"of":2,"data":"{}"}}"#,
        &data[half..]
    ));
    // A part from another page does not count.
    alice.page_at(
        "side",
        &format!(
            r#"{{"t":"png","req":{req},"part":0,"of":2,"data":"{}"}}"#,
            &data[..half]
        ),
    );
    assert_eq!(alice.host.images, vec![PNG.len()]);
    alice.page(&format!(
        r#"{{"t":"png","req":{req},"part":0,"of":2,"data":"{}"}}"#,
        &data[..half]
    ));
    assert_eq!(alice.host.images, vec![PNG.len(), PNG.len()]);
    let note = last_note(&alice);
    assert!(note.starts_with("Too big for a chat picture"), "{note}");
    // No Mumcord dialog was involved.
    assert!(alice.host.dialogs.is_empty());
}

#[test]
fn export_saves_vector_files_and_explains_a_refusal() {
    let (mut alice, _bob) = two_on_a_board();
    alice.page(r#"{"t":"create","obj":{"k":"rect","g":[0,0,40,40],"ink":1,"w":1}}"#);
    alice.page(r#"{"t":"export","dest":"file","format":"svg","area":"board","scale":1}"#);
    let (name, data) = alice.host.files.pop().expect("saved");
    assert_eq!(name, "Untitled board.svg");
    assert!(String::from_utf8(data).expect("utf8").contains("<svg"));
    alice.host.save_error = Some(Error::NotPermitted);
    alice.page(r#"{"t":"export","dest":"file","format":"board","area":"board","scale":1}"#);
    let note = last_note(&alice);
    assert!(note.starts_with("Mumcord did not allow saving"), "{note}");
    // Nothing selected: the export explains instead of saving an empty file.
    alice.page(
        r#"{"t":"export","dest":"file","format":"pdf","area":"selection","scale":1,"sel":[]}"#,
    );
    assert!(alice.host.files.is_empty());
    assert!(!last_note(&alice).is_empty());
}

#[test]
fn clearing_from_the_board_can_be_undone() {
    let (mut alice, mut bob) = two_on_a_board();
    alice.page(r#"{"t":"create","obj":{"k":"note","g":[0,0,156,112],"text":"keep me"}}"#);
    settle(&mut alice, &mut bob, 12);
    // The page asked "Clear the whole board?" itself (mockup 6b); this is its button.
    alice.page(r#"{"t":"clear"}"#);
    assert!(
        alice
            .posted_of("cleared")
            .last()
            .is_some_and(|c| c["until"].as_u64() > Some(0))
    );
    settle(&mut alice, &mut bob, 12);
    assert_eq!(bob.app_objects(), 0);
    alice.page(r#"{"t":"undo-clear"}"#);
    settle(&mut alice, &mut bob, 12);
    assert_eq!(bob.app_objects(), 1);
    // Bob is no host: his clear is refused with a reason.
    bob.page(r#"{"t":"clear"}"#);
    settle(&mut alice, &mut bob, 12);
    assert_eq!(alice.app_objects(), 1);
    assert!(!last_note(&bob).is_empty());
    // The settings dialog's Clear board opens the page's confirmation.
    alice.page(r#"{"t":"menu","item":"settings"}"#);
    alice.click(Slot::Dialog, "clear", ActionValue::None);
    let dialog = alice.posted_of("dialog").pop().expect("in-board dialog");
    assert_eq!(dialog["which"], "clear");
    assert_eq!(
        dialog["body"],
        "This removes 1 item for all 2 people on the board."
    );
    // Bob is no host: his page gets no Clear confirmation.
    bob.page(r#"{"t":"menu","item":"clear"}"#);
    assert!(bob.posted_of("dialog").is_empty());
    bob.page(r#"{"t":"menu","item":"export","sel":[]}"#);
    assert_eq!(
        bob.posted_of("dialog").pop().expect("export")["which"],
        "export"
    );
}

#[test]
fn oversized_and_garbage_page_messages_are_dropped() {
    let (mut alice, _bob) = two_on_a_board();
    let sent = alice.host.sent.len();
    let posted = alice.host.posted.len();
    let huge = format!(r#"{{"t":"create","pad":"{}"}}"#, "x".repeat(70_000));
    for m in [
        huge.as_str(),
        "garbage",
        r#"{"t":"create","obj":{"k":"note","g":[0,0,"a",1]}}"#,
        r#"{"t":"set","id":"1.1","fields":{"ink":99}}"#,
        r#"{"t":"delete","ids":"all"}"#,
    ] {
        alice.page(m);
    }
    assert_eq!(alice.app.dropped_page_messages(), 5);
    assert_eq!(alice.host.sent.len(), sent);
    assert_eq!(alice.host.posted.len(), posted);
}

#[test]
fn the_side_panel_takes_over_while_the_stage_is_hidden() {
    let (mut alice, _bob) = two_on_a_board();
    alice.event(Event::StageVisibilityChanged(false));
    assert_eq!(
        alice.host.web.last(),
        Some(&(Place::SidePanel, MINI_ENTRY.to_owned()))
    );
    // The mini view gets its own init once ready, within its smaller bridge.
    let before = alice.host.posted.len();
    alice.page_at("side", r#"{"t":"ready"}"#);
    assert!(
        alice.host.posted[before..]
            .iter()
            .all(|m| m.len() <= 16 * 1024)
    );
    assert!(
        alice.host.posted[before..]
            .iter()
            .any(|m| m.starts_with(r#"{"to":"side","t":"init""#))
    );
    // Up to three web views: the stage page stays open next to the side panel.
    let shown = alice.host.web.len();
    alice.event(Event::StageVisibilityChanged(true));
    assert_eq!(alice.host.web.len(), shown);
    // A Mumcord that closed the stage page gets it back once the stage is visible.
    alice.event(Event::SurfaceClosed(Place::Stage));
    alice.event(Event::StageVisibilityChanged(true));
    assert_eq!(
        alice.host.web.last(),
        Some(&(Place::Stage, STAGE_ENTRY.to_owned()))
    );
    // Without the permission the panel stays closed.
    alice.event(Event::SettingsChanged(PluginSettings {
        granted: vec!["ui.stage".into(), "ui.webview".into()],
        visibility: Visibility::Everyone,
    }));
    let shown = alice.host.web.len();
    alice.event(Event::StageVisibilityChanged(false));
    assert_eq!(alice.host.web.len(), shown);
}

#[test]
fn boards_are_saved_and_offered_again() {
    let (mut alice, mut bob) = two_on_a_board();
    alice.page(r#"{"t":"create","obj":{"k":"note","g":[0,0,156,112],"text":"Launch"}}"#);
    settle(&mut alice, &mut bob, 12);
    assert!(alice.host.store.contains_key("boards"));
    assert_eq!(
        alice.posted_of("status").last().expect("status")["saved"],
        "Saved on this device"
    );
    // Bob kept a copy too.
    assert!(bob.host.store.contains_key("boards"));
    alice.event(Event::StageLeft(peer(&alice.me())));
    alice.host.posted.clear();
    alice.starts_board();
    let boards = alice.posted_of("boards");
    assert_eq!(boards[0]["list"][0]["title"], "Untitled board");
    let id = boards[0]["list"][0]["id"].as_str().expect("id").to_owned();
    alice.page(&format!(r#"{{"t":"open","board":"{id}"}}"#));
    assert!(upserted_texts(&alice).contains(&"Launch".to_owned()));
}

#[test]
fn chat_lines_go_out_on_a_click_and_hide_behind_the_card_elsewhere() {
    let (mut alice, mut bob) = two_on_a_board();
    alice.page(r#"{"t":"menu","item":"settings"}"#);
    alice.host.sent.clear();
    alice.click(Slot::Dialog, "line-start", ActionValue::None);
    assert_eq!(
        alice.host.chat,
        vec!["Alice started a whiteboard. Draw together with Mumcord (Apps → Start whiteboard), or just listen in and talk along.".to_owned()]
    );
    let notice = alice
        .host
        .sent
        .iter()
        .find(|(_, k, _)| *k == KIND_LINE)
        .cloned()
        .expect("line notice");
    assert_eq!(notice.0, Target::Channel);
    deliver(&mut alice, &mut bob);
    let (card, sender, hash) = bob.host.supersedes.pop().expect("supersede");
    assert!(card.starts_with("wb-"));
    assert_eq!(sender, 1);
    assert_eq!(hash, alice.host.chat[0].len() as u32);
    // Without chat.send the click explains what is missing.
    alice.host.chat_error = Some(Error::NotPermitted);
    alice.click(Slot::Dialog, "line-summary", ActionValue::None);
    let tree = alice
        .last_tree(|s| matches!(s, Slot::Dialog))
        .expect("dialog");
    assert!(
        tree.iter()
            .any(|t| t.starts_with("Mumcord did not allow the chat line"))
    );
}

#[test]
fn announcements_are_throttled() {
    let (mut alice, mut bob) = two_on_a_board();
    let before = alice.posted_of("announce").len();
    for i in 0..10 {
        bob.page(&format!(
            r#"{{"t":"create","obj":{{"k":"rect","g":[{},0,40,40],"ink":1,"w":1}}}}"#,
            i * 50
        ));
        settle(&mut alice, &mut bob, 1);
    }
    // Ten objects in 2.5 s never become ten announcements.
    let said = alice.posted_of("announce").len() - before;
    assert!(said <= 2, "{said} announcements");
}

#[test]
fn describe_board_summarises() {
    let (mut alice, _bob) = two_on_a_board();
    alice.page(r#"{"t":"create","obj":{"k":"note","g":[0,0,156,112],"text":"QA pass"}}"#);
    alice.page(r#"{"t":"create","obj":{"k":"arrow","g":[0,0,40,40],"ink":1,"w":1}}"#);
    alice.page(r#"{"t":"menu","item":"describe"}"#);
    let tree = alice
        .last_tree(|s| matches!(s, Slot::Dialog))
        .expect("dialog");
    assert_eq!(tree[0], "\"Untitled board\": 2 items by 2 people.");
    assert!(
        tree.contains(&"On the board: 1 arrow, 1 sticky note.".to_owned()),
        "{tree:?}"
    );
    assert!(tree.contains(&"Notes: “QA pass”.".to_owned()), "{tree:?}");
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The BOARD payload `scenarios/invite-card.yaml` feeds in.
#[test]
fn the_scenario_board_payload_matches_the_codec() {
    let m = codec::Msg::Board(codec::BoardMsg {
        board: [0xab; 16],
        title: "Plan".into(),
        host: 0x0102_0304,
        started_ms: 1_700_000_000_000,
    });
    let (kind, payload) = codec::encode_peers(&m).expect("encodes");
    assert_eq!(kind, 6);
    assert_eq!(
        String::from_utf8(payload).unwrap(),
        r#".v=1;.msg={.d="Te&q4Te&q4Te&q4Te&q41w&]yzuH4y0F5KZ%gy5";.k=6;};"#
    );
}

/// Bob joins after Alice drew, but Alice's BOARD for him is lost (the host refused it, for
/// example over a rate limit): she repeats it until Bob says HELLO, so he still loads the board.
#[test]
fn a_lost_board_announcement_is_repeated_for_a_joiner() {
    let mut alice = User::new(1, "Alice", &[(2, "Bob")]);
    let mut bob = User::new(2, "Bob", &[(1, "Alice")]);
    alice.starts_board();
    alice.page(r#"{"t":"create","obj":{"k":"note","g":[10,20,156,112],"fill":2,"text":"early"}}"#);
    for _ in 0..20 {
        alice.tick();
    }
    alice.host.sent.clear();
    bob.event(Event::StageActivityStarted((
        peer(&alice.me()),
        "activity-1".into(),
    )));
    let bob_peer = peer(&bob.me());
    bob.event(Event::StageJoined(bob_peer.clone()));
    alice.event(Event::StageJoined(bob_peer));
    bob.page(r#"{"t":"ready"}"#);
    let mut lost = false;
    for _ in 0..40 {
        alice.tick();
        bob.tick();
        let before = alice.host.sent.len();
        alice.host.sent.retain(|m| m.1 != codec::KIND_BOARD);
        lost |= alice.host.sent.len() < before;
        if lost {
            break;
        }
        deliver(&mut alice, &mut bob);
        deliver(&mut bob, &mut alice);
    }
    assert!(lost, "Alice never announced the board to Bob");
    settle(&mut alice, &mut bob, 80);
    assert!(upserted_texts(&bob).contains(&"early".to_owned()));
}
