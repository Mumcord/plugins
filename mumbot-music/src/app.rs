//! The plugin's logic, independent of the real host so it can be tested natively.
//!
//! One dialog at a time shows one bot: the setup form until the bot is saved, then the player.
//! The player polls `GET /queue` only while the dialog is open, the bot is in the user's channel
//! and nothing else is waiting for an answer.

use std::collections::HashMap;

use mumcord_plugin_sdk::{
    events::{ActionValue, HttpResult, SelfState},
    http::HttpRequest,
    types::{Error, Peer},
    ui::{El, Slot},
};

use crate::{
    api::{self, Call, Outcome, Queue},
    bots::{self, BotConfig, BotId, KeySlot},
    text::{t, t_with},
    view,
};

/// The poll interval. FR-6.84 says 2 s, but mumbot allows 30 requests per minute per key, so
/// 2 s polling alone would use the whole budget and every click would get a 429.
pub const POLL_MS: u32 = 3_000;
/// The longest interval after repeated 429 answers.
pub const MAX_POLL_MS: u32 = 12_000;

/// The real or a mocked host.
pub trait Host {
    fn http(&mut self, request: HttpRequest) -> Result<u32, Error>;
    /// A repeating timer.
    fn start_timer(&mut self, ms: u32) -> Option<u32>;
    fn cancel_timer(&mut self, id: u32);
    /// Identity-scoped storage.
    fn load(&mut self, key: &str) -> Option<Vec<u8>>;
    fn save(&mut self, key: &str, value: &[u8]) -> Result<(), Error>;
    fn delete(&mut self, key: &str);
    fn keys(&mut self, prefix: &str) -> Vec<String>;
    fn delete_secret(&mut self, name: &str);
    fn show(&mut self, slot: Slot, tree: El);
    fn open_dialog(&mut self, title: &str);
    fn set_menu_entry(&mut self, label: Option<&str>);
    /// The users in the user's channel, the user first.
    fn presence(&mut self) -> Vec<Peer>;
    fn notify(&mut self, title: &str, body: &str);
}

/// Progress of the connection test in the setup form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Test {
    Idle,
    Running,
    Passed,
    Failed(String),
}

/// The setup form, for a new bot or for editing a saved one.
#[derive(Debug, Clone)]
pub struct Setup {
    pub address: String,
    /// The slot the password field writes to: the inactive one when editing.
    pub slot: KeySlot,
    /// Whether the user saved a key into `slot` in this form.
    pub key_entered: bool,
    pub test: Test,
    /// The saved config when editing.
    pub saved: Option<BotConfig>,
}

impl Setup {
    fn new(saved: Option<BotConfig>) -> Self {
        Self {
            address: saved
                .as_ref()
                .map(|c| c.address.clone())
                .unwrap_or_default(),
            slot: saved.as_ref().map_or(KeySlot::A, |c| c.key.other()),
            key_entered: false,
            test: Test::Idle,
            saved,
        }
    }

    /// The slot the test and Save use: the new key if one was entered, else the saved one.
    fn key_slot(&self) -> Option<KeySlot> {
        if self.key_entered {
            Some(self.slot)
        } else {
            self.saved.as_ref().map(|c| c.key)
        }
    }
}

/// The player of a saved bot.
#[derive(Debug, Clone)]
pub struct Player {
    pub config: BotConfig,
    pub queue: Option<Queue>,
    pub offline: Option<String>,
    pub message: Option<String>,
    pub add_text: String,
    pub interrupt: bool,
    pub confirm_remove: bool,
}

#[derive(Debug, Clone)]
pub enum Mode {
    /// The user has no certificate, so the plugin cannot recognise it again.
    NoCertificate,
    Setup(Setup),
    Player(Player),
}

#[derive(Debug, Clone)]
pub struct Dialog {
    pub bot: BotId,
    pub name: String,
    pub mode: Mode,
    /// Whether the bot is in the user's channel right now.
    pub present: bool,
    /// Whether the user was told that this bot went offline.
    notified: bool,
}

/// What an answer belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Test,
    Poll,
    Volume,
    Action,
}

/// Inline edit of a saved bot in the settings form (no request is made there).
#[derive(Debug, Clone)]
pub struct SettingsEdit {
    pub bot: BotId,
    pub config: BotConfig,
    pub address: String,
    pub key_entered: bool,
}

