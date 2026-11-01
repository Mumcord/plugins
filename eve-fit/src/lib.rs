//! EVE fit: shows EVE Online fits pasted into the chat as a card with a full view.
//!
//! This file only connects Mumcord's events and host calls to the logic in [`app`].

mod app;
mod eft;
mod view;

#[cfg(test)]
mod tests;

use std::cell::RefCell;

use mumcord_plugin_sdk::{
    Plugin,
    events::{ActionSource, Event},
    log,
    types::{Error, StartInfo},
    ui::{self, El, Slot},
};

use app::{App, Host};

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

    fn warn(&mut self, message: &str) {
        log::warn(message);
    }
}

struct EveFitPlugin;

impl Plugin for EveFitPlugin {
    fn start(_info: StartInfo) -> Result<(), String> {
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
        Event::UiAction(action) => {
            if let ActionSource::Slot(Slot::ChatCard(id)) = &action.source {
                app.on_card_action(host, id, &action.action);
            }
        }
        Event::DialogClosed => app.on_dialog_closed(),
        _ => {}
    }
}

mumcord_plugin_sdk::export!(EveFitPlugin);
