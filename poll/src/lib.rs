//! Poll: ask the channel a question and watch the votes come in live (FR-6.81).
//!
//! This file only connects Mumcord's events and host calls to the logic in [`app`].

mod app;
mod view;
mod wire;

#[cfg(test)]
mod tests;

use std::cell::RefCell;

use mumcord_plugin_sdk::{
    Plugin, chat,
    events::{ActionSource, ActionValue, Event},
    log, notify, peers, timers,
    types::{Error, StartInfo},
    ui::{self, El, Slot},
};

use app::{App, Host, To};

thread_local! {
    // A plugin instance is single-threaded; the host calls it one event at a time.
    static APP: RefCell<App> = RefCell::new(App::default());
}

/// The real host calls.
struct Mumcord;

impl Host for Mumcord {
    fn now(&mut self) -> u64 {
        mumcord_plugin_sdk::clock::now().unix_seconds
    }

    fn send(&mut self, to: To, kind: u8, payload: &[u8]) -> Result<(), Error> {
        let target = match to {
            To::Channel => peers::Target::Channel,
            To::Direct(session) => peers::Target::Direct(session),
        };
        peers::send(target, kind, payload)
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

    fn notify(&mut self, title: &str, body: &str) {
        if let Err(e) = notify::send(title, body) {
            log::debug(&format!("notification not shown: {e:?}"));
        }
    }

    fn start_timer(&mut self, ms: u32) -> Option<u32> {
        timers::start(ms, false)
            .inspect_err(|e| log::warn(&format!("could not start a timer: {e:?}")))
            .ok()
    }

    fn show(&mut self, slot: Slot, tree: El) {
        if let Err(e) = ui::show(slot, tree) {
            log::error(&format!("could not show the poll UI: {e:?}"));
        }
    }

    fn my_name(&mut self) -> Option<String> {
        // Needs `events.presence`; the user is the first entry of the snapshot.
        peers::presence().ok()?.into_iter().next().map(|p| p.name)
    }

    fn warn(&mut self, message: &str) {
        log::warn(message);
    }
}

struct PollPlugin;

impl Plugin for PollPlugin {
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
    if app.on_event(host, &event) {
        return;
    }
    match event {
        Event::UiAction(action) => {
            let card = match &action.source {
                ActionSource::Slot(Slot::ChatCard(id)) => Some(id.as_str()),
                ActionSource::Slot(Slot::Panel) => None,
                _ => return,
            };
            let text = match action.value {
                ActionValue::Text(text) => Some(text),
                _ => None,
            };
            app.on_action(host, card, &action.action, text);
        }
        Event::PeerMessage(message) => app.on_peer_message(
            host,
            &message.sender,
            message.kind,
            &message.payload,
            message.direct,
        ),
        Event::Joined(peer) => app.on_joined(host, &peer),
        Event::Left(peer) => app.on_left(host, &peer),
        Event::Timer(id) => app.on_timer(host, id),
        Event::SettingsChanged(settings) => app.set_granted(settings.granted),
        _ => {}
    }
}

mumcord_plugin_sdk::export!(PollPlugin);
