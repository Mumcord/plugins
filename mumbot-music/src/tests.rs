//! Scenario tests of the plugin logic against a mocked host and a mocked mumbot HTTP API.

use std::collections::BTreeMap;

use mumcord_plugin_sdk::{
    events::{ActionValue, Channel, HttpResult, SelfState},
    http::{Body, HeaderValue, HttpError, HttpRequest, HttpResponse, Method},
    types::{Error, Peer},
    ui::{Component, El, Slot},
};

use crate::{
    api,
    app::{App, DIALOG_TITLE, Host, MAX_POLL_MS, MENU_ENTRY, POLL_MS},
    view::{NO_CERTIFICATE_TEXT, NOT_IN_CHANNEL_TEXT},
};

const SERVER: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const BOT_KEY: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
const SECRET_A: &str = "bot/0123456789abcdef0123456789abcdef/fedcba9876543210fedcba9876543210/a";
const SECRET_B: &str = "bot/0123456789abcdef0123456789abcdef/fedcba9876543210fedcba9876543210/b";
const STORAGE_KEY: &str = "bots/0123456789abcdef0123456789abcdef/fedcba9876543210fedcba9876543210";
const QUEUE_BODY: &str = r#"{"now_playing":{"id":1,"title":"Song","source":"file","channel":1,"duration_secs":184},"queue":[{"id":2,"title":"Next"}],"volume":60}"#;

#[derive(Default)]
struct Mock {
    requests: Vec<(u32, HttpRequest)>,
    next_id: u32,
    timers: Vec<(u32, u32)>,
    cancelled: Vec<u32>,
    storage: BTreeMap<String, Vec<u8>>,
    deleted_secrets: Vec<String>,
    trees: BTreeMap<String, El>,
    dialogs: Vec<String>,
    menu: Option<Option<String>>,
    channel: Vec<Peer>,
    notes: Vec<String>,
}

impl Host for Mock {
    fn http(&mut self, request: HttpRequest) -> Result<u32, Error> {
        self.next_id += 1;
        self.requests.push((self.next_id, request));
        Ok(self.next_id)
    }
    fn start_timer(&mut self, ms: u32) -> Option<u32> {
        let id = 100 + u32::try_from(self.timers.len()).unwrap();
        self.timers.push((id, ms));
        Some(id)
    }
    fn cancel_timer(&mut self, id: u32) {
        self.cancelled.push(id);
    }
    fn load(&mut self, key: &str) -> Option<Vec<u8>> {
        self.storage.get(key).cloned()
    }
    fn save(&mut self, key: &str, value: &[u8]) -> Result<(), Error> {
        self.storage.insert(key.into(), value.to_vec());
        Ok(())
    }
    fn delete(&mut self, key: &str) {
        self.storage.remove(key);
    }
    fn keys(&mut self, prefix: &str) -> Vec<String> {
        self.storage
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect()
    }
    fn delete_secret(&mut self, name: &str) {
        self.deleted_secrets.push(name.into());
    }
    fn show(&mut self, slot: Slot, tree: El) {
        self.trees.insert(format!("{slot:?}"), tree);
    }
    fn open_dialog(&mut self, title: &str) {
        self.dialogs.push(title.into());
    }
    fn set_menu_entry(&mut self, label: Option<&str>) {
        self.menu = Some(label.map(str::to_owned));
    }
    fn presence(&mut self) -> Vec<Peer> {
        self.channel.clone()
    }
    fn notify(&mut self, _title: &str, body: &str) {
        self.notes.push(body.into());
    }
}

impl Mock {
    /// Every visible string of a slot, with "[x]" after disabled controls.
    fn texts(&self, slot: Slot) -> Vec<String> {
        fn walk(el: &El, out: &mut Vec<String>) {
            let (text, disabled) = match &el.component {
                Component::Text(t) => (Some(t.text.clone()), false),
                Component::Button(b) => (Some(b.label.clone()), b.disabled),
                Component::Slider(s) => (Some(s.label.clone()), s.disabled),
                Component::TextInput(t) => (Some(t.label.clone()), t.disabled),
                Component::Password(p) => {
                    (Some(format!("{} -> {}", p.label, p.secret)), p.disabled)
                }
                Component::FilePicker(f) => (Some(f.label.clone()), f.disabled),
                Component::Progress(p) => (Some(p.label.clone()), false),
                _ => (None, false),
            };
            if let Some(text) = text {
                out.push(if disabled {
                    format!("{text} [x]")
                } else {
                    text
                });
            }
            el.children.iter().for_each(|c| walk(c, out));
        }
        let mut out = Vec::new();
        if let Some(tree) = self.trees.get(&format!("{slot:?}")) {
            walk(tree, &mut out);
        }
        out
    }

