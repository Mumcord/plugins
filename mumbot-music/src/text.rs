//! The plugin's texts in the user's language. The host sends the language right after start and
//! on every change; until then, and for any missing key, English is used.

use std::cell::RefCell;

use mumcord_plugin_sdk::i18n::Strings;

thread_local! {
    // A plugin instance is single-threaded; the host calls it one event at a time.
    static STRINGS: RefCell<Strings> = RefCell::new(Strings::new(mumcord_plugin_sdk::i18n_all!()));
}

pub fn t(key: &str) -> String {
    STRINGS.with_borrow(|s| s.t(key))
}

pub fn t_with(key: &str, args: &[(&str, &str)]) -> String {
    STRINGS.with_borrow(|s| s.t_with(key, args))
}

pub fn set_language(tag: &str) {
    STRINGS.with_borrow_mut(|s| s.set_language(tag));
}
