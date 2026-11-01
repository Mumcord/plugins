//! The plugin's translations. A plugin instance is single-threaded, so one `Strings` per thread
//! (native tests run each test on its own thread and therefore start in English).

use std::cell::RefCell;

use mumcord_plugin_sdk::{events::Event, i18n::Strings};

thread_local! {
    static STRINGS: RefCell<Strings> = RefCell::new(Strings::new(mumcord_plugin_sdk::i18n_all!()));
}

pub fn t(key: &str) -> String {
    STRINGS.with_borrow(|s| s.t(key))
}

pub fn t_with(key: &str, args: &[(&str, &str)]) -> String {
    STRINGS.with_borrow(|s| s.t_with(key, args))
}

pub fn tn(key: &str, count: u64) -> String {
    STRINGS.with_borrow(|s| s.tn(key, count, &[]))
}

/// True when the event switched the language, so the shown UI must be rendered again.
pub fn on_event(event: &Event) -> bool {
    STRINGS.with_borrow_mut(|s| s.on_event(event))
}

#[cfg(test)]
pub fn set_language(tag: &str) {
    STRINGS.with_borrow_mut(|s| s.set_language(tag));
}
