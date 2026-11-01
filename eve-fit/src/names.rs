//! Item names from non-English EVE clients, from `names.txt` (written by
//! `web-src/scripts/items.mjs` from the same EVE data the web view bundles). The fitting wheel
//! only reads English names, so a fit copied from, say, a French or Japanese client is turned
//! into its English form before it goes to the page; the card and Copy fitting keep the text as
//! pasted.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

const NAMES: &str = include_str!("names.txt");

/// The English name of the item with this name in another language EVE supports, in any case.
/// `None` for English names and for names the data does not know.
pub fn english(name: &str) -> Option<&'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    let table = TABLE.get_or_init(table);
    // The table holds the names lowercased per code point, as EVE's data does, with no-break
    // spaces as spaces and without zero-width spaces.
    let lower: String = name
        .chars()
        .filter(|&c| c != '\u{200b}')
        .map(|c| if c == '\u{a0}' { ' ' } else { c })
        .flat_map(char::to_lowercase)
        .collect();
    table.get(lower.as_str()).copied()
}

fn table() -> HashMap<&'static str, &'static str> {
    let mut table = HashMap::new();
    for line in NAMES.lines().filter(|l| !l.starts_with('#')) {
        let mut names = line.split('\t');
        let Some(english) = names.next() else {
            continue;
        };
        for name in names {
            table.insert(name, english);
        }
    }
    table
}

/// The fit with every item name the data knows in English, line by line, so the layout stays as
/// pasted. Borrowed when there is nothing to change, e.g. for a fit from an English client.
pub fn canonical(text: &str) -> Cow<'_, str> {
    let mut changed = false;
    let lines: Vec<Cow<str>> = text
        .split('\n')
        .map(|line| match line.strip_suffix('\r') {
            // Windows line ends stay as pasted.
            Some(bare) => match canonical_line(bare) {
                Cow::Owned(english) => {
                    changed = true;
                    Cow::Owned(english + "\r")
                }
                Cow::Borrowed(_) => Cow::Borrowed(line),
            },
            None => {
                let line = canonical_line(line);
                changed |= matches!(line, Cow::Owned(_));
                line
            }
        })
        .collect();
    if changed {
        Cow::Owned(lines.join("\n"))
    } else {
        Cow::Borrowed(text)
    }
}

/// One EFT line: `[Ship, Fit name]`, `Module, Charge /OFFLINE`, `Item x5`, each with an optional
/// ` [1]` mutation reference.
fn canonical_line(line: &str) -> Cow<'_, str> {
    // Chat clients turn spaces into no-break spaces now and then.
    let clean = line.trim().replace('\u{a0}', " ");
    if let Some(inner) = clean.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
        return match inner.split_once(',') {
            Some((ship, name)) => match english(ship.trim()) {
                Some(ship) => Cow::Owned(format!("[{ship},{name}]")),
                None => Cow::Borrowed(line),
            },
            None => Cow::Borrowed(line),
        };
    }
    let (rest, offline) = match clean.strip_suffix("/OFFLINE") {
        Some(rest) => (rest.trim_end(), " /OFFLINE"),
        None => (clean.as_str(), ""),
    };
    if let Some(name) = english_item(rest) {
        return Cow::Owned(format!("{name}{offline}"));
    }
    if let Some((name, count)) = rest.rsplit_once(" x")
        && !count.is_empty()
        && count.bytes().all(|b| b.is_ascii_digit())
        && let Some(name) = english_item(name)
    {
        return Cow::Owned(format!("{name} x{count}{offline}"));
    }
    if let Some((module, charge)) = rest.split_once(',') {
        let module_en = english_item(module);
        let charge_en = english(charge.trim());
        if module_en.is_some() || charge_en.is_some() {
            let module = module_en.unwrap_or_else(|| module.trim().to_owned());
            let charge = charge_en.unwrap_or(charge.trim());
            return Cow::Owned(format!("{module}, {charge}{offline}"));
        }
    }
    Cow::Borrowed(line)
}

/// The English name of an item, keeping a ` [1]` mutation reference.
fn english_item(text: &str) -> Option<String> {
    let text = text.trim();
    if let Some(name) = english(text) {
        return Some(name.to_owned());
    }
    let start = text.rfind(" [")?;
    let reference = &text[start..];
    let digits = reference.strip_prefix(" [")?.strip_suffix(']')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    english(text[..start].trim_end()).map(|name| format!("{name}{reference}"))
}
