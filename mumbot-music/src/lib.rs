//! Mumbot Music: control a mumbot music bot in your channel (FR-6.84).
//!
//! This file only connects Mumcord's events and host calls to the logic in [`app`].

mod api;
mod app;
mod bots;
mod view;

#[cfg(test)]
mod tests;

use std::cell::RefCell;

use mumcord_plugin_sdk::{
    Plugin,
    events::{ActionSource, Event},
    http::{self, HttpRequest},
    log, notify, peers, secrets,
    storage::{self, Scope},
    timers,
    types::{Error, Peer, StartInfo},
    ui::{self, El, Slot},
};

use app::{App, Host};

thread_local! {
    // A plugin instance is single-threaded; the host calls it one event at a time.
    static APP: RefCell<App> = RefCell::new(App::default());
}

/// The real host calls. Logs never contain addresses, keys, titles or names.
struct Mumcord;

impl Host for Mumcord {
    fn http(&mut self, request: HttpRequest) -> Result<u32, Error> {
        http::request(&request)
    }

    fn start_timer(&mut self, ms: u32) -> Option<u32> {
        timers::start(ms, true)
            .inspect_err(|e| log::warn(&format!("could not start the poll timer: {e:?}")))
            .ok()
    }

    fn cancel_timer(&mut self, id: u32) {
        timers::cancel(id);
    }

    fn load(&mut self, key: &str) -> Option<Vec<u8>> {
        storage::get(Scope::Identity, key)
            .inspect_err(|e| log::warn(&format!("could not read a saved bot: {e:?}")))
            .ok()
            .flatten()
    }

    fn save(&mut self, key: &str, value: &[u8]) -> Result<(), Error> {
        storage::set(Scope::Identity, key, value)
            .inspect_err(|e| log::error(&format!("could not save a bot: {e:?}")))
    }

    fn delete(&mut self, key: &str) {
        if let Err(e) = storage::delete(Scope::Identity, key) {
            log::error(&format!("could not delete a saved bot: {e:?}"));
        }
    }

    fn keys(&mut self, prefix: &str) -> Vec<String> {
        storage::list_keys(Scope::Identity, prefix)
            .inspect_err(|e| log::warn(&format!("could not list saved bots: {e:?}")))
            .unwrap_or_default()
    }

    fn delete_secret(&mut self, name: &str) {
        if let Err(e) = secrets::delete(name) {
            log::error(&format!("could not delete an API key: {e:?}"));
        }
    }

    fn show(&mut self, slot: Slot, tree: El) {
        if let Err(e) = ui::show(slot, tree) {
            log::error(&format!("could not show the UI: {e:?}"));
        }
    }

    fn open_dialog(&mut self, title: &str) {
        if let Err(e) = ui::open_dialog(title) {
            log::error(&format!("could not open the dialog: {e:?}"));
        }
    }

    fn set_menu_entry(&mut self, label: Option<&str>) {
        if let Err(e) = ui::set_user_menu_entry(label) {
            log::error(&format!("could not add the menu entry: {e:?}"));
        }
    }

    fn presence(&mut self) -> Vec<Peer> {
        peers::presence()
            .inspect_err(|e| log::warn(&format!("could not read the channel: {e:?}")))
            .unwrap_or_default()
    }

    fn notify(&mut self, title: &str, body: &str) {
        if let Err(e) = notify::send(title, body) {
            log::debug(&format!("notification not shown: {e:?}"));
        }
    }
}

struct MumbotMusic;

impl Plugin for MumbotMusic {
    fn start(_info: StartInfo) -> Result<(), String> {
        APP.with_borrow_mut(|app| app.start(&mut Mumcord));
        Ok(())
    }

    fn on_event(event: Event) {
        APP.with_borrow_mut(|app| {
            let host = &mut Mumcord;
            match event {
                Event::SettingsChanged(settings) => app.set_granted(&settings.granted),
                Event::SelfChanged(state) => app.self_changed(host, &state),
                Event::Joined(peer) => app.joined(host, peer),
                Event::Left(peer) => app.left(host, &peer),
                Event::UserMenuRequested(peer) => app.menu_requested(host, &peer),
                Event::DialogClosed => app.dialog_closed(host),
                Event::Timer(id) => app.timer(host, id),
                Event::HttpResponse(result) => app.http_response(host, result),
                Event::UiAction(action) => match action.source {
                    ActionSource::Slot(Slot::Dialog) => {
                        app.dialog_action(host, &action.action, action.value)
                    }
                    ActionSource::Slot(Slot::Settings) => {
                        app.settings_action(host, &action.action, action.value);
                    }
                    _ => {}
                },
                _ => {}
            }
        });
    }

    fn stop() {
        APP.with_borrow_mut(|app| app.stop(&mut Mumcord));
    }
}

mumcord_plugin_sdk::export!(MumbotMusic);