#[derive(Default)]
pub struct App {
    server: Option<String>,
    channel: Option<u32>,
    members: Vec<Peer>,
    dialog: Option<Dialog>,
    /// Bumped whenever the dialog changes bot or mode, so late answers are ignored.
    generation: u32,
    pending: HashMap<u32, (u32, Purpose)>,
    timer: Option<u32>,
    interval: u32,
    notify: bool,
    settings_edit: Option<SettingsEdit>,
}

impl App {
    pub fn start(&mut self, host: &mut impl Host) {
        self.interval = POLL_MS;
        host.set_menu_entry(Some(&t("configure_bot")));
        self.render_settings(host);
    }

    /// The menu entry label is set once, so it has to be set again in the new language.
    pub fn language_changed(&mut self, host: &mut impl Host) {
        host.set_menu_entry(Some(&t("configure_bot")));
        self.update(host);
        self.render_settings(host);
    }

    pub fn stop(&mut self, host: &mut impl Host) {
        self.stop_polling(host);
    }

    #[cfg(test)]
    pub fn dialog(&self) -> Option<&Dialog> {
        self.dialog.as_ref()
    }

    #[cfg(test)]
    pub fn poll_interval(&self) -> u32 {
        self.interval
    }

    pub fn set_granted(&mut self, granted: &[String]) {
        self.notify = granted.iter().any(|p| p == "notify");
    }

    pub fn self_changed(&mut self, host: &mut impl Host, state: &SelfState) {
        let channel = state.channel.as_ref().map(|c| c.id);
        if state.server != self.server || channel != self.channel {
            self.server = state.server.clone();
            self.channel = channel;
            self.members = host.presence();
            self.presence_changed(host);
        }
    }

    pub fn joined(&mut self, host: &mut impl Host, peer: Peer) {
        self.members.retain(|p| p.session != peer.session);
        self.members.push(peer);
        self.presence_changed(host);
    }

    pub fn left(&mut self, host: &mut impl Host, peer: &Peer) {
        self.members.retain(|p| p.session != peer.session);
        self.presence_changed(host);
    }

    /// The user chose "Configure bot" on `peer`.
    pub fn menu_requested(&mut self, host: &mut impl Host, peer: &Peer) {
        self.close(host);
        let name = api::clean(&peer.name, 40);
        let (bot, mode) = match (self.server.as_deref(), peer.key.as_deref()) {
            (Some(server), Some(key)) => {
                let bot = BotId::new(server, key);
                let mode = match load_config(host, &bot) {
                    Some(config) => Mode::Player(new_player(config)),
                    None => Mode::Setup(Setup::new(None)),
                };
                (bot, mode)
            }
            _ => (BotId::new("", ""), Mode::NoCertificate),
        };
        self.dialog = Some(Dialog {
            present: self.is_present(&bot),
            bot,
            name,
            mode,
            notified: false,
        });
        self.generation += 1;
        host.open_dialog(&t("configure_bot"));
        self.update(host);
    }

    /// The user closed the dialog.
    pub fn dialog_closed(&mut self, host: &mut impl Host) {
        self.close(host);
    }

    pub fn timer(&mut self, host: &mut impl Host, id: u32) {
        if self.timer == Some(id) {
            self.poll(host);
        }
    }

