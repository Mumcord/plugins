//! A small plugin: a panel with a wave button that counts waves, stores the count and tells the
//! other users in the channel.

use std::cell::RefCell;

use mumcord_plugin_sdk::{
    Plugin,
    events::Event,
    i18n::Strings,
    log,
    peers::{self, Target},
    storage::{self, Scope},
    types::StartInfo,
    ui::{self, Animation, Slot},
};

const WAVE: u8 = 1;
const COUNT_KEY: &str = "waves";

struct Hello;

thread_local! {
    // Embeds i18n/en.json. Add a language by adding i18n/<tag>.json and its tag here.
    static STRINGS: RefCell<Strings> =
        RefCell::new(Strings::new(mumcord_plugin_sdk::i18n_embed!["en"]));
}

fn waves() -> u32 {
    storage::get_json(Scope::Identity, COUNT_KEY)
        .unwrap_or_else(|e| {
            log::warn(&format!("could not read the wave count: {e}"));
            None
        })
        .unwrap_or(0)
}

fn render(waves: u32) {
    let (heading, count, button) = STRINGS.with_borrow(|s| {
        (s.t("heading"), s.tn("waves", u64::from(waves), &[]), s.t("wave"))
    });
    let tree = ui::column()
        .child(ui::heading(&heading))
        .child(ui::text(&count))
        .child(ui::button("wave", &button).animate(Animation::Wave));
    if let Err(e) = ui::show(Slot::Panel, tree) {
        log::error(&format!("could not show the panel: {e}"));
    }
}

impl Plugin for Hello {
    fn start(_info: StartInfo) -> Result<(), String> {
        render(waves());
        Ok(())
    }

    fn on_event(event: Event) {
        match event {
            // The host sends the current language right after start and on every change.
            Event::LanguageChanged(tag) => {
                STRINGS.with_borrow_mut(|s| s.set_language(&tag));
                render(waves());
            }
            Event::UiAction(action) if action.action == "wave" => {
                let count = waves().saturating_add(1);
                if let Err(e) = storage::set_json(Scope::Identity, COUNT_KEY, &count) {
                    log::warn(&format!("could not store the wave count: {e}"));
                }
                // Nobody else in the channel is a normal case, not an error worth logging.
                let _ = peers::send_json(Target::Channel, WAVE, &());
                render(count);
            }
            Event::PeerMessage(message) if message.kind == WAVE => {
                log::info("someone waved");
            }
            _ => {}
        }
    }

    fn stop() {}
}

mumcord_plugin_sdk::export!(Hello);
