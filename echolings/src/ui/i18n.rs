//! UI strings from the package's `i18n/<lang>.json` files.
//!
//! The plugin API has no call to read package files, so the same files the host uses for the
//! plugin's name are compiled in. Lookup follows the host's order: the app language, then its
//! base language, then English.

use std::cell::RefCell;
use std::collections::HashMap;

/// Every shipped language. Adding a translation = adding its file here and under `i18n/`.
const FILES: &[(&str, &str)] = &[("en", include_str!("../../i18n/en.json"))];

pub struct I18n {
    strings: HashMap<String, String>,
    /// Keys asked for but not found; only tests read it.
    missing: RefCell<Vec<String>>,
}

impl I18n {
    pub fn new(language: &str) -> I18n {
        let mut strings = HashMap::new();
        // Lowest priority first, so better matches overwrite.
        for tag in fallback_chain(language).iter().rev() {
            if let Some((_, json)) = FILES.iter().find(|(t, _)| t == tag) {
                strings.extend(parse(json));
            }
        }
        I18n {
            strings,
            missing: RefCell::default(),
        }
    }

    /// The string for `key`, or the key itself if it is missing.
    pub fn t<'a>(&'a self, key: &'a str) -> &'a str {
        match self.strings.get(key) {
            Some(text) => text,
            None => {
                self.missing.borrow_mut().push(key.to_owned());
                key
            }
        }
    }

    /// The string for `key` with `{name}` placeholders filled in.
    pub fn f(&self, key: &str, args: &[(&str, &str)]) -> String {
        args.iter()
            .fold(self.t(key).to_owned(), |text, (name, value)| {
                text.replace(&format!("{{{name}}}"), value)
            })
    }

    #[cfg(test)]
    pub fn missing(&self) -> Vec<String> {
        self.missing.borrow().clone()
    }
}

fn parse(json: &str) -> HashMap<String, String> {
    // The files are checked by a test, so a broken one can only come from a bad build.
    serde_json::from_str(json).unwrap_or_default()
}

/// The key to use for a counted string: `{key}.one` for exactly one, otherwise `key`.
/// A singular key only needs to exist in the languages that have one; the others fall back.
pub fn count_key(key: &str, count: u32) -> String {
    if count == 1 {
        format!("{key}.one")
    } else {
        key.to_owned()
    }
}

/// `de-AT` → `de-AT`, `de`, `en`.
fn fallback_chain(language: &str) -> Vec<String> {
    let mut chain = vec![language.to_owned()];
    if let Some((base, _)) = language.split_once('-') {
        chain.push(base.to_owned());
    }
    chain.push("en".to_owned());
    chain.dedup();
    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_parse_and_translate_only_english_keys() {
        let english = parse(FILES[0].1);
        assert!(english.len() > 50);
        for (tag, json) in FILES {
            let table: HashMap<String, String> =
                serde_json::from_str(json).unwrap_or_else(|e| panic!("i18n/{tag}.json: {e}"));
            for key in table.keys() {
                assert!(english.contains_key(key), "{tag}: unknown key {key}");
            }
        }
    }

    #[test]
    fn falls_back_to_english_and_fills_placeholders() {
        let t = I18n::new("de-AT");
        assert_eq!(t.t("action.pat"), "Pat");
        assert_eq!(t.f("stat.bond", &[("bond", "4")]), "Bond 4");
        assert_eq!(t.t("no.such.key"), "no.such.key");
        assert_eq!(t.missing(), vec!["no.such.key".to_owned()]);
        assert_eq!(fallback_chain("en"), vec!["en".to_owned()]);
    }

    #[test]
    fn counted_strings_use_the_singular_only_for_one() {
        let t = I18n::new("en");
        let text = |days: u32| {
            let shown = days.to_string();
            t.f(&count_key("stat.together", days), &[("days", &shown)])
        };
        assert_eq!(text(1), "1 day together");
        assert_eq!(text(0), "0 days together");
        assert_eq!(text(2), "2 days together");
        assert!(t.missing().is_empty());
    }
}