    pub fn dialog_action(&mut self, host: &mut impl Host, action: &str, value: ActionValue) {
        let Some(dialog) = self.dialog.as_mut() else {
            return;
        };
        let present = dialog.present;
        match &mut dialog.mode {
            Mode::NoCertificate => {}
            Mode::Setup(setup) => match (action, value) {
                ("address", ActionValue::Text(text)) => {
                    setup.address = text;
                    setup.test = Test::Idle;
                }
                ("key", _) => {
                    setup.key_entered = true;
                    setup.test = Test::Idle;
                }
                ("test", _) if present => {
                    let bot = dialog.bot.clone();
                    let setup = setup.clone();
                    let test = match (bots::base_url(&setup.address), setup.key_slot()) {
                        (None, _) => Test::Failed(t("enter_address")),
                        (_, None) => Test::Failed(t("enter_key")),
                        (Some(base), Some(slot)) => {
                            let request = api::request(&base, &bot.secret(slot), &Call::Queue);
                            match self.send(host, request, Purpose::Test) {
                                Ok(()) => Test::Running,
                                Err(text) => Test::Failed(text),
                            }
                        }
                    };
                    if let Some(Mode::Setup(setup)) = self.dialog.as_mut().map(|d| &mut d.mode) {
                        setup.test = test;
                    }
                }
                ("save", _) if setup.test == Test::Passed => self.save_setup(host),
                ("cancel", _) => {
                    if let Some(config) = setup.saved.clone() {
                        if setup.key_entered {
                            host.delete_secret(&dialog.bot.secret(setup.slot));
                        }
                        dialog.mode = Mode::Player(new_player(config));
                        self.generation += 1;
                    }
                }
                _ => {}
            },
            Mode::Player(player) => {
                let call = match (action, value) {
                    ("edit", _) => {
                        dialog.mode = Mode::Setup(Setup::new(Some(player.config.clone())));
                        self.generation += 1;
                        None
                    }
                    ("remove", _) => {
                        player.confirm_remove = true;
                        None
                    }
                    ("remove-cancel", _) => {
                        player.confirm_remove = false;
                        None
                    }
                    ("remove-confirm", _) => {
                        let bot = dialog.bot.clone();
                        forget_bot(host, &bot);
                        dialog.mode = Mode::Setup(Setup::new(None));
                        self.generation += 1;
                        self.render_settings(host);
                        None
                    }
                    ("add-text", ActionValue::Text(text)) => {
                        player.add_text = text.trim().to_owned();
                        None
                    }
                    ("interrupt", ActionValue::Boolean(on)) => {
                        player.interrupt = on;
                        None
                    }
                    _ if !present || player.offline.is_some() => None,
                    ("skip", _) => Some(Call::Skip),
                    ("stop", _) => Some(Call::Stop),
                    ("volume", ActionValue::Number(v)) => {
                        Some(Call::Volume(v.clamp(0.0, 100.0).round() as u8))
                    }
                    ("add", _) if !player.add_text.is_empty() => Some(Call::Play {
                        source: std::mem::take(&mut player.add_text),
                        interrupt: player.interrupt,
                    }),
                    ("upload", ActionValue::File(file)) => Some(Call::Upload {
                        file,
                        interrupt: player.interrupt,
                    }),
                    _ => None,
                };
                if let Some(call) = call {
                    self.player_call(host, call);
                }
            }
        }
        self.update(host);
    }

    pub fn settings_action(&mut self, host: &mut impl Host, action: &str, value: ActionValue) {
        if let Some((verb, key)) = action.split_once(':') {
            let Some(bot) = BotId::from_storage_key(key) else {
                return;
            };
            match verb {
                "edit" => {
                    self.settings_edit = load_config(host, &bot).map(|config| SettingsEdit {
                        bot,
                        address: config.address.clone(),
                        config,
                        key_entered: false,
                    });
                }
                "remove" => {
                    forget_bot(host, &bot);
                    if self.settings_edit.as_ref().is_some_and(|e| e.bot == bot) {
                        self.settings_edit = None;
                    }
                    if self.dialog.as_ref().is_some_and(|d| d.bot == bot) {
                        if let Some(dialog) = self.dialog.as_mut() {
                            dialog.mode = Mode::Setup(Setup::new(None));
                        }
                        self.generation += 1;
                        self.update(host);
                    }
                }
                _ => {}
            }
        } else if let Some(edit) = self.settings_edit.as_mut() {
            match (action, value) {
                ("edit-address", ActionValue::Text(text)) => edit.address = text,
                ("edit-key", _) => edit.key_entered = true,
                ("edit-save", _) if bots::base_url(&edit.address).is_some() => {
                    let edit = self.settings_edit.take().expect("checked above");
                    let mut config = edit.config.clone();
                    config.address = edit.address.trim().to_owned();
                    if edit.key_entered {
                        config.key = edit.config.key.other();
                    }
                    if save_config(host, &edit.bot, &config) && edit.key_entered {
                        host.delete_secret(&edit.bot.secret(edit.config.key));
                    }
                    self.reload_player(host, &edit.bot);
                }
                ("edit-cancel", _) => {
                    let edit = self.settings_edit.take().expect("checked above");
                    if edit.key_entered {
                        host.delete_secret(&edit.bot.secret(edit.config.key.other()));
                    }
                }
                _ => {}
            }
        }
        self.render_settings(host);
    }

