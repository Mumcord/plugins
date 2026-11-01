//! A parser for EVE Online fits in EFT text form, as the game's "Copy to clipboard" and fitting
//! tools such as Pyfa export them:
//!
//! ```text
//! [Rifter, Tackle]
//! Damage Control II
//! [Empty Low slot]
//!
//! 5MN Microwarpdrive II
//! Warp Scrambler II
//!
//! 200mm AutoCannon II, Republic Fleet EMP S
//! 200mm AutoCannon II, Republic Fleet EMP S /OFFLINE
//!
//! Small Projectile Burst Aerator I
//!
//!
//! Warrior II x3
//!
//! Nanite Repair Paste x50
//! ```
//!
//! Module sections come in a fixed order (low, mid, high, rigs, subsystems) and are separated by
//! blank lines; an `[Empty Mid slot]` marker pins a section to its kind when a section is
//! missing. Sections whose lines all end in `xN` are drones and cargo, which their order tells
//! apart. Blank lines do not always survive the chat, so each item first goes where EVE's data
//! (see [`items`]) says it goes; the layout only decides for items the data does not know.
//!
//! The text comes from other people in the chat: it is untrusted, so the parser is bounded in
//! size and never panics.

use std::borrow::Cow;

use crate::items;

/// The longest text the parser looks at; the host forwards at most 4 KiB as well.
pub const MAX_TEXT_BYTES: usize = 4096;
/// More lines than any real fit has (a full ship with drones and cargo is about 60).
pub const MAX_LINES: usize = 200;
/// Item and ship names in EVE are short; longer ones are cut so the UI stays tidy.
pub const MAX_NAME_CHARS: usize = 100;

/// Where a group of items sits on the ship.
/// In display order: slots, then bays, then the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Section {
    Low,
    Mid,
    High,
    Rig,
    Subsystem,
    Drones,
    /// A single `xN` block of items EVE's data does not know: drones or cargo.
    DronesOrCargo,
    Cargo,
    /// Lines after the drones that are neither (implants, boosters).
    Other,
}

impl Section {
    /// The module sections in EFT order.
    const MODULES: [Section; 5] = [
        Section::Low,
        Section::Mid,
        Section::High,
        Section::Rig,
        Section::Subsystem,
    ];

    /// Slots on the ship, which can be empty; the rest are item bays.
    pub fn is_slot(self) -> bool {
        Self::MODULES.contains(&self)
    }
}

/// One kind of item in a section: identical lines are counted together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub charge: Option<String>,
    pub count: u32,
    pub offline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub section: Section,
    pub items: Vec<Item>,
    /// `[Empty … slot]` markers in this section.
    pub empty: u32,
}