    fn shows(&self, slot: Slot, text: &str) -> bool {
        self.texts(slot).iter().any(|t| t == text)
    }

    fn last(&self) -> &HttpRequest {
        &self.requests.last().expect("a request").1
    }

    fn last_id(&self) -> u32 {
        self.requests.last().expect("a request").0
    }
}

fn me() -> Peer {
    Peer {
        session: 1,
        name: "me".into(),
        key: Some("aa".repeat(32)),
    }
}

fn bot(session: u32) -> Peer {
    Peer {
        session,
        name: "Mumbot".into(),
        key: Some(BOT_KEY.into()),
    }
}

fn state(channel: u32) -> SelfState {
    SelfState {
        me: Some(me()),
        server: Some(SERVER.into()),
        channel: Some(Channel {
            id: channel,
            name: "Lobby".into(),
        }),
        muted: false,
        deafened: false,
        do_not_disturb: false,
    }
}

fn ok(id: u32, body: &str) -> HttpResult {
    status(id, 200, body)
}

fn status(id: u32, status: u16, body: &str) -> HttpResult {
    HttpResult {
        id,
        outcome: Ok(HttpResponse {
            status,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        }),
    }
}

fn text(s: &str) -> ActionValue {
    ActionValue::Text(s.into())
}

/// Started, in channel 1 with the bot (session 7).
fn setup() -> (App, Mock) {
    let mut app = App::default();
    let mut host = Mock {
        channel: vec![me(), bot(7)],
        ..Mock::default()
    };
    app.start(&mut host);
    app.self_changed(&mut host, &state(1));
    (app, host)
}

/// The bot is configured with key slot A and its dialog is open and polling.
fn configured() -> (App, Mock) {
    let (mut app, mut host) = setup();
    app.menu_requested(&mut host, &bot(7));
    app.dialog_action(&mut host, "address", text("127.0.0.1:8081"));
    app.dialog_action(&mut host, "key", ActionValue::None);
    app.dialog_action(&mut host, "test", ActionValue::None);
    let id = host.last_id();
    app.http_response(&mut host, ok(id, QUEUE_BODY));
    app.dialog_action(&mut host, "save", ActionValue::None);
    let id = host.last_id();
    app.http_response(&mut host, ok(id, QUEUE_BODY));
    (app, host)
}

#[test]
fn start_adds_the_menu_entry_and_an_empty_settings_list() {
    let (_, host) = setup();
    assert_eq!(host.menu, Some(Some(MENU_ENTRY.into())));
    assert!(
        host.texts(Slot::Settings)
            .iter()
            .any(|t| t.starts_with("No bots yet"))
    );
}

#[test]
fn a_new_bot_opens_the_setup_form_with_save_disabled() {
    let (mut app, mut host) = setup();
    app.menu_requested(&mut host, &bot(7));
    assert_eq!(host.dialogs, vec![DIALOG_TITLE.to_owned()]);
    assert!(host.shows(Slot::Dialog, "Save [x]"));
    assert!(host.shows(Slot::Dialog, &format!("API key -> {SECRET_A}")));
    assert!(
        host.requests.is_empty(),
        "nothing is requested before the test"
    );
}

#[test]
fn the_test_needs_an_address_and_a_key() {
    let (mut app, mut host) = setup();
    app.menu_requested(&mut host, &bot(7));
    app.dialog_action(&mut host, "test", ActionValue::None);
    assert!(host.shows(Slot::Dialog, "Enter the bot's address"));
    app.dialog_action(&mut host, "address", text("127.0.0.1:8081"));
    app.dialog_action(&mut host, "test", ActionValue::None);
    assert!(host.shows(Slot::Dialog, "Enter the API key"));
    assert!(host.requests.is_empty());
}

