//! The plugin's translations. A plugin instance is single-threaded, so one `Strings` per thread
//! (native tests run each test on its own thread and therefore start in English).
//!
//! The page's own strings live in the same files under the `page.` prefix, so translators see
//! one file per language; [`page_strings`] hands them to the page in `init`. Page strings use
//! the page's `{name}` placeholders, the plugin's own strings the SDK's `{{name}}`.

use std::cell::RefCell;
use std::collections::BTreeMap;

use mumcord_plugin_sdk::{events::Event, i18n::Strings};

/// Prefix of the page's keys in `i18n/<lang>.json`.
pub const PAGE_PREFIX: &str = "page.";
/// Every key the page uses, with its English text (the page's own fallback).
const PAGE_KEYS: &str = include_str!("../web/board/strings-keys.json");
/// Characters the host shows in one UI text.
pub const MAX_UI_CHARS: usize = 200;

thread_local! {
    static STRINGS: RefCell<Strings> = RefCell::new(Strings::new(mumcord_plugin_sdk::i18n_all!()));
    static LANG: RefCell<String> = RefCell::new("en".to_owned());
}

pub fn t(key: &str) -> String {
    STRINGS.with_borrow(|s| s.t(key))
}

pub fn t_with(key: &str, args: &[(&str, &str)]) -> String {
    STRINGS.with_borrow(|s| s.t_with(key, args))
}

pub fn tn(key: &str, count: u64, args: &[(&str, &str)]) -> String {
    STRINGS.with_borrow(|s| s.tn(key, count, args))
}

/// The UI language tag as the host reported it.
pub fn language() -> String {
    LANG.with_borrow(Clone::clone)
}

/// True when the event switched the language, so the shown UI must be rendered again.
pub fn on_event(event: &Event) -> bool {
    if let Event::LanguageChanged(tag) = event {
        LANG.with_borrow_mut(|l| tag.chars().take(16).collect::<String>().clone_into(l));
    }
    STRINGS.with_borrow_mut(|s| s.on_event(event))
}

/// The page's keys (sorted).
pub fn page_keys() -> Vec<String> {
    serde_json::from_str::<BTreeMap<String, String>>(PAGE_KEYS)
        .map(|m| m.into_keys().collect())
        .unwrap_or_default()
}

/// The page's strings in the current language, without the prefix.
pub fn page_strings() -> BTreeMap<String, String> {
    page_keys()
        .into_iter()
        .map(|k| {
            let v = t(&format!("{PAGE_PREFIX}{k}"));
            (k, v)
        })
        .collect()
}

/// Cuts `s` to what the host shows in one UI text.
pub fn clip(s: &str) -> String {
    clip_to(s, MAX_UI_CHARS)
}

pub fn clip_to(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: &str = include_str!("../i18n/en.json");

    #[test]
    fn every_page_key_is_in_the_english_file_with_the_same_text() {
        let page: BTreeMap<String, String> = serde_json::from_str(PAGE_KEYS).expect("page keys");
        let en: BTreeMap<String, String> = serde_json::from_str(EN).expect("en.json");
        assert!(!page.is_empty());
        for (k, v) in &page {
            assert_eq!(en.get(&format!("{PAGE_PREFIX}{k}")), Some(v), "{k}");
        }
        let strings = page_strings();
        assert_eq!(strings.len(), page.len());
        assert_eq!(strings.get("app.title"), page.get("app.title"));
    }

    #[test]
    fn clipping_keeps_short_text() {
        assert_eq!(clip("abc"), "abc");
        assert_eq!(clip_to("abcdef", 4), "abc…");
    }
}
