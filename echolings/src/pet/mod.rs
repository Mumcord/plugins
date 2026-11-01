//! The pet: species, growth, meters and the care loop.
//!
//! Pure logic without host calls, so all of it runs in host-side unit tests. The plugin feeds it
//! the current time and what happened; rendering and storage live elsewhere.

pub mod balance;
mod streak;
#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use balance::*;
pub use streak::Streak;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Species {
    Fizzle,
    Burrble,
    Mossmo,
    Zapling,
    Hushmoth,
}

impl Species {
    pub const ALL: [Species; 5] = [
        Species::Fizzle,
        Species::Burrble,
        Species::Mossmo,
        Species::Zapling,
        Species::Hushmoth,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Species::Fizzle => "fizzle",
            Species::Burrble => "burrble",
            Species::Mossmo => "mossmo",
            Species::Zapling => "zapling",
            Species::Hushmoth => "hushmoth",
        }
    }

    pub fn from_id(id: &str) -> Option<Species> {
        Species::ALL.into_iter().find(|s| s.id() == id)
    }

    /// Colour variant ids; the last one has to be unlocked.
    pub fn colours(self) -> [&'static str; 3] {
        match self {
            Species::Fizzle => ["ember", "rose", "frost"],
            Species::Burrble => ["lagoon", "kelp", "deep"],
            Species::Mossmo => ["meadow", "autumn", "blossom"],
            Species::Zapling => ["thunder", "sunset", "midnight"],
            Species::Hushmoth => ["dusk", "dawn", "aurora"],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Hum,
    Sprout,
    Buddy,
    Echo,
}

impl Stage {
    pub fn for_bond(bond: u32) -> Stage {
        match bond {
            b if b >= ECHO_BOND => Stage::Echo,
            b if b >= BUDDY_BOND => Stage::Buddy,
            b if b >= SPROUT_BOND => Stage::Sprout,
            _ => Stage::Hum,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Stage::Hum => "hum",
            Stage::Sprout => "sprout",
            Stage::Buddy => "buddy",
            Stage::Echo => "echo",
        }
    }

    /// Bond needed for the next stage, none at the last one.
    pub fn next_bond(self) -> Option<u32> {
        match self {
            Stage::Hum => Some(SPROUT_BOND),
            Stage::Sprout => Some(BUDDY_BOND),
            Stage::Buddy => Some(ECHO_BOND),
            Stage::Echo => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    Sleeping,
    Sleepy,
    MissesYou,
    Content,
    Cheerful,
}

impl Mood {
    pub fn id(self) -> &'static str {
        match self {
            Mood::Sleeping => "sleeping",
            Mood::Sleepy => "sleepy",
            Mood::MissesYou => "missesYou",
            Mood::Content => "content",
            Mood::Cheerful => "cheerful",
        }
    }
}

/// The current moment as the plugin's clock reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Now {
    pub secs: u64,
    pub utc_offset_minutes: i32,
}

impl Now {
    /// The user's local calendar day, as days since 1970-01-01.
    pub fn day(self) -> i64 {
        let local =
            i64::try_from(self.secs).unwrap_or(i64::MAX) + i64::from(self.utc_offset_minutes) * 60;
        local.div_euclid(86_400)
    }
}

/// What was earned on one local day, for the daily caps.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Today {
    pub day: i64,
    /// All Notes earned today (shown in the panel).
    pub notes: u32,
    pub voice_notes: u32,
    pub chat_notes: u32,
    pub help_notes: u32,
    pub greet_notes: u32,
    pub bond_points: u32,
    pub pat_points: u32,
    #[serde(default)]
    pub visit_notes: u32,
    #[serde(default)]
    pub visited_notes: u32,
    #[serde(default)]
    pub game_notes: u32,
}

/// The stored pet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pet {
    pub name: String,
    pub species: Species,
    /// Index into [`Species::colours`].
    pub colour: u8,
    pub cheer: u8,
    pub energy: u8,
    /// Only ever goes up.
    pub bond: u32,
    /// Progress towards the next Bond level, below [`POINTS_PER_BOND`].
    pub bond_points: u32,
    pub notes: u32,
    pub today: Today,
    pub streak: Streak,
    /// Unix seconds when the current nap ends; 0 = awake.
    pub nap_until: u64,
    /// Unix seconds of the last update, for catching up after time away.
    pub last_seen: u64,
}

/// What is going on right now. Not stored: it is rebuilt from host events after every start.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Session {
    /// The user's own session while connected.
    pub me: Option<u32>,
    /// Other users in the user's channel.
    pub others: BTreeSet<u32>,
    pub deafened: bool,
    pub do_not_disturb: bool,
    pub talking: bool,
    pub last_talked: Option<u64>,
    pub mentioned_at: Option<u64>,
    pub joined_at: Option<u64>,
    pub last_tick: Option<u64>,
    voice_secs: u64,
    energy_carry: u64,
    decay_carry: u64,
}