impl Group {
    /// Fitted modules, or the number of units in a bay.
    pub fn filled(&self) -> u32 {
        self.items.iter().map(|i| i.count).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fit {
    pub ship: String,
    /// The fit's name; may be empty.
    pub name: String,
    pub groups: Vec<Group>,
}

impl Fit {
    #[cfg(test)]
    pub fn group(&self, section: Section) -> Option<&Group> {
        self.groups.iter().find(|g| g.section == section)
    }
}

/// Parses an EFT fit. `None` when the text is not one: no `[Ship, Name]` header on the first
/// line, too long, or no item or empty slot at all.
pub fn parse(text: &str) -> Option<Fit> {
    if text.len() > MAX_TEXT_BYTES {
        return None;
    }
    // Chat clients turn spaces into no-break spaces now and then.
    let text = text.replace('\u{a0}', " ");
    let mut lines = text.lines().map(str::trim);
    let (ship, name) = header(lines.next()?)?;

    let mut blocks: Vec<Vec<&str>> = vec![Vec::new()];
    for (n, line) in lines.enumerate() {
        if n >= MAX_LINES {
            return None;
        }
        if line.is_empty() {
            if blocks.last().is_some_and(|b| !b.is_empty()) {
                blocks.push(Vec::new());
            }
        } else {
            blocks.last_mut()?.push(line);
        }
    }
    blocks.retain(|b| !b.is_empty());

    let groups = classify(blocks);
    if groups.iter().all(|g| g.items.is_empty() && g.empty == 0) {
        return None;
    }
    Some(Fit { ship, name, groups })
}

/// The fit with every block of `xN` lines that mixes drones with other items split in two:
/// the drones, a blank line, then the rest. The fitting wheel's engine puts such a mixed block,
/// common when a fit is written by hand or its blank lines got lost, into the cargo whole, so
/// the drones would not be in the drone bay. Everything else stays as pasted; borrowed when
/// nothing changes.
pub fn separate_drones(text: &str) -> Cow<'_, str> {
    let lines: Vec<&str> = text.split('\n').collect();
    let is_drone = |line: &str| {
        let line = line.trim().replace('\u{a0}', " ");
        quantity(&line).is_some_and(|(name, _)| items::section(name) == Some(Section::Drones))
    };
    let mut out: Vec<&str> = Vec::with_capacity(lines.len() + 1);
    let mut changed = false;
    for block in lines.split_inclusive(|l| l.trim().is_empty()) {
        let (items, blank) = match block.split_last() {
            Some((last, rest)) if last.trim().is_empty() => (rest, Some(*last)),
            _ => (block, None),
        };
        let all_bay = items
            .iter()
            .all(|l| quantity(&l.trim().replace('\u{a0}', " ")).is_some());
        let drones = items.iter().filter(|l| is_drone(l)).count();
        if !items.is_empty() && all_bay && drones > 0 && drones < items.len() {
            changed = true;
            out.extend(items.iter().filter(|l| is_drone(l)));
            // The blank line keeps the line ends of the text, `\r` included.
            out.push(if items[0].ends_with('\r') { "\r" } else { "" });
            out.extend(items.iter().filter(|l| !is_drone(l)));
        } else {
            out.extend(items);
        }
        out.extend(blank);
    }
    if changed {
        Cow::Owned(out.join("\n"))
    } else {
        Cow::Borrowed(text)
    }
}

/// `[Ship, Fit name]`; the fit name may contain commas and may be empty.
fn header(line: &str) -> Option<(String, String)> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?;
    let (ship, name) = inner.split_once(',')?;
    let ship = ship.trim();
    if ship.is_empty() || ship.starts_with("Empty ") {
        return None;
    }
    Some((cut(ship), cut(name.trim())))
}

/// Turns the blocks into sections. A line goes where EVE's data says its item goes (see
/// [`items`]), so fits whose blank lines got lost still come out right; lines the data does not
/// know go by the block they are in: module blocks in EFT order (or by their empty-slot marker),
/// then the `xN` blocks as drones and cargo.
fn classify(blocks: Vec<Vec<&str>>) -> Vec<Group> {
    // The details of mutated modules ("[1] Abyssal Gyrostabilizer" and its attribute lines)
    // repeat what the module lines already say.
    let blocks: Vec<Vec<&str>> = blocks
        .into_iter()
        .filter(|b| !(b[0].starts_with('[') && b[0][1..].starts_with(|c: char| c.is_ascii_digit())))
        .collect();
    let is_bay = |block: &[&str]| block.iter().all(|l| quantity(l).is_some());
    let bay_blocks = blocks.iter().filter(|b| is_bay(b)).count();

    let mut groups: Vec<Group> = Vec::new();
    // Index into Section::MODULES of the next module block.
    let mut next_module = 0;
    let mut bays_seen = 0;
    for block in &blocks {
        if is_bay(block) {
            let position = match (bay_blocks, bays_seen) {
                (1, _) => Section::DronesOrCargo,
                (_, 0) => Section::Drones,
                _ => Section::Cargo,
            };
            bays_seen += 1;
            let knows_any = block
                .iter()
                .any(|l| quantity(l).is_some_and(|(name, _)| items::section(name).is_some()));
            for line in block {
                let Some((name, count)) = quantity(line) else {
                    continue;
                };
                let section = match (position, items::section(name)) {
                    // Only a single bay leaves drones and cargo to the data.
                    (Section::DronesOrCargo, Some(Section::Drones)) => Section::Drones,
                    (Section::DronesOrCargo, None) if !knows_any => Section::DronesOrCargo,
                    (Section::DronesOrCargo, _) => Section::Cargo,
                    (position, _) => position,
                };
                put(&mut groups, section, bay_item(name, count));
            }
            continue;
        }
        // Where the block's unknown lines go.
        let position = if bays_seen > 0 {
            Section::Other
        } else if let Some(marked) = block.iter().find_map(|l| empty_slot(l)) {
            marked
        } else if let Some(known) = block.iter().find_map(|l| {
            module(l).and_then(|item| items::section(&item.name).filter(|s| s.is_slot()))
        }) {
            known
        } else {
            Section::MODULES
                .get(next_module)
                .copied()
                .unwrap_or(Section::Other)
        };
        if let Some(i) = Section::MODULES.iter().position(|&s| s == position) {
            next_module = i + 1;
        }
        for line in block {
            if let Some(section) = empty_slot(line) {
                slot_group(&mut groups, section).empty += 1;
            } else if let Some((name, count)) = quantity(line) {
                // A bay line among the modules: its blank line got lost.
                let section = match items::section(name) {
                    Some(Section::Drones) => Section::Drones,
                    _ => Section::Cargo,
                };
                put(&mut groups, section, bay_item(name, count));
            } else if let Some(item) = module(line) {
                let section = items::section(&item.name).unwrap_or(position);
                put(&mut groups, section, item);
            }
        }
    }
    // Slots first, then the bays, then implants and boosters.
    groups.sort_by_key(|g| g.section);
    groups
}

