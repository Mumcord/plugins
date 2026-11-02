//! Where EVE's items go on a ship, from `items.txt` (written by `web-src/scripts/items.mjs` from
//! the same EVE data the web view bundles). It sorts the lines of fits whose blank lines got
//! lost, e.g. on the way through a chat composer.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::eft::Section;
use crate::names;

const ITEMS: &str = include_str!("items.txt");

/// The section an item with this name, in English or another language EVE supports (see
/// [`names`]), belongs to: a slot for modules, `Drones` for drones and fighters, `Other` for
/// implants and boosters. `None` for anything else (charges, deployables, …) and for names the
/// data does not know.
pub fn section(name: &str) -> Option<Section> {
    static TABLE: OnceLock<HashMap<&'static str, Section>> = OnceLock::new();
    let table = TABLE.get_or_init(table);
    table
        .get(name)
        .or_else(|| names::english(name).and_then(|english| table.get(english)))
        .copied()
}

fn table() -> HashMap<&'static str, Section> {
    let mut table = HashMap::new();
    let mut current = None;
    for line in ITEMS.lines() {
        if line.starts_with('#') {
            continue;
        }
        if let Some(kind) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = match kind {
                "low" => Some(Section::Low),
                "mid" => Some(Section::Mid),
                "high" => Some(Section::High),
                "rig" => Some(Section::Rig),
                "subsystem" => Some(Section::Subsystem),
                "drone" => Some(Section::Drones),
                "other" => Some(Section::Other),
                _ => None,
            };
            continue;
        }
        if let Some(section) = current {
            table.entry(line).or_insert(section);
        }
    }
    table
}