impl Session {
    /// Voice time counts only with company, undeafened and after recent talking.
    fn counts_voice(&self, now: u64) -> bool {
        self.me.is_some()
            && !self.others.is_empty()
            && !self.deafened
            && !self.do_not_disturb
            && self
                .last_talked
                .is_some_and(|t| now.saturating_sub(t) <= VOICE_TALK_WINDOW_SECS)
    }

    fn alone(&self) -> bool {
        self.me.is_none() || self.others.is_empty()
    }

    /// Someone (not the user) joined the user's channel.
    pub fn joined(&mut self, other: u32, now: Now) {
        self.others.insert(other);
        self.joined_at = Some(now.secs);
    }

    pub fn left(&mut self, other: u32) {
        self.others.remove(&other);
    }

    pub fn mentioned(&mut self, now: Now) {
        self.mentioned_at = Some(now.secs);
    }
}

/// Visible feedback for a care action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reaction {
    Hop,
    Wave,
    Sparkle,
    Nap,
}

/// Why a care action was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NotEnoughNotes,
    TooTired,
    Asleep,
}

/// Trims a pet name, drops control characters and limits it to [`MAX_NAME_CHARS`]. None if
/// nothing is left.
pub fn clean_name(raw: &str) -> Option<String> {
    let words: Vec<&str> = raw.split_whitespace().collect();
    let name: String = words
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_CHARS)
        .collect();
    let name = name.trim().to_owned();
    (!name.is_empty()).then_some(name)
}

/// Adds up to `amount` to a daily counter without passing `cap`; returns what was added.
fn capped(counter: &mut u32, cap: u32, amount: u32) -> u32 {
    let granted = amount.min(cap.saturating_sub(*counter));
    *counter += granted;
    granted
}

fn raise(meter: &mut u8, by: u8) {
    *meter = meter.saturating_add(by).min(METER_MAX);
}

fn lower(meter: &mut u8, by: u8) {
    *meter = meter.saturating_sub(by);
}

/// Whole periods in `carry + secs`; the rest stays in `carry`.
fn periods(carry: &mut u64, secs: u64, period: u64) -> u64 {
    *carry += secs;
    let whole = *carry / period;
    *carry %= period;
    whole
}

fn meter_amount(units: u64) -> u8 {
    u8::try_from(units.min(u64::from(METER_MAX))).unwrap_or(METER_MAX)
}

impl Pet {
    /// A freshly hatched Hum.
    pub fn hatch(name: String, species: Species, colour: u8, now: Now) -> Pet {
        let mut pet = Pet {
            name,
            species,
            colour: colour.min(1),
            cheer: START_CHEER,
            energy: START_ENERGY,
            bond: 0,
            bond_points: 0,
            notes: 0,
            today: Today {
                day: now.day(),
                ..Today::default()
            },
            streak: Streak::default(),
            nap_until: 0,
            last_seen: now.secs,
        };
        pet.together(now);
        pet
    }

    pub fn stage(&self) -> Stage {
        Stage::for_bond(self.bond)
    }

