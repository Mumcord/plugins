//! Whiteboard: one shared, endless board for everyone in a voice channel.

pub mod codec;
pub mod model;

use mumcord_plugin_sdk::{Plugin, events::Event, types::StartInfo};

struct Whiteboard;

impl Plugin for Whiteboard {
    fn start(_info: StartInfo) -> Result<(), String> {
        Ok(())
    }

    fn on_event(_event: Event) {}

    fn stop() {}
}

mumcord_plugin_sdk::export!(Whiteboard);
