//! Skyloom: a voice-first party game. One player, the Weaver, sees a hidden constellation and
//! describes it out loud; everyone else moves one star on a shared night-sky loom. At dawn the
//! group sees how close their sky came to the real figure.
//!
//! This file connects the game ([`app`]) to Mumcord: events in, host calls out.

mod app;
mod deck;
mod fair;
mod rules;
mod score;
mod text;
mod view;
mod wire;

#[cfg(test)]
mod tests;

use std::cell::{Cell, RefCell};

use mumcord_plugin_sdk::{
    Plugin, audio, bovnar, chat, clock,
    events::{ActionSource, Event},
    log, notify, peers, random, relay, stage, storage, theme, timers,
    types::StartInfo,
    ui::{self, El, Slot},
    webview,
};

use app::{App, Host, Place, Prefs};
use view::Intent;
use wire::Message;

const START_ITEM: &str = "start";
const COMMAND: &str = "skyloom";
const PAGE: &str = "index.html";
const PREFS_KEY: &str = "prefs";
const BADGE: &str = "assets/badge.svg";

thread_local! {
    static APP: RefCell<App> = RefCell::new(App::default());
    static TICKER: Cell<Option<u32>> = const { Cell::new(None) };
    static RELAY_OK: Cell<bool> = const { Cell::new(false) };
}

/// Decodes an incoming document; a bad one is dropped with its reason and size, never its content.
fn decode(bytes: &[u8]) -> Option<Message> {
    Message::decode(bytes)
        .inspect_err(|e| {
            log::warn(&format!(
                "dropped a game message: {} ({} bytes)",
                e.reason(),
                bytes.len()
            ));
        })
        .ok()
}

struct Mumcord;

impl Host for Mumcord {
    fn random(&mut self, len: u32) -> Option<Vec<u8>> {
        random::bytes(len)
            .inspect_err(|e| log::warn(&format!("no random bytes: {e:?}")))
            .ok()
    }

    fn send(&mut self, message: &Message, live: bool) {
        let to = if live {
            bovnar::Destination::RelayChannel { fallback: false }
        } else {
            Message::peers(peers::Target::Channel)
        };
        if let Err(e) = bovnar::send_document(to, message) {
            log::debug(&format!("could not send a game message: {}", e.reason()));
        }
    }

    fn send_direct(&mut self, to: u32, message: &Message) {
        if let Err(e) = bovnar::send_document(Message::peers(peers::Target::Direct(to)), message) {
            log::warn(&format!(
                "could not reach the second Weaver: {}",
                e.reason()
            ));
        }
    }

    fn live(&self) -> bool {
        RELAY_OK.get()
    }

    fn start_game_burst(&mut self) {
        if let Err(e) = peers::start_game_burst() {
            log::debug(&format!("no game burst: {e:?}"));
        }
    }

    fn post(&mut self, json: String) {
        if let Err(e) = webview::post(&json) {
            log::debug(&format!("could not message the page: {e:?}"));
        }
    }

    fn start_stage(&mut self, title: &str) -> bool {
        match stage::start_web(title, PAGE) {
            Ok(_) => true,
            Err(e) => {
                log::warn(&format!("could not start the activity: {e:?}"));
                false
            }
        }
    }

    fn end_stage(&mut self) {
        stage::end();
    }

    fn chat_send(&mut self, text: &str) {
        if let Err(e) = chat::send(text) {
            log::debug(&format!("no plain line for standard clients: {e:?}"));
        }
    }

    fn line_hash(&mut self, text: &str) -> u32 {
        chat::line_hash(text)
    }

    fn supersede(&mut self, card: &str, sender: u32, hash: u32) {
        if let Err(e) = chat::supersede(card, sender, hash) {
            log::debug(&format!("could not supersede the text line: {e:?}"));
        }
    }

    fn show_card(&mut self, card: &str, tree: El) {
        if let Err(e) = ui::show(Slot::ChatCard(card.to_string()), tree) {
            log::debug(&format!("no chat card: {e:?}"));
        }
    }

    fn set_badge(&mut self, session: u32, on: bool) {
        if on {
            let badge = ui::image(BADGE, &text::t("badge"), 16, 16);
            if let Err(e) = ui::show(Slot::MemberBadge(session), badge) {
                log::debug(&format!("no member badge: {e:?}"));
            }
        } else {
            ui::clear(&Slot::MemberBadge(session));
        }
    }

    fn play(&mut self, sound: &str, gain: f32) {
        if let Err(e) = audio::play(sound, Some(gain)) {
            log::debug(&format!("no sound: {e:?}"));
        }
    }

    fn stop_sounds(&mut self) {
        audio::stop_all();
    }

    fn notify(&mut self, title: &str, body: &str) {
        // Optional and off by default; without the permission this simply does nothing.
        let _ = notify::send(title, body);
    }

    fn save(&mut self, prefs: &Prefs) {
        if let Err(e) = storage::set_json(storage::Scope::Identity, PREFS_KEY, prefs) {
            log::warn(&format!("could not save the settings: {e:?}"));
        }
    }

    fn warn(&mut self, message: &str) {
        log::warn(message);
    }
}

struct Skyloom;

