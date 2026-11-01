//! Dice roller: `/roll` in chat with rolls everyone can check (FR-6.82).
//!
//! This file only connects Mumcord's events and host calls to the logic in [`app`].

mod app;
mod fair;
mod notation;
mod view;
mod wire;

#[cfg(test)]
mod tests;

use std::cell::RefCell;

use mumcord_plugin_sdk::{
    Plugin, chat,
    events::{ActionSource, ActionValue, Event},
    log, peers, random, timers,
    types::{Error, StartInfo},
    ui::{self, El, Slot},
};

use app::{App, Host};
use wire::Message;

/// The chat command from the manifest.
const ROLL_COMMAND: &str = "roll";

thread_local! {
    // A plugin instance is single-threaded; the host calls it one event at a time.
    static APP: RefCell<App> = RefCell::new(App::default());
}

/// The real host calls.
struct Mumcord;

impl Host for Mumcord {
    fn random(&mut self, len: u32) -> Option<Vec<u8>> {
        random::bytes(len)
            .inspect_err(|e| log::warn(&format!("no random bytes: {e:?}")))
            .ok()
    }

    fn send(&mut self, message: &Message) -> Result<(), Error> {
        peers::send(peers::Target::Channel, message.kind(), &message.encode())
    }

    fn start_game_burst(&mut self) {
        // Refused at most once a minute or while another plugin bursts; the roll then uses the
        // normal budget.
        if let Err(e) = peers::start_game_burst() {
            log::debug(&format!("no game burst: {e:?}"));
        }
    }

    fn chat_send(&mut self, text: &str) -> Result<(), Error> {
        chat::send(text)
    }

    fn line_hash(&mut self, text: &str) -> u32 {
        chat::line_hash(text)
    }

    fn supersede(&mut self, card: &str, sender: u32, hash: u32) {
        if let Err(e) = chat::supersede(card, sender, hash) {
            log::debug(&format!("could not supersede the text line: {e:?}"));
        }
    }

    fn start_timer(&mut self, ms: u32) -> Option<u32> {
        timers::start(ms, false)
            .inspect_err(|e| log::warn(&format!("could not start a timer: {e:?}")))
            .ok()
    }

    fn show(&mut self, slot: Slot, tree: El) {
        if let Err(e) = ui::show(slot, tree) {
            log::error(&format!("could not show the dice UI: {e:?}"));
        }
    }

    fn warn(&mut self, message: &str) {
        log::warn(message);
    }
}

struct DicePlugin;

impl Plugin for DicePlugin {
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
    match event {
        Event::UiAction(action) => {
            if matches!(action.source, ActionSource::Command) && action.action == ROLL_COMMAND {
                let args = match &action.value {
                    ActionValue::Text(text) => text.as_str(),
                    _ => "",
                };
                app.on_command(host, args);
            }
        }
        Event::PeerMessage(message) => app.on_peer_message(
            host,
            message.sender.session,
            &message.sender.name,
            message.kind,
            &message.payload,
            message.direct,
        ),
        Event::Timer(id) => app.on_timer(host, id),
        Event::SettingsChanged(settings) => app.set_granted(host, &settings.granted),
        _ => {}
    }
}

mumcord_plugin_sdk::export!(DicePlugin);
