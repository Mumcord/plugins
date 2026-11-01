//! The pet and the plugin's own settings in plugin storage, scope "identity" (one pet per
//! identity). Values are versioned JSON so later plugin versions can migrate them.

use mumcord_plugin_sdk::{
    log,
    storage::{self, Scope},
};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;

use crate::pet::Pet;

const PET_KEY: &str = "pet";
const SETTINGS_KEY: &str = "settings";
const VERSION: u32 = 1;

/// How much the pet moves. The host's reduced-motion setting always wins.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnimationLevel {
    #[default]
    Full,
    Subtle,
    Still,
}

impl AnimationLevel {
    pub const ALL: [AnimationLevel; 3] = [
        AnimationLevel::Full,
        AnimationLevel::Subtle,
        AnimationLevel::Still,
    ];

    pub fn id(self) -> &'static str {
        match self {
            AnimationLevel::Full => "full",
            AnimationLevel::Subtle => "subtle",
            AnimationLevel::Still => "still",
        }
    }

    pub fn from_id(id: &str) -> Option<AnimationLevel> {
        AnimationLevel::ALL.into_iter().find(|a| a.id() == id)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub animation: AnimationLevel,
}

#[derive(Serialize, Deserialize)]
struct Stored<T> {
    v: u32,
    #[serde(flatten)]
    data: T,
}

#[derive(Serialize, Deserialize)]
struct PetData {
    pet: Pet,
}

/// Why stored data could not be used.
#[derive(Debug, PartialEq, Eq)]
pub enum LoadError {
    /// Written by a newer plugin version; it must not be overwritten.
    Newer(u32),
    Corrupt(String),
}

pub fn decode_pet(bytes: &[u8]) -> Result<Pet, LoadError> {
    decode::<PetData>(bytes).map(|d| d.pet)
}

pub fn encode_pet(pet: &Pet) -> Vec<u8> {
    encode(PetData { pet: pet.clone() })
}

pub fn decode_settings(bytes: &[u8]) -> Result<Settings, LoadError> {
    decode(bytes)
}

pub fn encode_settings(settings: &Settings) -> Vec<u8> {
    encode(settings.clone())
}

fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, LoadError> {
    #[derive(Deserialize)]
    struct Version {
        v: u32,
    }
    let version: Version =
        serde_json::from_slice(bytes).map_err(|e| LoadError::Corrupt(e.to_string()))?;
    if version.v > VERSION {
        return Err(LoadError::Newer(version.v));
    }
    serde_json::from_slice::<Stored<T>>(bytes)
        .map(|s| s.data)
        .map_err(|e| LoadError::Corrupt(e.to_string()))
}

fn encode<T: Serialize>(data: T) -> Vec<u8> {
    // Plain data structs always serialise.
    serde_json::to_vec(&Stored { v: VERSION, data }).unwrap_or_default()
}

/// Reads the pet. A pet written by a newer version stops the plugin instead of being lost;
/// unreadable data is logged and treated as no pet.
pub fn load_pet() -> Result<Option<Pet>, String> {
    load(PET_KEY, decode_pet)
}

pub fn load_settings() -> Settings {
    load(SETTINGS_KEY, decode_settings)
        .unwrap_or_else(|e| {
            log::warn(&e);
            None
        })
        .unwrap_or_default()
}

fn load<T>(key: &str, decode: fn(&[u8]) -> Result<T, LoadError>) -> Result<Option<T>, String> {
    let bytes = match storage::get(Scope::Identity, key) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Ok(None),
        Err(e) => return Err(format!("could not read {key}: {e:?}")),
    };
    match decode(&bytes) {
        Ok(value) => Ok(Some(value)),
        Err(LoadError::Newer(v)) => Err(format!(
            "{key} was saved by a newer Echolings (format {v}); update the plugin"
        )),
        Err(LoadError::Corrupt(e)) => {
            log::error(&format!("stored {key} is unreadable: {e}"));
            Ok(None)
        }
    }
}

pub fn save_pet(pet: &Pet) {
    save(PET_KEY, &encode_pet(pet));
}

pub fn save_settings(settings: &Settings) {
    save(SETTINGS_KEY, &encode_settings(settings));
}

thread_local! {
    /// What each key last stored. A write costs a database commit inside the plugin's call
    /// budget, and most ticks and clicks leave the pet unchanged.
    static SAVED: RefCell<HashMap<&'static str, Vec<u8>>> = RefCell::new(HashMap::new());
}

fn save(key: &'static str, bytes: &[u8]) {
    if already_saved(key, bytes) {
        return;
    }
    match storage::set(Scope::Identity, key, bytes) {
        Ok(()) => remember_saved(key, bytes),
        Err(e) => log::error(&format!("could not save {key}: {e:?}")),
    }
}

fn already_saved(key: &str, bytes: &[u8]) -> bool {
    SAVED.with(|saved| saved.borrow().get(key).is_some_and(|last| last == bytes))
}

fn remember_saved(key: &'static str, bytes: &[u8]) {
    SAVED.with(|saved| {
        saved.borrow_mut().insert(key, bytes.to_vec());
    });
}

/// The next saves write again, e.g. after a server change that may mean another identity.
pub fn forget_saved() {
    SAVED.with(|saved| saved.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_changed_values_are_written_again() {
        assert!(!already_saved(PET_KEY, b"a"));
        remember_saved(PET_KEY, b"a");
        assert!(already_saved(PET_KEY, b"a"));
        assert!(!already_saved(PET_KEY, b"b"));
        assert!(!already_saved(SETTINGS_KEY, b"a"));
        forget_saved();
        assert!(!already_saved(PET_KEY, b"a"));
    }
    use crate::pet::{Now, Species};

    fn pet() -> Pet {
        Pet::hatch(
            "Biscuit".into(),
            Species::Hushmoth,
            1,
            Now {
                secs: 1_793_577_600,
                utc_offset_minutes: 60,
            },
        )
    }

    #[test]
    fn pet_round_trips() {
        let pet = pet();
        let bytes = encode_pet(&pet);
        assert!(bytes.starts_with(b"{\"v\":1,"));
        assert_eq!(decode_pet(&bytes), Ok(pet));
    }

    #[test]
    fn newer_data_is_refused_not_dropped() {
        let text = String::from_utf8(encode_pet(&pet())).unwrap();
        let newer = text.replacen("\"v\":1", "\"v\":2", 1);
        assert_eq!(decode_pet(newer.as_bytes()), Err(LoadError::Newer(2)));
    }

    #[test]
    fn corrupt_data_is_reported() {
        assert!(matches!(
            decode_pet(b"{\"v\":1}"),
            Err(LoadError::Corrupt(_))
        ));
        assert!(matches!(decode_pet(b"\xff"), Err(LoadError::Corrupt(_))));
    }

    #[test]
    fn settings_round_trip_and_default_to_full() {
        let settings = Settings {
            animation: AnimationLevel::Still,
        };
        assert_eq!(decode_settings(&encode_settings(&settings)), Ok(settings));
        assert_eq!(Settings::default().animation, AnimationLevel::Full);
        assert_eq!(
            AnimationLevel::from_id("subtle"),
            Some(AnimationLevel::Subtle)
        );
    }
}
