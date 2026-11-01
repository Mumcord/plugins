//! EVE fit: shows EVE Online fits pasted into the chat as a card with a full view.
//!
//! This file only connects Mumcord's events and host calls to the logic in [`app`].

mod app;
mod eft;
mod history;
mod view;

#[cfg(test)]
mod tests;

use std::cell::RefCell;

use mumcord_plugin_sdk::{
    Plugin, clock,
    events::{ActionSource, Event, PanelVisibility},
    log,
    storage::{self, Scope},
    types::{Error, StartInfo},
    ui::{self, El, Slot},
    webview::{self, Place},
};

use app::{App, Host};
use history::Store;

thread_local! {
    // A plugin instance is single-threaded; the host calls it one event at a time.
    static APP: RefCell<App> = RefCell::new(App::default());
}

/// The real host calls.
struct Mumcord;

impl Host for Mumcord {
    fn show_card(&mut self, id: &str, tree: El) {
        if let Err(e) = ui::show(Slot::ChatCard(id.to_owned()), tree) {
            log::error(&format!("could not show the fit card: {e:?}"));
        }
    }

    fn show_dialog(&mut self, tree: El) {
        if let Err(e) = ui::show(Slot::Dialog, tree) {
            log::error(&format!("could not show the fit: {e:?}"));
        }
    }

    fn open_dialog(&mut self, title: &str, subtitle: Option<&str>) -> Result<(), Error> {
        ui::open_dialog_with(title, subtitle)
    }

    fn show_webview(&mut self) -> Result<(), Error> {
        webview::show(Place::Dialog, "index.html")
    }

    fn post_webview(&mut self, message: &str) -> Result<(), Error> {
        webview::post(message)
    }

    fn show_panel(&mut self, tree: El) {
        if let Err(e) = ui::show(Slot::Panel, tree) {
            log::error(&format!("could not show the fit history: {e:?}"));
        }
    }

    fn open_popover(&mut self) {
        if let Err(e) = ui::open_panel_popover() {
            log::debug(&format!("could not open the fit history: {e:?}"));
        }
    }

    fn now(&mut self) -> (u64, i32) {
        let now = clock::now();
        (now.unix_seconds, now.utc_offset_minutes)
    }

    fn warn(&mut self, message: &str) {
        log::warn(message);
    }
}

/// The history is per identity, like the chat it comes from.
impl Store for Mumcord {
    fn load(&mut self, key: &str) -> Option<Vec<u8>> {
        storage::get(Scope::Identity, key)
            .inspect_err(|e| log::warn(&format!("could not read the fit history: {e:?}")))
            .ok()
            .flatten()
    }

    fn save(&mut self, key: &str, value: &[u8]) {
        if let Err(e) = storage::set(Scope::Identity, key, value) {
            log::warn(&format!("could not save the fit history: {e:?}"));
        }
    }

    fn remove(&mut self, key: &str) {
        if let Err(e) = storage::delete(Scope::Identity, key) {
            log::warn(&format!("could not remove an old fit: {e:?}"));
        }
    }
}

struct EveFitPlugin;

impl Plugin for EveFitPlugin {
    fn start(_info: StartInfo) -> Result<(), String> {
        APP.with_borrow_mut(|app| app.start(&mut Mumcord));
        Ok(())
    }

    fn on_event(event: Event) {
        APP.with_borrow_mut(|app| handle(app, event));
    }

    fn stop() {}
}

fn handle(app: &mut App, event: Event) {
    let host = &mut Mumcord;
    app.on_event(host, &event);
    match event {
        Event::ChatMatch(m) => app.on_chat_match(host, &m.sender.name, &m.text),
        Event::UiAction(action) => match &action.source {
            ActionSource::Slot(Slot::ChatCard(id)) => {
                app.on_card_action(host, id, &action.action);
            }
            ActionSource::Slot(Slot::Panel) => app.on_panel_action(host, &action.action),
            _ => {}
        },
        Event::DialogClosed => app.on_dialog_closed(),
        Event::PanelVisibilityChanged(v) => {
            app.on_panel_visibility(host, matches!(v, PanelVisibility::Expanded));
        }
        Event::SettingsChanged(settings) => app.set_granted(host, &settings.granted),
        Event::WebviewMessage(message) => app.on_webview_message(host, &message),
        _ => {}
    }
}

mumcord_plugin_sdk::export!(EveFitPlugin);