    pub fn colour_id(&self) -> &'static str {
        let colours = self.species.colours();
        colours[usize::from(self.colour).min(colours.len() - 1)]
    }

    /// How many colour variants can be chosen right now.
    pub fn unlocked_colours(&self) -> u8 {
        if self.bond >= THIRD_COLOUR_BOND { 3 } else { 2 }
    }

    pub fn set_colour(&mut self, colour: u8) -> bool {
        if colour < self.unlocked_colours() {
            self.colour = colour;
            true
        } else {
            false
        }
    }

    /// Asset path of the pet's picture; `small` is the tight crop with thick outlines for badges.
    pub fn asset(&self, small: bool, asleep: bool) -> String {
        asset_path(
            self.species.id(),
            self.stage().id(),
            self.colour_id(),
            small,
            asleep,
        )
    }

    pub fn napping(&self, now: Now) -> bool {
        self.nap_until > now.secs
    }

    /// The pet sleeps during naps and whenever the user is in Do Not Disturb or quiet time.
    pub fn asleep(&self, session: &Session, now: Now) -> bool {
        self.napping(now) || session.do_not_disturb
    }

    pub fn mood(&self, session: &Session, now: Now) -> Mood {
        if self.asleep(session, now) {
            Mood::Sleeping
        } else if self.energy < SLEEPY_ENERGY {
            Mood::Sleepy
        } else if self.cheer < MISSES_YOU_CHEER {
            Mood::MissesYou
        } else if self.cheer >= CHEERFUL_CHEER {
            Mood::Cheerful
        } else {
            Mood::Content
        }
    }

    /// Notes earned on the current local day.
    pub fn notes_today(&self, now: Now) -> u32 {
        if self.today.day == now.day() {
            self.today.notes
        } else {
            0
        }
    }

    /// Catches up on time the plugin was not running: the pet napped and missed the user.
    pub fn catch_up(&mut self, now: Now) {
        let away = now.secs.saturating_sub(self.last_seen);
        if away > MAX_TICK_GAP_SECS {
            self.away(away);
        }
        self.last_seen = self.last_seen.max(now.secs);
        self.roll_day(now);
    }

    /// Advances the care loop; the plugin calls this about once a minute.
    pub fn tick(&mut self, session: &mut Session, now: Now) {
        let last = session.last_tick.replace(now.secs).unwrap_or(now.secs);
        let elapsed = now.secs.saturating_sub(last);
        self.roll_day(now);
        if session.talking {
            session.last_talked = Some(now.secs);
        }
        if elapsed > MAX_TICK_GAP_SECS {
            self.away(elapsed);
        } else if self.nap_until > last {
            // The nap covered (most of) this interval.
            let gain = periods(&mut session.energy_carry, elapsed, NAP_ENERGY_SECS);
            raise(&mut self.energy, meter_amount(gain));
        } else if session.do_not_disturb {
            let gain = periods(&mut session.energy_carry, elapsed, DND_ENERGY_SECS);
            raise(&mut self.energy, meter_amount(gain));
        } else if session.counts_voice(now.secs) {
            session.voice_secs += elapsed;
            while session.voice_secs >= VOICE_BLOCK_SECS {
                session.voice_secs -= VOICE_BLOCK_SECS;
                self.voice_block();
            }
        } else if session.alone() {
            let loss = periods(&mut session.decay_carry, elapsed, CHEER_DECAY_SECS);
            self.decay_cheer(loss);
        }
        if self.nap_until != 0 && !self.napping(now) {
            self.nap_until = 0;
        }
        if session.me.is_some() {
            self.together(now);
        }
        self.last_seen = self.last_seen.max(now.secs);
    }

    /// The user started or stopped talking. Returns true if that counted as greeting someone.
    pub fn on_own_talking(&mut self, session: &mut Session, talking: bool, now: Now) -> bool {
        session.talking = talking;
        if !talking {
            return false;
        }
        session.last_talked = Some(now.secs);
        let greeted = session
            .joined_at
            .take()
            .is_some_and(|t| now.secs.saturating_sub(t) <= GREET_WINDOW_SECS);
        if greeted {
            self.roll_day(now);
            let notes = capped(
                &mut self.today.greet_notes,
                GREETS_PER_DAY * GREET_NOTES,
                GREET_NOTES,
            );
            self.add_notes(notes);
            if notes > 0 {
                self.add_bond_points(GREET_POINTS);
            }
        }
        greeted
    }

    /// Someone wrote in a channel; `mine` if it was the user.
    pub fn on_chat(&mut self, session: &mut Session, mine: bool, now: Now) {
        self.roll_day(now);
        if !mine {
            raise(&mut self.cheer, OTHERS_CHAT_CHEER);
            return;
        }
        let notes = capped(&mut self.today.chat_notes, CHAT_NOTES_PER_DAY, CHAT_NOTES);
        self.add_notes(notes);
        if notes > 0 {
            self.add_bond_points(CHAT_POINTS);
        }
        let helped = session
            .mentioned_at
            .take()
            .is_some_and(|t| now.secs.saturating_sub(t) <= HELP_WINDOW_SECS);
        if helped {
            let notes = capped(
                &mut self.today.help_notes,
                HELPS_PER_DAY * HELP_NOTES,
                HELP_NOTES,
            );
            self.add_notes(notes);
            if notes > 0 {
                self.add_bond_points(HELP_POINTS);
            }
        }
        self.together(now);
    }

    pub fn pat(&mut self, session: &Session, now: Now) -> Option<Reaction> {
        self.roll_day(now);
        raise(&mut self.cheer, PAT_CHEER);
        let points = capped(&mut self.today.pat_points, PAT_POINTS_PER_DAY, PAT_POINTS);
        self.add_bond_points(points);
        self.together(now);
        (!self.asleep(session, now)).then_some(Reaction::Hop)
    }

    pub fn treat(&mut self, session: &Session, now: Now) -> Result<Option<Reaction>, Refusal> {
        if self.notes < TREAT_COST {
            return Err(Refusal::NotEnoughNotes);
        }
        self.roll_day(now);
        self.notes -= TREAT_COST;
        raise(&mut self.cheer, TREAT_CHEER);
        raise(&mut self.energy, TREAT_ENERGY);
        self.add_bond_points(TREAT_POINTS);
        self.together(now);
        Ok((!self.asleep(session, now)).then_some(Reaction::Sparkle))
    }

    pub fn play(&mut self, session: &Session, now: Now) -> Result<Reaction, Refusal> {
        if self.asleep(session, now) {
            return Err(Refusal::Asleep);
        }
        if self.energy < PLAY_ENERGY_COST {
            return Err(Refusal::TooTired);
        }
        self.roll_day(now);
        raise(&mut self.cheer, PLAY_CHEER);
        lower(&mut self.energy, PLAY_ENERGY_COST);
        self.add_bond_points(PLAY_POINTS);
        self.together(now);
        Ok(Reaction::Wave)
    }

    pub fn nap(&mut self, session: &Session, now: Now) -> Result<Reaction, Refusal> {
        if self.asleep(session, now) {
            return Err(Refusal::Asleep);
        }
        self.nap_until = now.secs + NAP_SECS;
        self.together(now);
        Ok(Reaction::Nap)
    }

    /// The user patted someone else's pet on a visit.
    pub fn visit(&mut self, now: Now) {
        self.roll_day(now);
        let notes = capped(
            &mut self.today.visit_notes,
            VISIT_NOTES_PER_DAY,
            VISIT_NOTES,
        );
        self.add_notes(notes);
        self.add_bond_points(VISIT_POINTS);
        self.together(now);
    }

    /// A visitor patted the user's pet.
    pub fn visited(&mut self, session: &Session, now: Now) -> Option<Reaction> {
        self.roll_day(now);
        let notes = capped(
            &mut self.today.visited_notes,
            VISITED_NOTES_PER_DAY,
            VISITED_NOTES,
        );
        self.add_notes(notes);
        raise(&mut self.cheer, VISITED_CHEER);
        self.add_bond_points(VISITED_POINTS);
        (!self.asleep(session, now)).then_some(Reaction::Hop)
    }

    /// The user finished a mini-game.
    pub fn game_finished(&mut self, now: Now) {
        self.roll_day(now);
        let notes = capped(&mut self.today.game_notes, GAME_NOTES_PER_DAY, GAME_NOTES);
        self.add_notes(notes);
        self.add_bond_points(GAME_POINTS);
        self.together(now);
    }

    fn voice_block(&mut self) {
        let notes = capped(
            &mut self.today.voice_notes,
            VOICE_NOTES_PER_DAY,
            VOICE_BLOCK_NOTES,
        );
        self.add_notes(notes);
        self.add_bond_points(VOICE_BLOCK_POINTS);
        raise(&mut self.cheer, VOICE_BLOCK_CHEER);
        lower(&mut self.energy, VOICE_BLOCK_ENERGY_COST);
    }

    fn away(&mut self, secs: u64) {
        let secs = secs.min(MAX_AWAY_SECS);
        self.decay_cheer(secs / CHEER_DECAY_SECS);
        raise(&mut self.energy, meter_amount(secs / AWAY_ENERGY_SECS));
        self.nap_until = 0;
    }

    fn decay_cheer(&mut self, loss: u64) {
        let floor = CHEER_FLOOR.min(self.cheer);
        self.cheer = self.cheer.saturating_sub(meter_amount(loss)).max(floor);
    }

    fn add_notes(&mut self, notes: u32) {
        self.notes = self.notes.saturating_add(notes);
        self.today.notes += notes;
    }

    fn add_bond_points(&mut self, points: u32) {
        let granted = capped(&mut self.today.bond_points, BOND_POINTS_PER_DAY, points);
        self.bond_points += granted;
        self.bond = self.bond.saturating_add(self.bond_points / POINTS_PER_BOND);
        self.bond_points %= POINTS_PER_BOND;
    }

    fn roll_day(&mut self, now: Now) {
        let day = now.day();
        if day > self.today.day {
            self.today = Today {
                day,
                ..Today::default()
            };
        }
    }

    /// Counts today as a day spent together.
    fn together(&mut self, now: Now) {
        self.roll_day(now);
        self.streak.mark(now.day());
    }
}

/// Pictures at or below this many pixels use the small crop.
pub const SMALL_MAX: u16 = 32;

/// Shared by the user's own pet and other people's cards so both use the same files.
pub fn asset_path(species: &str, stage: &str, colour: &str, small: bool, asleep: bool) -> String {
    let sleep = if asleep { "-sleep" } else { "" };
    let size = if small { "-small" } else { "" };
    format!("assets/pets/{species}-{stage}-{colour}{sleep}{size}.svg")
}