    pub fn http_response(&mut self, host: &mut impl Host, result: HttpResult) {
        let Some((generation, purpose)) = self.pending.remove(&result.id) else {
            return;
        };
        if generation != self.generation {
            return;
        }
        let Some(dialog) = self.dialog.as_mut() else {
            return;
        };
        let notify = self.notify;
        match &mut dialog.mode {
            Mode::Setup(setup) if purpose == Purpose::Test => {
                setup.test = match api::outcome(result, &setup.address) {
                    Outcome::Ok(body) if api::parse_queue(&body).is_some() => Test::Passed,
                    Outcome::Ok(_) => Test::Failed(t("not_mumbot")),
                    Outcome::RateLimited => Test::Failed(api::rate_limited_text()),
                    Outcome::Offline(text) | Outcome::Failed(text) => Test::Failed(text),
                };
            }
            Mode::Player(player) => {
                let mut slow_down = false;
                match api::outcome(result, &player.config.address) {
                    Outcome::Ok(body) => {
                        player.offline = None;
                        dialog.notified = false;
                        match purpose {
                            Purpose::Poll => {
                                player.queue = api::parse_queue(&body);
                                if player.message.as_deref()
                                    == Some(api::rate_limited_text().as_str())
                                {
                                    player.message = None;
                                }
                            }
                            Purpose::Volume => {
                                if let (Some(queue), Some(volume)) =
                                    (player.queue.as_mut(), api::parse_volume(&body))
                                {
                                    queue.volume = volume;
                                }
                                player.message = None;
                            }
                            _ => player.message = None,
                        }
                        if self.interval != POLL_MS {
                            self.interval = POLL_MS;
                            self.restart_timer(host);
                        }
                    }
                    Outcome::Offline(text) => {
                        if !dialog.notified && notify {
                            host.notify(
                                &t("manifest.name"),
                                &t_with("went_offline", &[("name", &dialog.name)]),
                            );
                        }
                        dialog.notified = true;
                        player.offline = Some(text);
                    }
                    Outcome::RateLimited => {
                        player.message = Some(api::rate_limited_text());
                        slow_down = true;
                    }
                    Outcome::Failed(text) => player.message = Some(text),
                }
                if slow_down {
                    self.interval = (self.interval * 2).min(MAX_POLL_MS);
                    self.restart_timer(host);
                }
            }
            _ => {}
        }
        self.update(host);
    }

    fn save_setup(&mut self, host: &mut impl Host) {
        let Some(dialog) = self.dialog.as_mut() else {
            return;
        };
        let Mode::Setup(setup) = &dialog.mode else {
            return;
        };
        let Some(key) = setup.key_slot() else { return };
        let config = BotConfig {
            name: setup
                .saved
                .as_ref()
                .map_or_else(|| dialog.name.clone(), |c| c.name.clone()),
            address: setup.address.trim().to_owned(),
            key,
        };
        if !save_config(host, &dialog.bot, &config) {
            setup_failed(dialog, &t("save_failed"));
            return;
        }
        // The old key is no longer needed once the new one is active.
        if let Some(old) = setup.saved.as_ref().filter(|old| old.key != key) {
            host.delete_secret(&dialog.bot.secret(old.key));
        }
        dialog.mode = Mode::Player(new_player(config));
        self.generation += 1;
        self.render_settings(host);
    }

    fn player_call(&mut self, host: &mut impl Host, call: Call) {
        let Some(dialog) = self.dialog.as_ref() else {
            return;
        };
        let Mode::Player(player) = &dialog.mode else {
            return;
        };
        let Some(base) = bots::base_url(&player.config.address) else {
            return;
        };
        let purpose = match call {
            Call::Queue => Purpose::Poll,
            Call::Volume(_) => Purpose::Volume,
            _ => Purpose::Action,
        };
        let request = api::request(&base, &dialog.bot.secret(player.config.key), &call);
        if let Err(text) = self.send(host, request, purpose)
            && let Some(Mode::Player(player)) = self.dialog.as_mut().map(|d| &mut d.mode)
        {
            player.message = Some(text);
        }
    }

    fn poll(&mut self, host: &mut impl Host) {
        if self
            .pending
            .values()
            .any(|(g, p)| *g == self.generation && *p == Purpose::Poll)
        {
            return;
        }
        if self
            .dialog
            .as_ref()
            .is_some_and(|d| d.present && matches!(d.mode, Mode::Player(_)))
        {
            self.player_call(host, Call::Queue);
        }
    }

