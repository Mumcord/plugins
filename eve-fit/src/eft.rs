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
//! The plugin ships no EVE data, so everything is read from the text alone. Module sections come
//! in a fixed order (low, mid, high, rigs, subsystems) and are separated by blank lines; an
//! `[Empty Mid slot]` marker pins a section to its kind when a section is missing. Sections
//! whose lines all end in `xN` are drones and cargo, which only their order tells apart.
//!
//! The text comes from other people in the chat: it is untrusted, so the parser is bounded in
//! size and never panics.

/// The longest text the parser looks at; the host forwards at most 4 KiB as well.
pub const MAX_TEXT_BYTES: usize = 4096;
/// More lines than any real fit has (a full ship with drones and cargo is about 60).
pub const MAX_LINES: usize = 200;
/// Item and ship names in EVE are short; longer ones are cut so the UI stays tidy.
pub const MAX_NAME_CHARS: usize = 100;

/// Where a group of items sits on the ship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Low,
    Mid,
    High,
    Rig,
    Subsystem,
    Drones,
    /// A single `xN` block: without EVE data there is no telling drones from cargo.
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

/// Turns the blocks into sections: module blocks in EFT order (or by their empty-slot marker),
/// then the `xN` blocks as drones and cargo.
fn classify(blocks: Vec<Vec<&str>>) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    // Index into Section::MODULES of the next module block.
    let mut next_module = 0;
    let mut bays: Vec<Group> = Vec::new();
    for block in blocks {
        // The details of mutated modules ("[1] Abyssal Gyrostabilizer" and its attribute lines)
        // repeat what the module lines already say.
        if block[0].starts_with('[') && block[0][1..].starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        if block.iter().all(|l| quantity(l).is_some()) {
            bays.push(bay(&block, Section::Cargo));
            continue;
        }
        let section = if !bays.is_empty() {
            Section::Other
        } else if let Some(marked) = block.iter().find_map(|l| empty_slot(l)) {
            marked
        } else if let Some(&s) = Section::MODULES.get(next_module) {
            s
        } else {
            Section::Other
        };
        if let Some(i) = Section::MODULES.iter().position(|&s| s == section) {
            next_module = i + 1;
        }
        let group = modules(&block, section);
        merge(&mut groups, group);
    }
    match bays.len() {
        0 => {}
        1 => bays[0].section = Section::DronesOrCargo,
        _ => bays[0].section = Section::Drones,
    }
    // Implants and boosters come between drones and cargo; keep the bays together on top.
    let other: Vec<Group> = groups
        .iter()
        .filter(|g| g.section == Section::Other)
        .cloned()
        .collect();
    groups.retain(|g| g.section != Section::Other);
    for g in bays {
        merge(&mut groups, g);
    }
    for g in other {
        merge(&mut groups, g);
    }
    groups
}

/// Adds a group, joining it with an earlier group of the same section.
fn merge(groups: &mut Vec<Group>, group: Group) {
    match groups.iter_mut().find(|g| g.section == group.section) {
        Some(existing) => {
            existing.empty += group.empty;
            for item in group.items {
                add(&mut existing.items, item);
            }
        }
        None => groups.push(group),
    }
}

fn add(items: &mut Vec<Item>, item: Item) {
    match items
        .iter_mut()
        .find(|i| i.name == item.name && i.charge == item.charge && i.offline == item.offline)
    {
        Some(existing) => existing.count = existing.count.saturating_add(item.count),
        None => items.push(item),
    }
}

fn modules(block: &[&str], section: Section) -> Group {
    let mut group = Group {
        section,
        items: Vec::new(),
        empty: 0,
    };
    for line in block {
        if empty_slot(line).is_some() {
            group.empty += 1;
            continue;
        }
        let (line, offline) = match line.strip_suffix("/OFFLINE") {
            Some(rest) => (rest.trim_end(), true),
            None => (*line, false),
        };
        let line = strip_mutation(line);
        let (name, charge) = match line.split_once(',') {
            Some((name, charge)) => (name.trim(), Some(charge.trim())),
            None => (line, None),
        };
        if name.is_empty() {
            continue;
        }
        add(
            &mut group.items,
            Item {
                name: cut(name),
                charge: charge.filter(|c| !c.is_empty()).map(cut),
                count: 1,
                offline,
            },
        );
    }
    group
}

fn bay(block: &[&str], section: Section) -> Group {
    let mut group = Group {
        section,
        items: Vec::new(),
        empty: 0,
    };
    for line in block {
        if let Some((name, count)) = quantity(line) {
            add(
                &mut group.items,
                Item {
                    name: cut(name),
                    charge: None,
                    count,
                    offline: false,
                },
            );
        }
    }
    group
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