#[test]
fn setup_errors_keep_save_disabled() {
    let cases: Vec<(HttpResult, &str)> = vec![
        (status(0, 401, "{}"), api::KEY_REJECTED_TEXT),
        (status(0, 503, "{}"), api::BOT_DISCONNECTED_TEXT),
        (
            HttpResult {
                id: 0,
                outcome: Err(HttpError::Connect("refused".into())),
            },
            "Can't reach the bot at 127.0.0.1:9",
        ),
        (
            HttpResult {
                id: 0,
                outcome: Err(HttpError::HostNotAllowed),
            },
            api::PLAIN_HTTP_TEXT,
        ),
        (
            HttpResult {
                id: 0,
                outcome: Err(HttpError::Tls("expired".into())),
            },
            api::BAD_CERTIFICATE_TEXT,
        ),
    ];
    for (mut result, message) in cases {
        let (mut app, mut host) = setup();
        app.menu_requested(&mut host, &bot(7));
        app.dialog_action(&mut host, "address", text("127.0.0.1:9"));
        app.dialog_action(&mut host, "key", ActionValue::None);
        app.dialog_action(&mut host, "test", ActionValue::None);
        let request = host.last();
        assert_eq!(request.url, "http://127.0.0.1:9/queue");
        assert!(matches!(request.method, Method::Get));
        assert!(matches!(&request.headers[0].value, HeaderValue::Secret(s) if s == SECRET_A));
        result.id = host.last_id();
        app.http_response(&mut host, result);
        assert!(
            host.shows(Slot::Dialog, message),
            "{message}: {:?}",
            host.texts(Slot::Dialog)
        );
        assert!(host.shows(Slot::Dialog, "Save [x]"));
        app.dialog_action(&mut host, "save", ActionValue::None);
        assert!(host.storage.is_empty());
    }
}

#[test]
fn a_passed_test_saves_the_bot_and_starts_polling() {
    let (app, host) = configured();
    let saved: serde_json::Value = serde_json::from_slice(&host.storage[STORAGE_KEY]).unwrap();
    assert_eq!(
        saved,
        serde_json::json!({"name": "Mumbot", "address": "127.0.0.1:8081", "key": "a"})
    );
    assert_eq!(host.timers, vec![(100, POLL_MS)]);
    assert_eq!(host.requests.len(), 2, "test + first poll");
    assert!(host.shows(Slot::Dialog, "Song"));
    assert!(host.shows(Slot::Dialog, "3:04"));
    assert!(host.shows(Slot::Dialog, "1. Next"));
    assert!(
        host.texts(Slot::Settings)
            .iter()
            .any(|t| t == "127.0.0.1:8081")
    );
    assert!(app.dialog().is_some());
    assert!(host.deleted_secrets.is_empty());
}

#[test]
fn a_saved_bot_opens_the_player_after_a_new_session() {
    let (mut app, mut host) = configured();
    app.dialog_closed(&mut host);
    assert_eq!(host.cancelled, vec![100]);
    // The bot reconnects with another session id; it is recognised by its certificate.
    app.left(&mut host, &bot(7));
    app.joined(&mut host, bot(9));
    app.menu_requested(&mut host, &bot(9));
    assert!(host.shows(Slot::Dialog, "Skip"));
    assert_eq!(host.last().url, "http://127.0.0.1:8081/queue");
}

#[test]
fn polling_only_runs_while_open_and_one_at_a_time() {
    let (mut app, mut host) = configured();
    let before = host.requests.len();
    app.timer(&mut host, 100);
    app.timer(&mut host, 100);
    assert_eq!(
        host.requests.len(),
        before + 1,
        "no second poll while one is waiting"
    );
    app.timer(&mut host, 999);
    assert_eq!(
        host.requests.len(),
        before + 1,
        "foreign timers are ignored"
    );
    app.dialog_closed(&mut host);
    let id = host.last_id();
    app.http_response(&mut host, ok(id, QUEUE_BODY));
    app.timer(&mut host, 100);
    assert_eq!(host.requests.len(), before + 1, "nothing after closing");
}