    fn send(
        &mut self,
        host: &mut impl Host,
        request: HttpRequest,
        purpose: Purpose,
    ) -> Result<(), String> {
        match host.http(request) {
            Ok(id) => {
                self.pending.insert(id, (self.generation, purpose));
                Ok(())
            }
            Err(Error::RateLimited) => Err(api::rate_limited_text()),
            Err(e) => Err(t_with("refused", &[("reason", e.name())])),
        }
    }

    fn is_present(&self, bot: &BotId) -> bool {
        self.members
            .iter()
            .any(|p| bot.matches(self.server.as_deref(), p.key.as_deref()))
    }

    fn presence_changed(&mut self, host: &mut impl Host) {
        let Some(bot) = self.dialog.as_ref().map(|d| d.bot.clone()) else {
            return;
        };
        let present = self.is_present(&bot);
        if let Some(dialog) = self.dialog.as_mut()
            && dialog.present != present
        {
            dialog.present = present;
            self.update(host);
        }
    }

    /// Applies a settings edit to the open player of that bot.
    fn reload_player(&mut self, host: &mut impl Host, bot: &BotId) {
        let Some(dialog) = self.dialog.as_mut().filter(|d| &d.bot == bot) else {
            return;
        };
        if let (Mode::Player(_), Some(config)) = (&dialog.mode, load_config(host, bot)) {
            dialog.mode = Mode::Player(new_player(config));
            self.generation += 1;
            self.update(host);
        }
    }

    fn close(&mut self, host: &mut impl Host) {
        if let Some(dialog) = self.dialog.take()
            && let Mode::Setup(setup) = &dialog.mode
            && setup.key_entered
            && setup.saved.as_ref().is_none_or(|c| c.key != setup.slot)
        {
            // A key typed into a form that was never saved must not stay in the keychain.
            host.delete_secret(&dialog.bot.secret(setup.slot));
        }
        self.generation += 1;
        self.stop_polling(host);
    }

    /// Re-renders the dialog and starts or stops polling to match it.
    fn update(&mut self, host: &mut impl Host) {
        let Some(dialog) = self.dialog.as_ref() else {
            return;
        };
        let polling = dialog.present && matches!(dialog.mode, Mode::Player(_));
        host.show(Slot::Dialog, view::dialog(dialog));
        match (polling, self.timer) {
            (true, None) => {
                self.timer = host.start_timer(self.interval);
                self.poll(host);
            }
            (false, Some(_)) => self.stop_polling(host),
            _ => {}
        }
    }

    fn restart_timer(&mut self, host: &mut impl Host) {
        if let Some(id) = self.timer.take() {
            host.cancel_timer(id);
            self.timer = host.start_timer(self.interval);
        }
    }

    fn stop_polling(&mut self, host: &mut impl Host) {
        if let Some(id) = self.timer.take() {
            host.cancel_timer(id);
        }
    }

    fn render_settings(&mut self, host: &mut impl Host) {
        let saved: Vec<(String, BotConfig)> = host
            .keys(bots::PREFIX)
            .into_iter()
            .filter_map(|key| {
                let bot = BotId::from_storage_key(&key)?;
                Some((key, load_config(host, &bot)?))
            })
            .collect();
        host.show(
            Slot::Settings,
            view::settings(&saved, self.settings_edit.as_ref()),
        );
    }
}

fn new_player(config: BotConfig) -> Player {
    Player {
        config,
        queue: None,
        offline: None,
        message: None,
        add_text: String::new(),
        interrupt: false,
        confirm_remove: false,
    }
}

fn setup_failed(dialog: &mut Dialog, text: &str) {
    if let Mode::Setup(setup) = &mut dialog.mode {
        setup.test = Test::Failed(text.into());
    }
}

fn load_config(host: &mut impl Host, bot: &BotId) -> Option<BotConfig> {
    serde_json::from_slice(&host.load(&bot.storage_key())?).ok()
}

fn save_config(host: &mut impl Host, bot: &BotId, config: &BotConfig) -> bool {
    let bytes = serde_json::to_vec(config).unwrap_or_default();
    host.save(&bot.storage_key(), &bytes).is_ok()
}

/// Deletes everything stored for a bot: its config and both keychain slots.
fn forget_bot(host: &mut impl Host, bot: &BotId) {
    host.delete(&bot.storage_key());
    host.delete_secret(&bot.secret(KeySlot::A));
    host.delete_secret(&bot.secret(KeySlot::B));
}
