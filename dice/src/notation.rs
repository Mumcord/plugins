//! Dice notation: `d20`, `2d6+3`, `3d8-1`, `d20 adv`, `d20 dis`, `4d6 dl`, `4d6 dh` (FR-6.82).
//!
//! The canonical form ([`Notation::canonical`]) is what goes into the commitment, so every
//! client must produce exactly the same text for the same roll.

pub const MAX_DICE: u8 = 20;
pub const MIN_SIDES: u16 = 2;
pub const MAX_SIDES: u16 = 1000;
pub const MAX_MODIFIER: i16 = 1000;
/// The longest canonical form ("20d1000+1000 adv" is 16 bytes); anything longer on the wire is
/// rejected before parsing.
pub const MAX_CANONICAL_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Plain,
    Advantage,
    Disadvantage,
    DropLowest,
    DropHighest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Notation {
    pub count: u8,
    pub sides: u16,
    pub modifier: i16,
    pub mode: Mode,
}

impl Notation {
    /// How many dice are actually thrown: advantage and disadvantage throw the die twice.
    pub fn dice_thrown(&self) -> usize {
        match self.mode {
            Mode::Advantage | Mode::Disadvantage => 2,
            _ => usize::from(self.count),
        }
    }

    /// `2d6+3`, `d20 adv`, `4d6 dl`.
    pub fn canonical(&self) -> String {
        let mut text = String::new();
        if self.count > 1 {
            text.push_str(&self.count.to_string());
        }
        text.push('d');
        text.push_str(&self.sides.to_string());
        if self.modifier > 0 {
            text.push_str(&format!("+{}", self.modifier));
        } else if self.modifier < 0 {
            text.push_str(&self.modifier.to_string());
        }
        let suffix = match self.mode {
            Mode::Plain => "",
            Mode::Advantage => " adv",
            Mode::Disadvantage => " dis",
            Mode::DropLowest => " dl",
            Mode::DropHighest => " dh",
        };
        text.push_str(suffix);
        text
    }

    /// The canonical form with the mode written out, for the card and the text line:
    /// `d20 with advantage`, `4d6 drop lowest`.
    pub fn described(&self) -> String {
        let mut plain = *self;
        plain.mode = Mode::Plain;
        let base = plain.canonical();
        match self.mode {
            Mode::Plain => base,
            Mode::Advantage => format!("{base} with advantage"),
            Mode::Disadvantage => format!("{base} with disadvantage"),
            Mode::DropLowest => format!("{base} drop lowest"),
            Mode::DropHighest => format!("{base} drop highest"),
        }
    }
}

/// Why a `/roll` was refused. The view turns it into text in the user's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotationError {
    Unknown,
    Modifier,
    DiceCount,
    Sides,
    AdvantageNeedsOneDie,
    DropNeedsTwoDice,
}

/// Parses what the user typed after `/roll`. Case and spaces do not matter; an empty roll is a
/// d20.
pub fn parse(input: &str) -> Result<Notation, NotationError> {
    let compact: String = input
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if compact.is_empty() {
        return Ok(Notation {
            count: 1,
            sides: 20,
            modifier: 0,
            mode: Mode::Plain,
        });
    }
    let unknown = || NotationError::Unknown;

    let (body, mode) = split_mode(&compact);
    let (count_text, rest) = body.split_once('d').ok_or_else(unknown)?;
    let sides_end = rest.find(['+', '-']).unwrap_or(rest.len());
    let (sides_text, modifier_text) = rest.split_at(sides_end);

    let count = if count_text.is_empty() {
        1
    } else {
        number(count_text).ok_or_else(unknown)?
    };
    let sides = number(sides_text).ok_or_else(unknown)?;
    let modifier = match modifier_text.split_at_checked(1) {
        None | Some(("", _)) => 0,
        Some((sign, digits)) => {
            let value = number(digits).ok_or_else(unknown)?;
            if value > u64::from(MAX_MODIFIER.unsigned_abs()) {
                return Err(NotationError::Modifier);
            }
            // In range: checked above.
            let value = i16::try_from(value).unwrap_or(MAX_MODIFIER);
            if sign == "-" { -value } else { value }
        }
    };

    if count == 0 || count > u64::from(MAX_DICE) {
        return Err(NotationError::DiceCount);
    }
    if sides < u64::from(MIN_SIDES) || sides > u64::from(MAX_SIDES) {
        return Err(NotationError::Sides);
    }
    // Both in range: checked above.
    let count = u8::try_from(count).unwrap_or(MAX_DICE);
    let sides = u16::try_from(sides).unwrap_or(MAX_SIDES);
    match mode {
        Mode::Advantage | Mode::Disadvantage if count != 1 => {
            return Err(NotationError::AdvantageNeedsOneDie);
        }
        Mode::DropLowest | Mode::DropHighest if count < 2 => {
            return Err(NotationError::DropNeedsTwoDice);
        }
        _ => {}
    }
    Ok(Notation {
        count,
        sides,
        modifier,
        mode,
    })
}

fn split_mode(compact: &str) -> (&str, Mode) {
    // Longest suffixes first, so "advantage" is not read as "...adv" + "antage".
    const SUFFIXES: [(&str, Mode); 6] = [
        ("disadvantage", Mode::Disadvantage),
        ("advantage", Mode::Advantage),
        ("adv", Mode::Advantage),
        ("dis", Mode::Disadvantage),
        ("dl", Mode::DropLowest),
        ("dh", Mode::DropHighest),
    ];
    for (suffix, mode) in SUFFIXES {
        if let Some(body) = compact.strip_suffix(suffix) {
            return (body, mode);
        }
    }
    (compact, Mode::Plain)
}

/// Plain ASCII digits only (no signs), at most 6 of them so nothing can overflow.
fn number(text: &str) -> Option<u64> {
    if text.is_empty() || text.len() > 6 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}