#[test]
fn the_bot_leaving_hides_the_controls_and_stops_polling() {
    let (mut app, mut host) = configured();
    app.left(&mut host, &bot(7));
    assert!(host.shows(Slot::Dialog, NOT_IN_CHANNEL_TEXT));
    assert!(!host.shows(Slot::Dialog, "Skip"));
    assert_eq!(host.cancelled, vec![100]);
    let before = host.requests.len();
    app.dialog_action(&mut host, "skip", ActionValue::None);
    app.timer(&mut host, 100);
    assert_eq!(host.requests.len(), before);
    // It comes back by itself.
    app.joined(&mut host, bot(8));
    assert!(host.shows(Slot::Dialog, "Skip"));
    assert_eq!(host.requests.len(), before + 1);
}

#[test]
fn moving_to_another_channel_counts_as_the_bot_leaving() {
    let (mut app, mut host) = configured();
    host.channel = vec![me()];
    app.self_changed(&mut host, &state(2));
    assert!(host.shows(Slot::Dialog, NOT_IN_CHANNEL_TEXT));
}

#[test]
fn controls_send_the_right_calls() {
    let (mut app, mut host) = configured();
    let poll = host.last_id();
    app.http_response(&mut host, ok(poll, QUEUE_BODY));

    app.dialog_action(&mut host, "skip", ActionValue::None);
    assert_eq!(host.last().url, "http://127.0.0.1:8081/skip");
    app.dialog_action(&mut host, "stop", ActionValue::None);
    assert_eq!(host.last().url, "http://127.0.0.1:8081/stop");
    app.dialog_action(&mut host, "volume", ActionValue::Number(40.0));
    let id = host.last_id();
    assert!(matches!(&host.last().body, Body::Bytes(b) if b == br#"{"volume":40}"#));
    app.http_response(&mut host, ok(id, r#"{"volume":40}"#));
    assert!(app_volume(&app) == Some(40));

    app.dialog_action(&mut host, "add", ActionValue::None);
    assert_eq!(
        host.last().url,
        "http://127.0.0.1:8081/volume",
        "nothing to add yet"
    );
    app.dialog_action(&mut host, "add-text", text("https://media.local/a.mp3"));
    app.dialog_action(&mut host, "interrupt", ActionValue::Boolean(true));
    app.dialog_action(&mut host, "add", ActionValue::None);
    let Body::Bytes(body) = &host.last().body else {
        panic!("json body")
    };
    let body: serde_json::Value = serde_json::from_slice(body).unwrap();
    assert_eq!(
        body,
        serde_json::json!({"url": "https://media.local/a.mp3", "interrupt": true})
    );

    app.dialog_action(&mut host, "upload", ActionValue::File(5));
    assert_eq!(host.last().url, "http://127.0.0.1:8081/play?interrupt=true");
    assert!(matches!(host.last().body, Body::File(5)));
    let id = host.last_id();
    app.http_response(&mut host, status(id, 403, "{}"));
    assert!(
        host.texts(Slot::Dialog)
            .iter()
            .any(|t| t.starts_with("Not allowed"))
    );
}

fn app_volume(app: &App) -> Option<u8> {
    match &app.dialog()?.mode {
        crate::app::Mode::Player(p) => p.queue.as_ref().map(|q| q.volume),
        _ => None,
    }
}

#[test]
fn rate_limits_slow_polling_down_and_success_resets_it() {
    let (mut app, mut host) = configured();
    for expected in [6_000, 12_000, MAX_POLL_MS] {
        let id = host.last_id();
        app.http_response(&mut host, ok(id, QUEUE_BODY));
        app.dialog_action(&mut host, "skip", ActionValue::None);
        let id = host.last_id();
        app.http_response(&mut host, status(id, 429, "{}"));
        assert_eq!(app.poll_interval(), expected);
        assert!(host.shows(Slot::Dialog, api::RATE_LIMITED_TEXT));
    }
    assert_eq!(host.timers.last().unwrap().1, MAX_POLL_MS);
    app.dialog_action(&mut host, "skip", ActionValue::None);
    let id = host.last_id();
    app.http_response(&mut host, ok(id, r#"{"skipped":true}"#));
    assert_eq!(app.poll_interval(), POLL_MS);
    assert_eq!(host.timers.last().unwrap().1, POLL_MS);
}

#[test]
fn an_offline_bot_disables_the_controls_and_recovers() {
    let (mut app, mut host) = configured();
    app.set_granted(&["notify".into()]);
    app.timer(&mut host, 100);
    let id = host.last_id();
    app.http_response(
        &mut host,
        HttpResult {
            id,
            outcome: Err(HttpError::Timeout),
        },
    );
    assert!(host.shows(Slot::Dialog, "Can't reach the bot at 127.0.0.1:8081"));
    assert!(host.shows(Slot::Dialog, "Skip [x]"));
    assert_eq!(host.notes, vec!["Mumbot went offline".to_owned()]);
    app.timer(&mut host, 100);
    let id = host.last_id();
    app.http_response(&mut host, status(id, 503, "{}"));
    assert_eq!(host.notes.len(), 1, "notified once");
    app.timer(&mut host, 100);
    let id = host.last_id();
    app.http_response(&mut host, ok(id, QUEUE_BODY));
    assert!(host.shows(Slot::Dialog, "Skip"));
}

#[test]
fn a_hostile_title_is_shown_as_short_plain_text() {
    let (mut app, mut host) = configured();
    app.timer(&mut host, 100);
    let id = host.last_id();
    let title = format!("<script>x</script>\u{1b}[31m{}", "z".repeat(300));
    let body =
        serde_json::json!({"now_playing": {"title": title}, "queue": [], "volume": 1}).to_string();
    app.http_response(&mut host, ok(id, &body));
    let shown = host
        .texts(Slot::Dialog)
        .into_iter()
        .find(|t| t.starts_with("<script>"))
        .unwrap();
    assert!(shown.chars().count() <= 80);
    assert!(!shown.chars().any(char::is_control));
}

#[test]
fn a_failed_edit_keeps_the_working_key() {
    let (mut app, mut host) = configured();
    app.dialog_action(&mut host, "edit", ActionValue::None);
    assert!(host.shows(
        Slot::Dialog,
        &format!("New API key (leave empty to keep the current one) -> {SECRET_B}")
    ));
    app.dialog_action(&mut host, "key", ActionValue::None);
    app.dialog_action(&mut host, "test", ActionValue::None);
    assert!(matches!(&host.last().headers[0].value, HeaderValue::Secret(s) if s == SECRET_B));
    let id = host.last_id();
    app.http_response(&mut host, status(id, 401, "{}"));
    app.dialog_action(&mut host, "cancel", ActionValue::None);
    assert_eq!(host.deleted_secrets, vec![SECRET_B.to_owned()]);
    let saved: serde_json::Value = serde_json::from_slice(&host.storage[STORAGE_KEY]).unwrap();
    assert_eq!(saved["key"], "a");
    assert!(host.shows(Slot::Dialog, "Skip"));
}

#[test]
fn a_passed_edit_switches_the_key_slot() {
    let (mut app, mut host) = configured();
    app.dialog_action(&mut host, "edit", ActionValue::None);
    app.dialog_action(&mut host, "address", text("10.0.0.5:8081"));
    app.dialog_action(&mut host, "key", ActionValue::None);
    app.dialog_action(&mut host, "test", ActionValue::None);
    let id = host.last_id();
    app.http_response(&mut host, ok(id, QUEUE_BODY));
    app.dialog_action(&mut host, "save", ActionValue::None);
    let saved: serde_json::Value = serde_json::from_slice(&host.storage[STORAGE_KEY]).unwrap();
    assert_eq!(
        saved,
        serde_json::json!({"name": "Mumbot", "address": "10.0.0.5:8081", "key": "b"})
    );
    assert_eq!(host.deleted_secrets, vec![SECRET_A.to_owned()]);
    assert_eq!(host.last().url, "http://10.0.0.5:8081/queue");
}

#[test]
fn closing_an_unsaved_setup_deletes_the_typed_key() {
    let (mut app, mut host) = setup();
    app.menu_requested(&mut host, &bot(7));
    app.dialog_action(&mut host, "key", ActionValue::None);
    app.dialog_closed(&mut host);
    assert_eq!(host.deleted_secrets, vec![SECRET_A.to_owned()]);
}

#[test]
fn remove_deletes_the_config_and_both_keys() {
    let (mut app, mut host) = configured();
    app.dialog_action(&mut host, "remove", ActionValue::None);
    assert!(host.storage.contains_key(STORAGE_KEY), "asks first");
    app.dialog_action(&mut host, "remove-confirm", ActionValue::None);
    assert!(host.storage.is_empty());
    assert_eq!(
        host.deleted_secrets,
        vec![SECRET_A.to_owned(), SECRET_B.to_owned()]
    );
    assert!(host.shows(Slot::Dialog, "Save [x]"));
    assert!(
        host.texts(Slot::Settings)
            .iter()
            .any(|t| t.starts_with("No bots yet"))
    );
}

#[test]
fn settings_list_edits_and_removes_saved_bots() {
    let (mut app, mut host) = configured();
    app.dialog_closed(&mut host);
    assert!(host.shows(Slot::Settings, "Mumbot"));
    app.settings_action(&mut host, &format!("edit:{STORAGE_KEY}"), ActionValue::None);
    assert!(host.shows(
        Slot::Settings,
        &format!("New API key (leave empty to keep the current one) -> {SECRET_B}")
    ));
    app.settings_action(&mut host, "edit-address", text("10.0.0.9:8081"));
    app.settings_action(&mut host, "edit-key", ActionValue::None);
    app.settings_action(&mut host, "edit-save", ActionValue::None);
    let saved: serde_json::Value = serde_json::from_slice(&host.storage[STORAGE_KEY]).unwrap();
    assert_eq!(saved["address"], "10.0.0.9:8081");
    assert_eq!(saved["key"], "b");
    assert_eq!(host.deleted_secrets, vec![SECRET_A.to_owned()]);
    assert!(host.shows(Slot::Settings, "10.0.0.9:8081"));

    app.settings_action(
        &mut host,
        &format!("remove:{STORAGE_KEY}"),
        ActionValue::None,
    );
    assert!(host.storage.is_empty());
    assert!(
        host.texts(Slot::Settings)
            .iter()
            .any(|t| t.starts_with("No bots yet"))
    );
    assert_eq!(host.requests.len(), 2, "settings never make requests");
}

#[test]
fn bots_on_two_servers_are_kept_apart() {
    let (_, mut host) = configured();
    let mut app = App::default();
    app.start(&mut host);
    let mut other = state(1);
    other.server = Some("9".repeat(64));
    app.self_changed(&mut host, &other);
    app.menu_requested(&mut host, &bot(7));
    assert!(
        host.shows(Slot::Dialog, "Save [x]"),
        "same bot user on another server is a new bot"
    );
    assert!(
        host.texts(Slot::Dialog)
            .iter()
            .any(|t| t.ends_with(&format!(
                "{}/fedcba9876543210fedcba9876543210/a",
                "9".repeat(32)
            )))
    );
}

#[test]
fn a_user_without_certificate_cannot_be_configured() {
    let (mut app, mut host) = setup();
    app.menu_requested(
        &mut host,
        &Peer {
            session: 3,
            name: "guest".into(),
            key: None,
        },
    );
    assert!(host.shows(Slot::Dialog, NO_CERTIFICATE_TEXT));
    assert!(host.requests.is_empty());
}

#[test]
fn late_answers_for_a_closed_dialog_are_ignored() {
    let (mut app, mut host) = setup();
    app.menu_requested(&mut host, &bot(7));
    app.dialog_action(&mut host, "address", text("127.0.0.1:8081"));
    app.dialog_action(&mut host, "key", ActionValue::None);
    app.dialog_action(&mut host, "test", ActionValue::None);
    let id = host.last_id();
    app.dialog_closed(&mut host);
    app.menu_requested(&mut host, &bot(7));
    app.http_response(&mut host, ok(id, QUEUE_BODY));
    assert!(host.shows(Slot::Dialog, "Save [x]"));
}
