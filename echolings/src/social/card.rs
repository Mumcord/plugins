//! The pet card: the compact summary other people's clients see (FR-6.83: at most 100 bytes).
//!
//! Wire format, version 1 (all integers big-endian):
//! `version u8 · species u8 · colour u8 · mood u8 · bond u16 · days together u16 · streak u16 ·
//! name length u8 · name (UTF-8, at most 16 characters)`. Later versions may append fields, so
//! a reader ignores bytes after the name. Everything received is untrusted: unknown species,
//! colours or moods drop the card, and the name goes through [`clean_name`].

use crate::pet::{Mood, Now, Pet, Session, Species, Stage, clean_name};

pub const MAX_BYTES: usize = 100;
const VERSION: u8 = 1;
const HEADER_BYTES: usize = 11;

const MOODS: [Mood; 5] = [
    Mood::Sleeping,
    Mood::Sleepy,
    Mood::MissesYou,
    Mood::Content,
    Mood::Cheerful,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub species: Species,
    /// Index into [`Species::colours`].
    pub colour: u8,
    pub mood: Mood,
    pub bond: u16,
    pub days_together: u16,
    pub streak: u16,
    pub name: String,
}

fn clamp(value: u32) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

impl Card {
    pub fn of(pet: &Pet, session: &Session, now: Now) -> Card {
        Card {
            species: pet.species,
            colour: pet.colour,
            mood: pet.mood(session, now),
            bond: clamp(pet.bond),
            days_together: clamp(pet.streak.days_together),
            streak: clamp(pet.streak.current(now.day())),
            name: pet.name.clone(),
        }
    }

    pub fn stage(&self) -> Stage {
        Stage::for_bond(u32::from(self.bond))
    }

    /// Asset path of the pet's picture, the same files the user's own pet uses.
    pub fn asset(&self) -> String {
        let colours = self.species.colours();
        format!(
            "assets/pets/{}-{}-{}.svg",
            self.species.id(),
            self.stage().id(),
            colours[usize::from(self.colour).min(colours.len() - 1)]
        )
    }

    pub fn encode(&self) -> Vec<u8> {
        let species = Species::ALL.iter().position(|s| *s == self.species);
        let mood = MOODS.iter().position(|m| *m == self.mood);
        let name = self.name.as_bytes();
        let mut out = Vec::with_capacity(HEADER_BYTES + name.len());
        out.push(VERSION);
        out.push(species.and_then(|i| u8::try_from(i).ok()).unwrap_or(0));
        out.push(self.colour);
        out.push(mood.and_then(|i| u8::try_from(i).ok()).unwrap_or(0));
        out.extend_from_slice(&self.bond.to_be_bytes());
        out.extend_from_slice(&self.days_together.to_be_bytes());
        out.extend_from_slice(&self.streak.to_be_bytes());
        out.push(u8::try_from(name.len()).unwrap_or(u8::MAX));
        out.extend_from_slice(name);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Card> {
        if bytes.len() > MAX_BYTES || bytes.len() < HEADER_BYTES || bytes[0] < VERSION {
            return None;
        }
        let u16_at = |i: usize| u16::from_be_bytes([bytes[i], bytes[i + 1]]);
        let species = *Species::ALL.get(usize::from(bytes[1]))?;
        let colour = bytes[2];
        if usize::from(colour) >= species.colours().len() {
            return None;
        }
        let mood = *MOODS.get(usize::from(bytes[3]))?;
        let name_len = usize::from(bytes[10]);
        let name = bytes.get(HEADER_BYTES..HEADER_BYTES + name_len)?;
        let name = clean_name(std::str::from_utf8(name).ok()?)?;
        Some(Card {
            species,
            colour,
            mood,
            bond: u16_at(4),
            days_together: u16_at(6),
            streak: u16_at(8),
            name,
        })
    }
}
