//! Whiteboard: one shared, endless board for everyone in a voice channel.
//!
//! The pure parts (board model, wire codec, sync engine, store, export, page bridge, dialogs)
//! never touch the host. [`app`] wires them to Mumcord's events through its `Host` trait; this
//! file implements that trait with the SDK and exports the plugin.

mod app;
pub mod bridge;
pub mod codec;
mod dialogs;
pub mod export;
pub mod model;
pub mod store;
pub mod sync;
mod text;

#[cfg(test)]
mod tests;

use std::cell::RefCell;

use mumcord_plugin_sdk::{
    Plugin, chat, clock,
    events::Event,
    files, log, peers, random, relay, stage, storage, timers,
    types::{Error, StartInfo},
    ui::{self, El, Slot},
    webview::{self, Place},
};

use app::{App, Host, Level};
use sync::{PeerInfo, SessionId, Target};

thread_local! {
    // A plugin instance is single-threaded; the host calls it one event at a time.
    static APP: RefCell<App> = RefCell::new(App::default());
}

/// The real host calls.
struct Mumcord;

/// Boards are personal: they live in the identity's own storage.
const SCOPE: storage::Scope = storage::Scope::Identity;

impl store::Storage for Mumcord {
    fn get(&mut self, key: &str) -> Result<Option<Vec<u8>>, String> {
        storage::get(SCOPE, key).map_err(|e| format!("{e:?}"))
    }

    fn set(&mut self, key: &str, value: &[u8]) -> Result<(), String> {
        storage::set(SCOPE, key, value).map_err(|e| format!("{e:?}"))
    }

    fn delete(&mut self, key: &str) -> Result<(), String> {
        storage::delete(SCOPE, key).map_err(|e| format!("{e:?}"))
    }

    fn list_keys(&mut self, prefix: &str) -> Result<Vec<String>, String> {
        storage::list_keys(SCOPE, prefix).map_err(|e| format!("{e:?}"))
    }
}

fn peers_target(target: Target) -> peers::Target {
    match target {
        Target::Channel => peers::Target::Channel,
        Target::Direct(s) => peers::Target::Direct(s),
    }
}

impl Host for Mumcord {
    fn wall_ms(&mut self) -> u64 {
        clock::now().unix_seconds.saturating_mul(1000)
    }

    fn random(&mut self, len: u32) -> Option<Vec<u8>> {
        random::bytes(len)
            .inspect_err(|e| log::warn(&format!("no random bytes: {e:?}")))
            .ok()
    }

    fn presence(&mut self) -> Vec<PeerInfo> {
        peers::presence()
            .map(|list| list.iter().map(app::peer_info).collect())
            .unwrap_or_default()
    }

    fn peers_send(&mut self, target: Target, kind: u8, payload: &[u8]) -> Result<(), Error> {
        peers::send(peers_target(target), kind, payload)
    }

    fn game_burst(&mut self) -> bool {
        peers::start_game_burst().is_ok()
    }

    fn relay_available(&mut self) -> bool {
        relay::status() == relay::RelayStatus::Available
    }

    fn relay_send(&mut self, target: Target, payload: &[u8]) -> Result<(), Error> {
        // No Mumble fallback: the sync engine picks the transport itself.
        match target {
            Target::Channel => relay::send_channel(payload, false),
            Target::Direct(s) => relay::send_direct(s, payload, false),
        }
    }

    fn stage_start(&mut self, title: &str, entry: &str) -> Result<String, Error> {
        stage::start_web(title, entry)
    }

    fn stage_join(&mut self, activity: &str) -> Result<(), Error> {
        stage::join(activity)
    }

    fn webview_show(&mut self, place: Place, entry: &str) -> Result<(), Error> {
        webview::show(place, entry)
    }

    fn webview_post(&mut self, message: &str) -> Result<(), Error> {
        webview::post(message)
    }

    fn show(&mut self, slot: Slot, tree: El) -> Result<(), Error> {
        ui::show(slot, tree)
    }

    fn open_dialog(&mut self, title: &str) -> Result<(), Error> {
        ui::open_dialog(title)
    }

    fn close_dialog(&mut self) {
        ui::close_dialog();
    }

    fn chat_send(&mut self, text: &str) -> Result<(), Error> {
        chat::send(text)
    }

    fn line_hash(&mut self, text: &str) -> u32 {
        chat::line_hash(text)
    }

    fn supersede(&mut self, card: &str, sender: SessionId, hash: u32) -> Result<(), Error> {
        chat::supersede(card, sender, hash)
    }

    fn post_image(&mut self, png: &[u8], alt: &str) -> Result<chat::PostOutcome, Error> {
        chat::post_image(png, alt)
    }

    fn save_file(&mut self, name: &str, data: &[u8]) -> Result<bool, Error> {
        files::save(name, data)
    }

    fn start_timer(&mut self, ms: u32) -> Option<u32> {
        timers::start(ms, true)
            .inspect_err(|e| log::warn(&format!("could not start a timer: {e:?}")))
            .ok()
    }

    fn cancel_timer(&mut self, id: u32) {
        timers::cancel(id);
    }

    fn log(&mut self, level: Level, message: &str) {
        match level {
            Level::Debug => log::debug(message),
            Level::Warn => log::warn(message),
            Level::Error => log::error(message),
        }
    }
}

struct Whiteboard;

impl Plugin for Whiteboard {
    fn start(_info: StartInfo) -> Result<(), String> {
        Ok(())
    }

    fn on_event(event: Event) {
        APP.with_borrow_mut(|app| app.on_event(&mut Mumcord, event));
    }

    fn stop() {}
}

mumcord_plugin_sdk::export!(Whiteboard);
