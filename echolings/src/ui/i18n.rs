//! UI strings from the package's `i18n/<lang>.json` files, through the SDK helper.
//!
//! All 24 Mumcord languages are compiled in. Lookup follows the host's order: the app language,
//! then its base language, then English.

#[cfg(test)]
use std::cell::RefCell;

use mumcord_plugin_sdk::i18n::Strings;

pub struct I18n {
    strings: Strings,
    /// Keys asked for but not found; only tests read it.
    #[cfg(test)]
    missing: RefCell<Vec<String>>,
}

impl I18n {
    pub fn new(language: &str) -> I18n {
        let mut strings = Strings::new(mumcord_plugin_sdk::i18n_all!());
        strings.set_language(language);
        I18n {
            strings,
            #[cfg(test)]
            missing: RefCell::default(),
        }
    }

    /// Switches the language; the caller renders again afterwards.
    pub fn set_language(&mut self, language: &str) {
        self.strings.set_language(language);
    }

    /// The string for `key`, or the key itself if it is missing.
    pub fn t(&self, key: &str) -> String {
        self.track(key, self.strings.t(key))
    }

    /// The string for `key` with `{{name}}` placeholders filled in.
    pub fn f(&self, key: &str, args: &[(&str, &str)]) -> String {
        self.track(key, self.strings.t_with(key, args))
    }

    /// The plural form of `key` for `count`, with `{{count}}` filled in.
    pub fn n(&self, key: &str, count: u64) -> String {
        let text = self.strings.tn(key, count, &[]);
        self.track(key, text)
    }

    /// A translation is never equal to its own key, so an unchanged key marks a miss.
    #[cfg(test)]
    fn track(&self, key: &str, text: String) -> String {
        if text == key
            || text
                .strip_prefix(key)
                .is_some_and(|rest| rest.starts_with('_'))
        {
            self.missing.borrow_mut().push(key.to_owned());
        }
        text
    }

    #[cfg(not(test))]
    fn track(&self, _key: &str, text: String) -> String {
        text
    }

    #[cfg(test)]
    pub fn missing(&self) -> Vec<String> {
        self.missing.borrow().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeSet, HashMap};

    const LANGUAGES: [&str; 24] = [
        "bg", "cs", "da", "de", "el", "en", "es", "et", "fi", "fr", "ga", "hr", "hu", "it", "lt",
        "lv", "mt", "nl", "pl", "pt", "ro", "sk", "sl", "sv",
    ];

    fn load(tag: &str) -> HashMap<String, String> {
        let path = format!("{}/i18n/{tag}.json", env!("CARGO_MANIFEST_DIR"));
        let json = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        serde_json::from_str(&json).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    /// Plural keys beyond English's `_one`/`_other` carry the extra CLDR categories.
    fn base_key(key: &str) -> String {
        for extra in ["_zero", "_two", "_few", "_many"] {
            if let Some(base) = key.strip_suffix(extra) {
                return format!("{base}_other");
            }
        }
        key.to_owned()
    }

    fn placeholders(text: &str) -> BTreeSet<String> {
        text.split("{{")
            .skip(1)
            .filter_map(|rest| rest.split_once("}}"))
            .map(|(name, _)| name.trim().to_owned())
            .collect()
    }

    #[test]
    fn every_language_has_the_keys_of_english() {
        let english = load("en");
        let keys: BTreeSet<_> = english.keys().cloned().collect();
        for tag in LANGUAGES {
            let table = load(tag);
            let own: BTreeSet<_> = table.keys().map(|k| base_key(k)).collect();
            assert_eq!(own, keys, "i18n/{tag}.json differs from en.json");
            for (key, text) in &table {
                assert_eq!(
                    placeholders(text),
                    placeholders(&english[&base_key(key)]),
                    "i18n/{tag}.json: placeholders of {key}"
                );
            }
        }
    }

    #[test]
    fn falls_back_to_english_and_fills_placeholders() {
        let t = I18n::new("de-AT");
        assert_eq!(t.t("action.pat"), "Streicheln");
        assert_eq!(t.f("stat.bond", &[("bond", "4")]), "Bindung 4");
        assert_eq!(t.t("no.such.key"), "no.such.key");
        let unknown = I18n::new("xx");
        assert_eq!(unknown.t("action.pat"), "Pat");
    }

    #[test]
    fn counted_strings_follow_the_language_plural_rules() {
        let en = I18n::new("en");
        assert_eq!(en.n("stat.together", 1), "1 day together");
        assert_eq!(en.n("stat.together", 0), "0 days together");
        assert_eq!(en.n("stat.together", 2), "2 days together");
        let de = I18n::new("de");
        assert_eq!(de.n("stat.together", 1), "1 Tag zusammen");
        assert_eq!(de.n("stat.together", 3), "3 Tage zusammen");
    }
}