/// The group of `section`, added if there is none yet.
fn slot_group(groups: &mut Vec<Group>, section: Section) -> &mut Group {
    let i = match groups.iter().position(|g| g.section == section) {
        Some(i) => i,
        None => {
            groups.push(Group {
                section,
                items: Vec::new(),
                empty: 0,
            });
            groups.len() - 1
        }
    };
    &mut groups[i]
}

/// Adds an item to its section, counting it together with identical ones.
fn put(groups: &mut Vec<Group>, section: Section, item: Item) {
    let items = &mut slot_group(groups, section).items;
    match items
        .iter_mut()
        .find(|i| i.name == item.name && i.charge == item.charge && i.offline == item.offline)
    {
        Some(existing) => existing.count = existing.count.saturating_add(item.count),
        None => items.push(item),
    }
}

/// A module line: `Name`, `Name, Charge`, either with ` /OFFLINE`.
fn module(line: &str) -> Option<Item> {
    let (line, offline) = match line.strip_suffix("/OFFLINE") {
        Some(rest) => (rest.trim_end(), true),
        None => (line, false),
    };
    let line = strip_mutation(line);
    let (name, charge) = match line.split_once(',') {
        Some((name, charge)) => (name.trim(), Some(charge.trim())),
        None => (line, None),
    };
    if name.is_empty() {
        return None;
    }
    Some(Item {
        name: cut(name),
        charge: charge.filter(|c| !c.is_empty()).map(cut),
        count: 1,
        offline,
    })
}

fn bay_item(name: &str, count: u32) -> Item {
    Item {
        name: cut(name),
        charge: None,
        count,
        offline: false,
    }
}

/// `Warrior II x5` → ("Warrior II", 5).
fn quantity(line: &str) -> Option<(&str, u32)> {
    let (name, count) = line.rsplit_once(" x")?;
    if count.is_empty() || !count.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let name = strip_mutation(name.trim());
    if name.is_empty() {
        return None;
    }
    Some((name, count.parse().unwrap_or(u32::MAX)))
}

/// `[Empty Low slot]`, `[Empty Med slot]`, … → the section it marks.
fn empty_slot(line: &str) -> Option<Section> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?.trim();
    let kind = inner
        .strip_prefix("Empty ")?
        .strip_suffix(" slot")?
        .to_ascii_lowercase();
    match kind.as_str() {
        "low" => Some(Section::Low),
        "med" | "mid" | "medium" => Some(Section::Mid),
        "high" => Some(Section::High),
        "rig" => Some(Section::Rig),
        "subsystem" => Some(Section::Subsystem),
        _ => None,
    }
}

/// Mutated modules carry a reference such as ` [1]` to their details block.
fn strip_mutation(line: &str) -> &str {
    if let Some(start) = line.rfind(" [")
        && let Some(inner) = line[start + 2..].strip_suffix(']')
        && !inner.is_empty()
        && inner.bytes().all(|b| b.is_ascii_digit())
    {
        return line[..start].trim_end();
    }
    line
}

fn cut(text: &str) -> String {
    text.chars().take(MAX_NAME_CHARS).collect()
}