impl Plugin for Skyloom {
    fn start(_info: StartInfo) -> Result<(), String> {
        APP.with_borrow_mut(|app| {
            app.prefs = storage::get_json(storage::Scope::Identity, PREFS_KEY)
                .ok()
                .flatten()
                .unwrap_or_default();
            let current = theme::current();
            app.dark = current.mode == theme::Mode::Dark;
            app.reduce_motion = current.reduced_motion;
            refresh_presence(app);
        });
        let live = relay::status() == relay::RelayStatus::Available;
        RELAY_OK.set(live);
        APP.with_borrow_mut(|app| app.live = live);
        Ok(())
    }

    fn on_event(event: Event) {
        APP.with_borrow_mut(|app| {
            handle(app, event);
            keep_ticking(app);
        });
    }

    fn stop() {
        APP.with_borrow(|app| {
            if app.joined {
                // Others see this client leave; the night goes on without it.
                Mumcord.send(&Message::Leave, false);
            }
        });
    }
}

fn refresh_presence(app: &mut App) {
    if let Ok(peers) = peers::presence() {
        if let Some(me) = peers.first() {
            app.me = me.session;
        }
        app.on_presence(peers.into_iter().map(|p| (p.session, p.name)).collect());
    }
}

/// Runs the game clock only while there is a night to run.
fn keep_ticking(app: &App) {
    let want = app.night.is_some();
    match (want, TICKER.get()) {
        (true, None) => match timers::start(app::TICK_MS as u32, true) {
            Ok(id) => TICKER.set(Some(id)),
            Err(e) => log::warn(&format!("could not start the game clock: {e:?}")),
        },
        (false, Some(id)) => {
            timers::cancel(id);
            TICKER.set(None);
        }
        _ => {}
    }
}

fn show_place(app: &mut App, place: Place) {
    let host = &mut Mumcord;
    let result = match place {
        Place::Stage => webview::show(webview::Place::Stage, PAGE),
        Place::SidePanel => webview::show(webview::Place::SidePanel, PAGE),
        Place::Dialog => ui::open_dialog(&text::t("dialog.title"))
            .and_then(|()| webview::show(webview::Place::Dialog, PAGE)),
    };
    match result {
        Ok(()) => {
            app.place = Some(place);
            // The new page announces itself with "ready" and then gets the whole view.
            app.page_ready = false;
        }
        Err(e) => host.warn(&format!("could not move the game: {e:?}")),
    }
}

fn handle(app: &mut App, event: Event) {
    let host = &mut Mumcord;
    if text::on_event(&event) {
        if app.page_ready {
            host.post(view::strings());
        }
        app.refresh(host);
        return;
    }
    match event {
        Event::AppMenuActivated(item) if item == START_ITEM => {
            refresh_presence(app);
            app.on_start(host);
        }
        Event::UiAction(action) => match action.source {
            ActionSource::Command if action.action == COMMAND => {
                refresh_presence(app);
                app.on_start(host);
            }
            ActionSource::Slot(Slot::ChatCard(_)) if action.action == "join" => {
                if let Some((id, _)) = &app.activity
                    && let Err(e) = stage::join(id)
                {
                    log::warn(&format!("could not join the night: {e:?}"));
                }
            }
            _ => {}
        },
        Event::WebviewMessage(message) => match Intent::parse(&message) {
            Some(Intent::Place(place)) => show_place(app, place),
            Some(intent) => app.on_page(host, intent),
            None => log::debug("ignored an unexpected message from the page"),
        },
        Event::PeerMessage(message) if message.kind == wire::KIND => {
            if let Some(m) = decode(&message.payload) {
                app.on_message(host, message.sender.session, m);
            }
        }
        Event::RelayMessage((peer, data)) => {
            if let Some(m) = decode(&data) {
                app.on_message(host, peer.session, m);
            }
        }
        Event::RelayStatusChanged(status) => {
            RELAY_OK.set(status == relay::RelayStatus::Available);
            app.live = RELAY_OK.get();
            app.refresh(host);
        }
        Event::Timer(id) if TICKER.get() == Some(id) => {
            app.tick(host, clock::now().unix_seconds);
        }
        Event::Joined(peer) => app.on_joined(peer.session, peer.name),
        Event::Left(peer) => app.on_left(host, peer.session),
        Event::SelfChanged(state) => {
            if let Some(me) = state.me {
                app.me = me.session;
            }
        }
        Event::Talking((peer, on)) => app.on_talking(host, peer.session, on),
        Event::ThemeChanged(info) => {
            app.dark = info.mode == theme::Mode::Dark;
            app.reduce_motion = info.reduced_motion;
            app.refresh(host);
        }
        Event::StageActivityStarted((peer, id)) => {
            refresh_presence(app);
            app.on_activity_started(host, peer.session, id);
        }
        Event::StageActivityEnded(id) => app.on_activity_ended(host, &id),
        Event::StageJoined(peer) if peer.session == app.me => app.on_stage_joined(host),
        Event::StageLeft(peer) if peer.session == app.me => app.on_stage_left(host),
        Event::SurfaceClosed(place) if app.joined => {
            if place != webview::Place::Stage {
                show_place(app, Place::Stage);
            }
        }
        Event::DialogClosed if app.joined && app.place == Some(Place::Dialog) => {
            show_place(app, Place::Stage);
        }
        _ => {}
    }
}

mumcord_plugin_sdk::export!(Skyloom);
