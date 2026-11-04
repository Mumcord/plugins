//! The game: one night of Skyloom as this client sees it.
//!
//! Every client runs the same state machine on the same messages. Authority is split so no one
//! client decides everything: the Lamplighter (the first player in the roster) seats players and
//! sets the night's settings, the Weaver seals and reveals the figure, and every Threader alone
//! moves and locks their own stars. Results are computed by every client and compared.

use std::collections::{BTreeMap, BTreeSet};

use mumcord_plugin_sdk::ui::El;

use crate::deck;
use crate::fair;
use crate::rules::{self, Twist};
use crate::score::{self, Point};
use crate::view;
use crate::wire::{MAX_PLAYERS, Message, Nonce, Pos, Secret, Settings};

/// Ticks drive all timing; the host calls [`App::tick`] every `TICK_MS`.
pub const TICK_MS: u64 = 100;
/// The Weaver waits at most this long for witnesses before weaving starts.
const SEAL_WAIT_MS: u64 = 3_000;
/// The Weaver waits at most this long for every lock before revealing.
const LOCK_WAIT_MS: u64 = 5_000;
/// After the reveal, missing locks are waited for this long before scoring without them.
const SCORE_WAIT_MS: u64 = 3_000;
/// Result hashes are collected this long before the verdict is final.
const VERIFY_WAIT_MS: u64 = 6_000;
/// A round that does not move on for this long is void ("clouds roll in").
const STALL_MS: u64 = 20_000;
/// Own star updates: every 2 s over Mumble (FR-6.36), 4 per second in the game burst, about
/// 12 per second over the relay.
const SEND_SLOW_MS: u64 = 2_000;
const SEND_BURST_MS: u64 = 250;
const SEND_LIVE_MS: u64 = 80;
/// Star positions go to the page at most this often.
const PAGE_STARS_MS: u64 = 100;
/// Past skies kept in storage.
pub const PAST_SKIES: usize = 20;

/// Calls into Mumcord. Everything that leaves the plugin goes through here.
pub trait Host {
    fn random(&mut self, len: u32) -> Option<Vec<u8>>;
    /// Sends to everyone in the channel who runs Skyloom: over Mumble plugin data, or over the
    /// relay when `live` (star movement at a higher rate; only sent while [`Host::live`]).
    fn send(&mut self, message: &Message, live: bool);
    fn send_direct(&mut self, to: u32, message: &Message);
    /// The relay carries our data right now, so star movement can be live.
    fn live(&self) -> bool;
    fn start_game_burst(&mut self);
    /// Posts a message to the page, if one is open.
    fn post(&mut self, json: String);
    /// Starts the activity in the media area; false when the host refused.
    fn start_stage(&mut self, title: &str) -> bool;
    fn end_stage(&mut self);
    fn chat_send(&mut self, text: &str);
    fn line_hash(&mut self, text: &str) -> u32;
    fn supersede(&mut self, card: &str, sender: u32, hash: u32);
    fn show_card(&mut self, card: &str, tree: El);
    fn set_badge(&mut self, session: u32, on: bool);
    fn play(&mut self, sound: &str, gain: f32);
    fn stop_sounds(&mut self);
    fn notify(&mut self, title: &str, body: &str);
    fn save(&mut self, prefs: &Prefs);
    fn warn(&mut self, message: &str);
}

/// What the local user keeps between nights.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Prefs {
    /// Sound volume 0–100.
    pub volume: u8,
    pub muted: bool,
    /// Past night skies, newest last.
    pub skies: Vec<PastSky>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            volume: 40,
            muted: false,
            skies: Vec::new(),
        }
    }
}

/// A woven night: its name and its constellations. No names of other people.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PastSky {
    pub name: String,
    pub night: u32,
    /// Unix seconds when the night ended.
    pub at: u64,
    /// (figure id, resonance) of each woven round.
    pub figures: Vec<(String, u8)>,
    pub team: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The Weaver seals the figure and Threaders witness.
    Seal,
    Weave,
    /// Positions are locked; waiting for the reveal.
    Dawn,
    Reveal,
    /// Clouds rolled in: the Weaver left or the round stalled.
    Void,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verify {
    Pending,
    Verified,
    /// The seal did not match the revealed secret.
    SealBroken,
    /// Other clients computed a different result.
    Differs,
    /// This client lacks data (it joined late or missed messages).
    Missing,
}

/// One round's result as this client computed it.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub figure: Option<usize>,
    /// (session, star index, position, resonance) of each scored star.
    pub stars: Vec<(u32, u8, Pos, u8)>,
    /// The figure points matched to each star, in loom units, for the reveal.
    pub targets: Vec<(f64, f64)>,
    /// For each star, the index of its figure point (the order along the figure's lines).
    pub order: Vec<usize>,
    /// The true figure's lines fitted onto the stars.
    pub ghost: Vec<Vec<(f64, f64)>>,
    pub bonus: u8,
    /// Seconds the Weaver had left when tying off early.
    pub spare: Option<u8>,
    pub resonance: u8,
    pub hash: [u8; 8],
    pub verify: Verify,
    pub at: u64,
}

pub struct Round {
    pub no: u8,
    pub weaver: u32,
    pub twin: Option<u32>,
    pub twist: Twist,
    /// Players of the round in roster order (the slot order of the masks).
    pub slots: Vec<u32>,
    pub threaders: Vec<u32>,
    pub stars_each: usize,
    pub phase: Phase,
    pub phase_at: u64,
    pub deadline: u64,
    pub commit: Option<[u8; 32]>,
    pub used: u64,
    pub secret: Option<Secret>,
    pub witnesses: BTreeMap<u32, Nonce>,
    pub witness_mask: Option<u16>,
    pub positions: BTreeMap<u32, Vec<Pos>>,
    pub locks: BTreeMap<u32, Vec<Pos>>,
    pub lock_mask: Option<u16>,
    pub bonus: u8,
    pub spare: Option<u8>,
    pub hashes: BTreeMap<u32, [u8; 8]>,
    pub outcome: Option<Outcome>,
    pub burst: bool,
    pub own_dirty: bool,
    pub last_sent: u64,
    pub last_live: u64,
    pub drift_at: u64,
    pub left: BTreeSet<u32>,
    pub void_reason: Option<&'static str>,
}

impl Round {
    pub fn is_threader(&self, session: u32) -> bool {
        self.threaders.contains(&session)
    }

    pub fn is_weaver(&self, session: u32) -> bool {
        self.weaver == session || self.twin == Some(session)
    }

    pub fn finished(&self) -> bool {
        matches!(self.phase, Phase::Void) || self.outcome.is_some()
    }

    /// All stars of the round in order, with their owners.
    pub fn all_stars(&self) -> Vec<(u32, u8, Pos)> {
        let starts = rules::start_positions(self.threaders.len() * self.stars_each);
        let mut out = Vec::new();
        for (t, &owner) in self.threaders.iter().enumerate() {
            if self.left.contains(&owner) {
                continue;
            }
            for i in 0..self.stars_each {
                let pos = self
                    .positions
                    .get(&owner)
                    .and_then(|p| p.get(i).copied())
                    .unwrap_or(starts[t * self.stars_each + i]);
                out.push((owner, i as u8, pos));
            }
        }
        out
    }

    /// The figure this client may see now: the Weavers after the seal, everyone after a valid
    /// reveal.
    pub fn known_figure(&self) -> Option<usize> {
        let secret = self.secret.as_ref()?;
        let mask = self.witness_mask?;
        let witnesses = self.witness_list(mask)?;
        Some(fair::figure(secret, self.no, &witnesses, self.used))
    }

    fn witness_list(&self, mask: u16) -> Option<Vec<(u8, Nonce)>> {
        let mut list = Vec::new();
        for (slot, session) in self.slots.iter().enumerate() {
            if mask & (1 << slot) != 0 {
                list.push((slot as u8, *self.witnesses.get(session)?));
            }
        }
        Some(list)
    }
}

/// A finished round, kept for the night's tapestry.
#[derive(Debug, Clone)]
pub struct Record {
    pub no: u8,
    pub weaver: u32,
    pub figure: Option<usize>,
    pub resonance: Option<u8>,
    pub verified: bool,
    /// The player with the closest star.
    pub truest: Option<u32>,
}

pub struct Night {
    pub id: u32,
    pub rev: u8,
    pub settings: Settings,
    /// Seated players in join order; the first is the Lamplighter.
    pub players: Vec<u32>,
    revs: Vec<(u8, Vec<u32>)>,
    pub weaved: Vec<u32>,
    pub used: u64,
    pub history: Vec<Record>,
    pub round: Option<Round>,
    pub round_no: u8,
    pub ended: bool,
    pub spectators: BTreeSet<u32>,
    /// Lamplighter only: players to seat after the round.
    pending: Vec<u32>,
    roster_due: bool,
    /// Distance each player's stars travelled this night.
    pub travel: BTreeMap<u32, u64>,
    pub posted_rounds: BTreeSet<u8>,
    pub summary_posted: bool,
    pub ended_at: u64,
    pub sky_name: String,
}

impl Night {
    fn new(id: u32, rev: u8, settings: Settings, players: Vec<u32>) -> Night {
        Night {
            id,
            rev,
            settings,
            revs: vec![(rev, players.clone())],
            players,
            weaved: Vec::new(),
            used: 0,
            history: Vec::new(),
            round: None,
            round_no: 0,
            ended: false,
            spectators: BTreeSet::new(),
            pending: Vec::new(),
            roster_due: false,
            travel: BTreeMap::new(),
            posted_rounds: BTreeSet::new(),
            summary_posted: false,
            ended_at: 0,
            sky_name: String::new(),
        }
    }

    pub fn lamplighter(&self) -> Option<u32> {
        self.players.first().copied()
    }

    /// The player who weaves next, or None when the night is over.
    pub fn next_weaver(&self) -> Option<u32> {
        if self.ended || self.history.len() >= rules::MAX_ROUNDS {
            return None;
        }
        self.players
            .iter()
            .copied()
            .find(|p| !self.weaved.contains(p))
    }

    /// Rounds this night will have, as far as known now.
    pub fn total_rounds(&self) -> usize {
        let left = self
            .players
            .iter()
            .filter(|p| !self.weaved.contains(p))
            .count();
        (self.history.len() + left).min(rules::MAX_ROUNDS)
    }

    fn between_rounds(&self) -> bool {
        self.round.as_ref().is_none_or(Round::finished)
    }

    fn weaved_mask(&self) -> u16 {
        self.players
            .iter()
            .enumerate()
            .filter(|(_, p)| self.weaved.contains(p))
            .fold(0, |m, (i, _)| m | (1 << i))
    }

    /// Mean resonance of the verified rounds that were woven or not (void rounds excluded).
    pub fn team(&self) -> Option<u8> {
        let scored: Vec<u32> = self
            .history
            .iter()
            .filter_map(|r| r.resonance.map(u32::from))
            .collect();
        (!scored.is_empty()).then(|| (scored.iter().sum::<u32>() / scored.len() as u32) as u8)
    }
}

/// Where the page is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Stage,
    SidePanel,
    Dialog,
}

#[derive(Default)]
pub struct App {
    pub me: u32,
    pub names: BTreeMap<u32, String>,
    pub talking: BTreeSet<u32>,
    pub now: u64,
    pub unix: u64,
    pub night: Option<Night>,
    /// The channel's Skyloom activity: (id, starter).
    pub activity: Option<(String, u32)>,
    /// The local user is in the activity.
    pub joined: bool,
    pub place: Option<Place>,
    pub page_ready: bool,
    pub prefs: Prefs,
    pub guess: String,
    pub dark: bool,
    pub reduce_motion: bool,
    /// The relay carries star movement live.
    pub live: bool,
    page_stars_dirty: bool,
    last_page_stars: u64,
}

impl App {
    pub fn name(&self, session: u32) -> String {
        self.names
            .get(&session)
            .cloned()
            .unwrap_or_else(|| format!("#{session}"))
    }

    fn round(&self) -> Option<&Round> {
        self.night.as_ref()?.round.as_ref()
    }

    fn random_array<const N: usize>(host: &mut dyn Host) -> Option<[u8; N]> {
        host.random(N as u32)?.try_into().ok()
    }

    // ----- Host events -----

    pub fn on_presence(&mut self, peers: Vec<(u32, String)>) {
        for (session, name) in peers {
            self.names.insert(session, name);
        }
    }

    pub fn on_joined(&mut self, session: u32, name: String) {
        self.names.insert(session, name);
    }

    /// Someone left the channel (or the activity).
    pub fn on_left(&mut self, host: &mut dyn Host, session: u32) {
        self.talking.remove(&session);
        self.drop_player(host, session);
    }

    pub fn on_talking(&mut self, host: &mut dyn Host, session: u32, on: bool) {
        let changed = if on {
            self.talking.insert(session)
        } else {
            self.talking.remove(&session)
        };
        if changed && self.page_ready {
            host.post(view::talk(session, on));
        }
    }

    /// The user chose "Start Skyloom" (Apps menu or `/skyloom`): a fresh gesture.
    pub fn on_start(&mut self, host: &mut dyn Host) {
        if self.joined && self.night.is_some() {
            return;
        }
        if self
            .activity
            .as_ref()
            .is_some_and(|(_, starter)| *starter != self.me)
        {
            // Someone else's night runs here: the host's Join button is the way in.
            host.warn("a Skyloom night already runs in this channel");
            return;
        }
        if !host.start_stage("Skyloom") {
            return;
        }
        let Some(id) = Self::random_array::<4>(host).map(u32::from_be_bytes) else {
            return;
        };
        self.joined = true;
        self.place = Some(Place::Stage);
        self.night = Some(Night::new(id, 0, Settings::default(), vec![self.me]));
        self.send_roster(host);
        // The click is the consent for the one plain line standard clients see.
        host.chat_send(&view::start_line(&self.name(self.me)));
        host.set_badge(self.me, true);
        self.refresh(host);
    }

    pub fn on_activity_started(&mut self, host: &mut dyn Host, starter: u32, id: String) {
        self.activity = Some((id.clone(), starter));
        if starter == self.me {
            return;
        }
        let name = self.name(starter);
        let card = view::invite_card_id(&id);
        host.show_card(&card, view::invite_card(&name, false));
        let hash = host.line_hash(&view::start_line(&name));
        host.supersede(&card, starter, hash);
        host.notify(
            "Skyloom",
            &crate::text::t_with("notify.opened", &[("name", &name)]),
        );
    }

    pub fn on_activity_ended(&mut self, host: &mut dyn Host, id: &str) {
        if self.activity.as_ref().is_some_and(|(a, _)| a == id) {
            self.activity = None;
            self.clear_badges(host);
            self.night = None;
            self.joined = false;
            self.place = None;
            self.page_ready = false;
        }
    }

    /// The local user joined the activity (the host's Join button or the invite card).
    pub fn on_stage_joined(&mut self, host: &mut dyn Host) {
        if self.joined {
            return;
        }
        self.joined = true;
        self.place = Some(Place::Stage);
        let play = self
            .night
            .as_ref()
            .is_none_or(|n| n.players.len() < MAX_PLAYERS && !n.ended);
        host.send(&Message::Join { play }, false);
        if !play && let Some(night) = &mut self.night {
            night.spectators.insert(self.me);
        }
        self.refresh(host);
    }

    /// The local user left the activity; it keeps running for the others.
    pub fn on_stage_left(&mut self, host: &mut dyn Host) {
        if !self.joined {
            return;
        }
        self.joined = false;
        self.place = None;
        self.page_ready = false;
        host.send(&Message::Leave, false);
        let me = self.me;
        self.drop_player(host, me);
    }

    // ----- Messages from other clients -----

    pub fn on_message(&mut self, host: &mut dyn Host, sender: u32, message: Message) {
        if sender == self.me {
            return;
        }
        match message {
            Message::Roster {
                night,
                rev,
                round,
                settings,
                weaved,
                players,
            } => self.on_roster(host, sender, night, rev, round, settings, weaved, players),
            Message::Join { play } => self.on_join(host, sender, play),
            Message::Leave => self.drop_player(host, sender),
            Message::Ready { round, rev } => self.on_ready(host, sender, round, rev),
            Message::Seal {
                round,
                commit,
                used,
            } => self.on_seal(host, sender, round, commit, used),
            Message::Witness { round, nonce } => {
                if let Some(r) = self.round_mut(round)
                    && r.is_threader(sender)
                    && matches!(r.phase, Phase::Seal | Phase::Weave)
                {
                    r.witnesses.insert(sender, nonce);
                }
            }
            Message::Weave { round, witnesses } => self.on_weave(host, sender, round, witnesses),
            Message::Stars { round, stars } => self.on_stars(sender, round, stars, false),
            Message::Lock { round, stars } => {
                self.on_stars(sender, round, stars, true);
                self.refresh(host);
            }
            Message::Pluck {
                round,
                target,
                star,
            } => {
                let ok = self
                    .round_mut(round)
                    .is_some_and(|r| r.is_weaver(sender) && r.phase == Phase::Weave);
                if ok {
                    self.on_pluck(host, sender, target, star);
                }
            }
            Message::TieOff { round, spare } => {
                let ok = self
                    .round_mut(round)
                    .is_some_and(|r| r.weaver == sender && r.phase == Phase::Weave);
                if ok {
                    if let Some(r) = self.round_mut(round) {
                        r.bonus = rules::tie_off_bonus(spare);
                        r.spare = Some(spare);
                    }
                    self.enter_dawn(host);
                }
            }
            Message::Reveal {
                round,
                secret,
                locks,
            } => {
                let now = self.now;
                if let Some(r) = self.round_mut(round)
                    && r.weaver == sender
                    && matches!(r.phase, Phase::Weave | Phase::Dawn)
                {
                    r.secret = Some(secret);
                    r.lock_mask = Some(locks);
                    r.phase = Phase::Reveal;
                    r.phase_at = now;
                    self.try_score(host);
                }
            }
            Message::Result { round, hash } => {
                if let Some(r) = self.round_mut(round)
                    && r.slots.contains(&sender)
                {
                    r.hashes.insert(sender, hash);
                }
            }
            Message::Share { round, secret } => {
                let me = self.me;
                if let Some(r) = self.round_mut(round)
                    && r.weaver == sender
                    && r.twin == Some(me)
                {
                    r.secret = Some(secret);
                    self.refresh(host);
                }
            }
            Message::End { night } => {
                let ok = self
                    .night
                    .as_ref()
                    .is_some_and(|n| n.id == night && n.lamplighter() == Some(sender));
                if ok {
                    self.end_night(host, sender);
                }
            }
        }
    }

    fn round_mut(&mut self, no: u8) -> Option<&mut Round> {
        self.night.as_mut()?.round.as_mut().filter(|r| r.no == no)
    }

    #[allow(clippy::too_many_arguments)]
    fn on_roster(
        &mut self,
        host: &mut dyn Host,
        sender: u32,
        id: u32,
        rev: u8,
        round: u8,
        settings: Settings,
        weaved: u16,
        players: Vec<u32>,
    ) {
        if players.first() != Some(&sender) {
            return;
        }
        let weaved: Vec<u32> = players
            .iter()
            .enumerate()
            .filter(|(i, _)| weaved & (1 << i) != 0)
            .map(|(_, &p)| p)
            .collect();
        match &mut self.night {
            Some(night) if night.id == id => {
                // Revisions wrap; a later one differs from the current.
                if night.rev == rev && night.players == players {
                    return;
                }
                let before = night.players.clone();
                night.rev = rev;
                night.settings = settings;
                night.players = players.clone();
                night.revs.push((rev, players.clone()));
                if night.revs.len() > 8 {
                    night.revs.remove(0);
                }
                for p in weaved {
                    if !night.weaved.contains(&p) {
                        night.weaved.push(p);
                    }
                }
                night.round_no = night.round_no.max(round);
                for p in &players {
                    night.spectators.remove(p);
                }
                for p in before.iter().filter(|p| !players.contains(p)) {
                    host.set_badge(*p, false);
                }
            }
            _ => {
                let mut fresh = Night::new(id, rev, settings, players.clone());
                fresh.weaved = weaved;
                fresh.round_no = round;
                // Another night of the same group keeps its spectators.
                if let Some(old) = self.night.take() {
                    fresh.spectators = old
                        .spectators
                        .into_iter()
                        .filter(|s| !players.contains(s))
                        .collect();
                }
                self.night = Some(fresh);
            }
        }
        for p in &players {
            host.set_badge(*p, true);
        }
        self.refresh(host);
    }

    fn on_join(&mut self, host: &mut dyn Host, sender: u32, play: bool) {
        let me = self.me;
        let Some(night) = &mut self.night else {
            return;
        };
        if night.players.contains(&sender) {
            return;
        }
        let lamplighter = night.lamplighter() == Some(me);
        let seat = play && !night.ended && night.players.len() + night.pending.len() < MAX_PLAYERS;
        if !seat {
            night.spectators.insert(sender);
        } else if lamplighter {
            if night.between_rounds() {
                night.players.push(sender);
                night.roster_due = true;
            } else {
                // Drop-in mid-round: watch now, a star next round.
                night.pending.push(sender);
                night.spectators.insert(sender);
                // Tells the newcomer about the night so they can watch until their seat is ready.
                night.roster_due = true;
            }
        } else {
            night.spectators.insert(sender);
        }
        self.refresh(host);
    }

    /// A player left the night or the channel.
    fn drop_player(&mut self, host: &mut dyn Host, session: u32) {
        let me = self.me;
        let now = self.now;
        let Some(night) = &mut self.night else {
            return;
        };
        night.spectators.remove(&session);
        night.pending.retain(|&p| p != session);
        if !night.players.contains(&session) {
            return;
        }
        night.players.retain(|&p| p != session);
        host.set_badge(session, false);
        if let Some(r) = &mut night.round
            && !r.finished()
            && r.slots.contains(&session)
        {
            r.left.insert(session);
            if r.is_weaver(session) && r.phase != Phase::Reveal {
                r.phase = Phase::Void;
                r.phase_at = now;
                r.void_reason = Some("weaver-left");
            }
        }
        // The longest-present player is the next in join order, so the hand-over needs no message.
        if night.lamplighter() == Some(me) {
            night.roster_due = true;
        }
        if night.players.is_empty() {
            self.night = None;
        } else if self
            .night
            .as_ref()
            .and_then(|n| n.round.as_ref())
            .is_some_and(|r| r.phase == Phase::Void)
        {
            self.finish_round(host);
        }
        self.refresh(host);
    }

    fn on_ready(&mut self, host: &mut dyn Host, sender: u32, round: u8, rev: u8) {
        let Some(night) = &self.night else {
            return;
        };
        if !night.players.contains(&sender) || round <= night.round_no || !night.between_rounds() {
            return;
        }
        // The Weaver's click also posted the previous round's line: show its card instead.
        if let Some(prev) = night.history.last() {
            let line = view::round_line(self, prev);
            let card = view::round_card_id(night.id, prev.no);
            let hash = host.line_hash(&line);
            host.supersede(&card, sender, hash);
        }
        self.start_round(host, round, sender, rev);
    }

    fn start_round(&mut self, host: &mut dyn Host, no: u8, weaver: u32, rev: u8) {
        let me = self.me;
        let now = self.now;
        let Some(night) = &mut self.night else {
            return;
        };
        let slots = night
            .revs
            .iter()
            .rev()
            .find(|(r, _)| *r == rev)
            .map(|(_, p)| p.clone())
            .unwrap_or_else(|| night.players.clone());
        let twist = rules::twist(night.id, no, night.settings.mixed, slots.len());
        let twin = (twist == Twist::Twin)
            .then(|| rules::twin_of(&slots, weaver))
            .flatten();
        let threaders: Vec<u32> = slots
            .iter()
            .copied()
            .filter(|&p| p != weaver && Some(p) != twin)
            .collect();
        night.round_no = no;
        let mut round = Round {
            no,
            weaver,
            twin,
            twist,
            stars_each: rules::stars_each(threaders.len()),
            slots,
            threaders,
            phase: Phase::Seal,
            phase_at: now,
            deadline: 0,
            commit: None,
            used: night.used,
            secret: None,
            witnesses: BTreeMap::new(),
            witness_mask: None,
            positions: BTreeMap::new(),
            locks: BTreeMap::new(),
            lock_mask: None,
            bonus: 0,
            spare: None,
            hashes: BTreeMap::new(),
            outcome: None,
            burst: false,
            own_dirty: false,
            last_sent: 0,
            last_live: 0,
            drift_at: 0,
            left: BTreeSet::new(),
            void_reason: None,
        };
        if weaver == me {
            match Self::random_array::<32>(host) {
                Some(secret) => {
                    let commit = fair::commit(&secret, no);
                    round.secret = Some(secret);
                    round.commit = Some(commit);
                    host.send(
                        &Message::Seal {
                            round: no,
                            commit,
                            used: night.used,
                        },
                        false,
                    );
                }
                None => {
                    round.phase = Phase::Void;
                    round.void_reason = Some("no-random");
                }
            }
        }
        night.round = Some(round);
        if self.round().is_some_and(|r| r.phase == Phase::Void) {
            self.finish_round(host);
        }
        self.refresh(host);
    }

    fn on_seal(&mut self, host: &mut dyn Host, sender: u32, no: u8, commit: [u8; 32], used: u64) {
        let me = self.me;
        let Some(r) = self.round_mut(no) else {
            return;
        };
        if r.weaver != sender || r.phase != Phase::Seal || r.commit.is_some() {
            return;
        }
        r.commit = Some(commit);
        r.used = used;
        if r.is_threader(me)
            && let Some(nonce) = Self::random_array::<16>(host)
        {
            if let Some(r) = self.round_mut(no) {
                r.witnesses.insert(me, nonce);
            }
            host.send(&Message::Witness { round: no, nonce }, false);
        }
    }

    fn on_weave(&mut self, host: &mut dyn Host, sender: u32, no: u8, witnesses: u16) {
        let Some(r) = self.round_mut(no) else {
            return;
        };
        if r.weaver != sender || r.phase != Phase::Seal {
            return;
        }
        r.witness_mask = Some(witnesses);
        self.begin_weave(host);
    }

    fn begin_weave(&mut self, host: &mut dyn Host) {
        let now = self.now;
        let Some(night) = &mut self.night else {
            return;
        };
        let secs = u64::from(night.settings.weave_secs);
        if let Some(r) = &mut night.round {
            r.phase = Phase::Weave;
            r.phase_at = now;
            r.deadline = now + secs * 1000;
            r.drift_at = now;
        }
        self.announce(host, "weave");
        self.refresh(host);
    }

    fn on_stars(&mut self, sender: u32, no: u8, stars: Vec<Pos>, lock: bool) {
        let Some(night) = &mut self.night else {
            return;
        };
        let Some(r) = night.round.as_mut().filter(|r| r.no == no) else {
            return;
        };
        if !r.is_threader(sender) || stars.len() != r.stars_each || r.locks.contains_key(&sender) {
            return;
        }
        let open = match r.phase {
            Phase::Weave => true,
            // Locks may arrive a little after this client's own deadline or the reveal.
            Phase::Dawn | Phase::Reveal => lock,
            _ => false,
        };
        if !open {
            return;
        }
        if let Some(old) = r.positions.get(&sender) {
            let moved: u64 = old.iter().zip(&stars).map(|(a, b)| distance(*a, *b)).sum();
            *night.travel.entry(sender).or_default() += moved;
        }
        r.positions.insert(sender, stars.clone());
        if lock {
            r.locks.insert(sender, stars);
        }
        self.page_stars_dirty = true;
    }

    fn on_pluck(&mut self, host: &mut dyn Host, by: u32, target: u32, star: u8) {
        if self.page_ready {
            host.post(view::pluck(by, target, star));
        }
        if target == self.me {
            self.play(host, "sounds/shimmer.ogg");
            self.say(
                host,
                &crate::text::t_with("say.plucked", &[("name", &self.name(by))]),
            );
        }
    }

    // ----- Intents from the page (untrusted, validated here) -----

    pub fn on_page(&mut self, host: &mut dyn Host, intent: view::Intent) {
        use view::Intent;
        match intent {
            Intent::Ready => {
                self.page_ready = true;
                host.post(view::strings());
                self.refresh(host);
            }
            Intent::Move { star, x, y } => self.move_star(star, x, y),
            Intent::Settle { star } => {
                let height = self
                    .round()
                    .and_then(|r| r.positions.get(&self.me))
                    .and_then(|p| p.get(usize::from(star)))
                    .map(|p| p.1);
                if let Some(y) = height {
                    // Higher on the loom, higher note.
                    let note = 5 - (u32::from(y) * 5 / (u32::from(crate::wire::LOOM_H) + 1));
                    self.play(host, &format!("sounds/settle-{note}.ogg"));
                }
            }
            Intent::Pluck { target, star } => {
                let me = self.me;
                let ok = self
                    .round()
                    .filter(|r| r.is_weaver(me) && r.phase == Phase::Weave)
                    .filter(|r| r.is_threader(target) && usize::from(star) < r.stars_each)
                    .map(|r| r.no);
                if let Some(no) = ok {
                    host.send(
                        &Message::Pluck {
                            round: no,
                            target,
                            star,
                        },
                        false,
                    );
                    self.on_pluck(host, me, target, star);
                }
            }
            Intent::TieOff => self.tie_off(host),
            Intent::Begin | Intent::WeaveReady => self.weave_ready(host),
            Intent::Settings { weave_secs, mixed } => {
                let me = self.me;
                if let Some(night) = &mut self.night
                    && night.lamplighter() == Some(me)
                    && night.round_no == 0
                    && matches!(weave_secs, 60 | 90 | 120)
                {
                    night.settings = Settings { weave_secs, mixed };
                    night.roster_due = true;
                    self.refresh(host);
                }
            }
            Intent::Seat { play } => self.seat(host, play),
            Intent::Tapestry => self.tapestry(host),
            Intent::PostSummary => self.post_summary(host),
            Intent::AnotherNight => self.another_night(host),
            Intent::Name(name) => {
                if let Some(night) = &mut self.night {
                    night.sky_name = name;
                    self.save_sky(host);
                }
                self.refresh(host);
            }
            Intent::Guess(text) => {
                self.guess = text;
            }
            Intent::Volume(volume) => {
                self.prefs.volume = volume.min(100);
                host.save(&self.prefs);
            }
            Intent::Mute(muted) => {
                self.prefs.muted = muted;
                if muted {
                    host.stop_sounds();
                }
                host.save(&self.prefs);
                self.refresh(host);
            }
            Intent::CloseNight => {
                let me = self.me;
                if self
                    .activity
                    .as_ref()
                    .is_some_and(|(_, starter)| *starter == me)
                    || self.night.as_ref().and_then(Night::lamplighter) == Some(me)
                {
                    host.end_stage();
                }
            }
            // Place changes are handled by the host glue, which owns the web view.
            Intent::Place(_) | Intent::Rules => {}
        }
    }

    fn move_star(&mut self, star: u8, x: i32, y: i32) {
        let me = self.me;
        let Some(night) = &mut self.night else {
            return;
        };
        let Some(r) = &mut night.round else {
            return;
        };
        if !r.is_threader(me) || r.phase != Phase::Weave || usize::from(star) >= r.stars_each {
            return;
        }
        let mut mine = r
            .all_stars()
            .into_iter()
            .filter(|s| s.0 == me)
            .map(|s| s.2)
            .collect::<Vec<_>>();
        let Some(slot) = mine.get_mut(usize::from(star)) else {
            return;
        };
        let to = rules::clamp(x, y);
        *night.travel.entry(me).or_default() += distance(*slot, to);
        *slot = to;
        r.positions.insert(me, mine);
        r.own_dirty = true;
        self.page_stars_dirty = true;
    }

    fn tie_off(&mut self, host: &mut dyn Host) {
        let me = self.me;
        let now = self.now;
        let Some(r) = self.night.as_mut().and_then(|n| n.round.as_mut()) else {
            return;
        };
        if r.weaver != me || r.phase != Phase::Weave {
            return;
        }
        let spare = (r.deadline.saturating_sub(now) / 1000).min(255) as u8;
        r.bonus = rules::tie_off_bonus(spare);
        r.spare = Some(spare);
        host.send(&Message::TieOff { round: r.no, spare }, false);
        self.enter_dawn(host);
    }

    /// "I'm ready to weave" (or "Begin the night" for the first round).
    fn weave_ready(&mut self, host: &mut dyn Host) {
        let me = self.me;
        let Some(night) = &self.night else {
            return;
        };
        if night.next_weaver() != Some(me) || !night.between_rounds() || night.players.len() < 2 {
            return;
        }
        let (no, rev) = (night.round_no + 1, night.rev);
        if let Some(prev) = night.history.last().cloned()
            && !night.posted_rounds.contains(&prev.no)
        {
            host.chat_send(&view::round_line(self, &prev));
            if let Some(night) = &mut self.night {
                night.posted_rounds.insert(prev.no);
            }
        }
        host.send(&Message::Ready { round: no, rev }, false);
        self.start_round(host, no, me, rev);
    }

    fn seat(&mut self, host: &mut dyn Host, play: bool) {
        let me = self.me;
        let Some(night) = &mut self.night else {
            return;
        };
        if night.players.contains(&me) {
            if !play && night.between_rounds() {
                // Stepping back to watch: leave the seat, stay in the activity.
                host.send(&Message::Leave, false);
                self.drop_player(host, me);
                if let Some(night) = &mut self.night {
                    night.spectators.insert(me);
                }
                host.send(&Message::Join { play: false }, false);
                self.refresh(host);
            }
            return;
        }
        if play {
            night.spectators.remove(&me);
        }
        host.send(&Message::Join { play }, false);
        self.refresh(host);
    }

    fn tapestry(&mut self, host: &mut dyn Host) {
        let me = self.me;
        let Some(night) = &self.night else {
            return;
        };
        if night.lamplighter() != Some(me) || !night.between_rounds() || night.ended {
            return;
        }
        let id = night.id;
        if let Some(prev) = night.history.last().cloned()
            && !night.posted_rounds.contains(&prev.no)
        {
            host.chat_send(&view::round_line(self, &prev));
            if let Some(night) = &mut self.night {
                night.posted_rounds.insert(prev.no);
            }
        }
        host.send(&Message::End { night: id }, false);
        self.end_night(host, me);
    }

    fn end_night(&mut self, host: &mut dyn Host, by: u32) {
        let unix = self.unix;
        let Some(night) = &mut self.night else {
            return;
        };
        if night.ended {
            return;
        }
        night.ended = true;
        night.ended_at = unix;
        night.round = None;
        if by != self.me
            && let Some(prev) = night.history.last().cloned()
        {
            let id = night.id;
            let hash = host.line_hash(&view::round_line(self, &prev));
            host.supersede(&view::round_card_id(id, prev.no), by, hash);
        }
        if let Some(night) = &self.night {
            let card = view::summary_card_id(night.id);
            host.show_card(&card, view::summary_card(self));
            if let Some(lamplighter) = night.lamplighter() {
                let hash = host.line_hash(&view::summary_line(self));
                host.supersede(&card, lamplighter, hash);
            }
        }
        self.save_sky(host);
        self.announce(host, "results");
        self.refresh(host);
    }

    /// The Lamplighter starts a new night with the same group once this one is over. The new
    /// night id in the roster tells everyone else to start over too.
    fn another_night(&mut self, host: &mut dyn Host) {
        let me = self.me;
        let Some(night) = &self.night else {
            return;
        };
        if !night.ended || night.lamplighter() != Some(me) {
            return;
        }
        let Some(id) = Self::random_array::<4>(host).map(u32::from_be_bytes) else {
            return;
        };
        let mut fresh = Night::new(id, 0, night.settings, night.players.clone());
        fresh.spectators = night.spectators.clone();
        self.night = Some(fresh);
        self.send_roster(host);
        self.refresh(host);
    }

    fn post_summary(&mut self, host: &mut dyn Host) {
        let me = self.me;
        let Some(night) = &self.night else {
            return;
        };
        if !night.ended || night.summary_posted || night.lamplighter() != Some(me) {
            return;
        }
        host.chat_send(&view::summary_line(self));
        if let Some(night) = &mut self.night {
            night.summary_posted = true;
        }
        self.refresh(host);
    }

    fn save_sky(&mut self, host: &mut dyn Host) {
        let Some(night) = &self.night else {
            return;
        };
        if !night.ended {
            return;
        }
        let figures: Vec<(String, u8)> = night
            .history
            .iter()
            .filter_map(|r| {
                let figure = deck::get(r.figure?)?;
                Some((figure.id.to_string(), r.resonance?))
            })
            .collect();
        if figures.is_empty() {
            return;
        }
        let sky = PastSky {
            name: if night.sky_name.is_empty() {
                crate::text::t("results.untitled")
            } else {
                night.sky_name.clone()
            },
            night: night.id,
            at: night.ended_at,
            team: night.team().unwrap_or(0),
            figures,
        };
        self.prefs.skies.retain(|s| s.night != sky.night);
        self.prefs.skies.push(sky);
        let extra = self.prefs.skies.len().saturating_sub(PAST_SKIES);
        self.prefs.skies.drain(..extra);
        host.save(&self.prefs);
    }

    // ----- Time -----

    /// Called every [`TICK_MS`] while a night is known. `unix` is the wall clock in seconds.
    pub fn tick(&mut self, host: &mut dyn Host, unix: u64) {
        self.now += TICK_MS;
        self.unix = unix;
        let me = self.me;
        let now = self.now;
        let live = host.live();

        if let Some(night) = &mut self.night
            && night.roster_due
            && night.lamplighter() == Some(me)
        {
            night.roster_due = false;
            self.send_roster(host);
        }

        let Some(phase) = self.round().map(|r| r.phase) else {
            self.push_page_stars(host);
            return;
        };
        match phase {
            Phase::Seal => self.tick_seal(host),
            Phase::Weave => {
                self.tick_drift();
                let Some(r) = self.night.as_mut().and_then(|n| n.round.as_mut()) else {
                    return;
                };
                let first_light = now + rules::FIRST_LIGHT_MS >= r.deadline;
                if first_light && !r.burst {
                    r.burst = true;
                    if r.is_threader(me) {
                        host.start_game_burst();
                    }
                    self.announce(host, "first-light");
                }
                let Some(r) = self.night.as_mut().and_then(|n| n.round.as_mut()) else {
                    return;
                };
                // The relay, when there is one, streams movement live; Mumble plugin data always
                // carries it too at its own rate, so clients without the relay keep up and the
                // game never depends on it.
                if r.own_dirty
                    && live
                    && now >= r.last_live + SEND_LIVE_MS
                    && let Some(stars) = r.positions.get(&me).cloned()
                {
                    r.last_live = now;
                    host.send(&Message::Stars { round: r.no, stars }, true);
                }
                let every = if first_light {
                    SEND_BURST_MS
                } else {
                    SEND_SLOW_MS
                };
                if r.own_dirty && now >= r.last_sent + every {
                    r.own_dirty = false;
                    r.last_sent = now;
                    if let Some(stars) = r.positions.get(&me).cloned() {
                        host.send(&Message::Stars { round: r.no, stars }, false);
                    }
                }
                if now >= r.deadline {
                    self.enter_dawn(host);
                }
            }
            Phase::Dawn => {
                let Some(r) = self.round() else { return };
                let present: Vec<u32> = r
                    .threaders
                    .iter()
                    .copied()
                    .filter(|t| !r.left.contains(t))
                    .collect();
                let all_locked = present.iter().all(|t| r.locks.contains_key(t));
                if r.weaver == me && (all_locked || now >= r.phase_at + LOCK_WAIT_MS) {
                    self.reveal(host);
                } else if now >= r.phase_at + STALL_MS {
                    self.void(host, "stalled");
                }
            }
            Phase::Reveal => {
                if self.round().is_some_and(|r| r.outcome.is_none()) {
                    self.try_score(host);
                } else {
                    self.tick_verify(host);
                }
            }
            Phase::Void => {}
        }
        self.push_page_stars(host);
    }

    fn tick_seal(&mut self, host: &mut dyn Host) {
        let me = self.me;
        let now = self.now;
        let Some(r) = self.night.as_mut().and_then(|n| n.round.as_mut()) else {
            return;
        };
        if r.weaver != me {
            if now >= r.phase_at + STALL_MS {
                self.void(host, "stalled");
            }
            return;
        }
        let present: Vec<u32> = r
            .threaders
            .iter()
            .copied()
            .filter(|t| !r.left.contains(t))
            .collect();
        let all = present.iter().all(|t| r.witnesses.contains_key(t));
        if !all && now < r.phase_at + SEAL_WAIT_MS {
            return;
        }
        let mask = r
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| r.witnesses.contains_key(s))
            .fold(0u16, |m, (i, _)| m | (1 << i));
        r.witness_mask = Some(mask);
        let no = r.no;
        if let (Some(twin), Some(secret)) = (r.twin, r.secret) {
            host.send_direct(twin, &Message::Share { round: no, secret });
        }
        host.send(
            &Message::Weave {
                round: no,
                witnesses: mask,
            },
            false,
        );
        self.begin_weave(host);
    }

    /// Drift: the drifting star's owner sees it pulled away every second and must hold it.
    fn tick_drift(&mut self) {
        let me = self.me;
        let now = self.now;
        let Some(night) = &mut self.night else {
            return;
        };
        let id = night.id;
        let Some(r) = &mut night.round else {
            return;
        };
        if r.twist != Twist::Drift || !r.is_threader(me) || now < r.drift_at + 1000 {
            return;
        }
        r.drift_at = now;
        let stars = r.all_stars();
        let Some((index, (dx, dy))) = rules::drift(id, r.no, r.threaders.len() * r.stars_each)
        else {
            return;
        };
        let Some(&(owner, star, _)) = stars.get(index) else {
            return;
        };
        if owner != me {
            return;
        }
        let mut mine: Vec<Pos> = stars.iter().filter(|s| s.0 == me).map(|s| s.2).collect();
        if let Some(p) = mine.get_mut(usize::from(star)) {
            *p = rules::clamp(i32::from(p.0) + dx, i32::from(p.1) + dy);
        }
        r.positions.insert(me, mine);
        r.own_dirty = true;
        self.page_stars_dirty = true;
    }

    /// The deadline passed (or the Weaver tied off): positions lock.
    fn enter_dawn(&mut self, host: &mut dyn Host) {
        let me = self.me;
        let now = self.now;
        let Some(r) = self.night.as_mut().and_then(|n| n.round.as_mut()) else {
            return;
        };
        if r.phase != Phase::Weave {
            return;
        }
        r.phase = Phase::Dawn;
        r.phase_at = now;
        if r.is_threader(me) {
            let stars: Vec<Pos> = r
                .all_stars()
                .into_iter()
                .filter(|s| s.0 == me)
                .map(|s| s.2)
                .collect();
            r.locks.insert(me, stars.clone());
            r.positions.insert(me, stars.clone());
            host.send(&Message::Lock { round: r.no, stars }, false);
        }
        self.announce(host, "dawn");
        self.refresh(host);
    }

    fn reveal(&mut self, host: &mut dyn Host) {
        let now = self.now;
        let Some(r) = self.night.as_mut().and_then(|n| n.round.as_mut()) else {
            return;
        };
        let Some(secret) = r.secret else {
            return;
        };
        let mask = r
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| r.locks.contains_key(s) && !r.left.contains(s))
            .fold(0u16, |m, (i, _)| m | (1 << i));
        r.lock_mask = Some(mask);
        r.phase = Phase::Reveal;
        r.phase_at = now;
        host.send(
            &Message::Reveal {
                round: r.no,
                secret,
                locks: mask,
            },
            false,
        );
        self.try_score(host);
    }

    /// Scores the round once every scored lock is here (or waiting is over).
    fn try_score(&mut self, host: &mut dyn Host) {
        let now = self.now;
        let me = self.me;
        let Some(night) = &mut self.night else {
            return;
        };
        let Some(r) = &mut night.round else {
            return;
        };
        if r.outcome.is_some() || r.phase != Phase::Reveal {
            return;
        }
        let Some(mask) = r.lock_mask else {
            return;
        };
        let scored: Vec<u32> = r
            .slots
            .iter()
            .enumerate()
            .filter(|(i, s)| mask & (1 << i) != 0 && r.is_threader(**s))
            .map(|(_, &s)| s)
            .collect();
        let complete = scored.iter().all(|s| r.locks.contains_key(s));
        if !complete && now < r.phase_at + SCORE_WAIT_MS {
            return;
        }
        let mut stars: Vec<(u32, u8, Pos)> = Vec::new();
        for s in &scored {
            if let Some(lock) = r.locks.get(s) {
                for (i, p) in lock.iter().enumerate() {
                    stars.push((*s, i as u8, *p));
                }
            }
        }
        let seal_ok = match (r.commit, r.secret) {
            (Some(c), Some(secret)) => fair::commit(&secret, r.no) == c,
            _ => false,
        };
        let figure = r.known_figure();
        let mut verify = if r.commit.is_none() || figure.is_none() {
            Verify::Missing
        } else if !seal_ok {
            Verify::SealBroken
        } else {
            Verify::Pending
        };
        if !complete && verify == Verify::Pending {
            verify = Verify::Missing;
        }
        let Some(fig) = figure.and_then(deck::get).filter(|_| stars.len() >= 2) else {
            r.phase = Phase::Void;
            r.phase_at = now;
            r.void_reason = Some(if stars.len() < 2 {
                "too-few"
            } else {
                "unverified"
            });
            self.finish_round(host);
            return;
        };
        let points = fig.sample(stars.len());
        let sky: Vec<Point> = stars
            .iter()
            .map(|s| (i32::from(s.2.0), i32::from(s.2.1)))
            .collect();
        let Some(result) = score::score(&sky, &points) else {
            return;
        };
        let resonance = result.resonance.saturating_add(r.bonus).min(100);
        let hash = fair::result_hash(
            r.no,
            figure.unwrap_or(0),
            &stars.iter().map(|s| (s.0, s.2)).collect::<Vec<_>>(),
            resonance,
        );
        let transform = score::fit(&points, &sky);
        let targets = result
            .matched
            .iter()
            .map(|&j| match transform {
                Some(t) => score::apply(t, points[j]),
                None => (f64::from(points[j].0), f64::from(points[j].1)),
            })
            .collect();
        let ghost = fig
            .lines()
            .iter()
            .map(|line| {
                line.iter()
                    .map(|&(x, y)| {
                        let p = (x * 16, y * 16);
                        match transform {
                            Some(t) => score::apply(t, p),
                            None => (f64::from(p.0), f64::from(p.1)),
                        }
                    })
                    .collect()
            })
            .collect();
        let outcome = Outcome {
            figure,
            stars: stars
                .iter()
                .zip(&result.stars)
                .map(|(s, &res)| (s.0, s.1, s.2, res))
                .collect(),
            targets,
            order: result.matched.clone(),
            ghost,
            bonus: r.bonus,
            spare: r.spare,
            resonance,
            hash,
            verify,
            at: now,
        };
        r.hashes.insert(me, hash);
        let no = r.no;
        r.outcome = Some(outcome);
        if r.slots.contains(&me) {
            host.send(&Message::Result { round: no, hash }, false);
        }
        self.play(host, "sounds/thread-pull.ogg");
        if resonance >= score::WOVEN {
            self.play(host, "sounds/chime.ogg");
        }
        self.finish_round(host);
        self.announce(host, "reveal");
    }

    fn tick_verify(&mut self, host: &mut dyn Host) {
        let now = self.now;
        let me = self.me;
        let Some(night) = &mut self.night else {
            return;
        };
        let Some(r) = &mut night.round else {
            return;
        };
        let Some(outcome) = &mut r.outcome else {
            return;
        };
        if outcome.verify != Verify::Pending {
            return;
        }
        let mine = outcome.hash;
        if r.hashes.values().any(|h| *h != mine) {
            outcome.verify = Verify::Differs;
        } else {
            let everyone = r
                .slots
                .iter()
                .filter(|s| !r.left.contains(s) && **s != me)
                .all(|s| r.hashes.contains_key(s));
            if !everyone && now < outcome.at + VERIFY_WAIT_MS {
                return;
            }
            outcome.verify = Verify::Verified;
        }
        let verified = outcome.verify == Verify::Verified;
        let no = r.no;
        if let Some(record) = night.history.iter_mut().find(|h| h.no == no) {
            record.verified = verified;
        }
        let id = night.id;
        if let Some(record) = night.history.iter().find(|h| h.no == no).cloned() {
            host.show_card(
                &view::round_card_id(id, no),
                view::round_card(self, &record),
            );
        }
        self.refresh(host);
    }

    fn void(&mut self, host: &mut dyn Host, reason: &'static str) {
        let now = self.now;
        let Some(r) = self.night.as_mut().and_then(|n| n.round.as_mut()) else {
            return;
        };
        if r.finished() {
            return;
        }
        r.phase = Phase::Void;
        r.phase_at = now;
        r.void_reason = Some(reason);
        self.finish_round(host);
    }

    /// Records the round and lets the night move on.
    fn finish_round(&mut self, host: &mut dyn Host) {
        let me = self.me;
        let Some(night) = &mut self.night else {
            return;
        };
        let Some(r) = &night.round else {
            return;
        };
        if night.history.iter().any(|h| h.no == r.no) {
            return;
        }
        let outcome = r.outcome.as_ref();
        let by_owner = |pick: fn(u8, u8) -> bool| {
            outcome.and_then(|o| {
                o.stars
                    .iter()
                    .fold(None::<(u32, u8)>, |best, s| match best {
                        Some(b) if !pick(s.3, b.1) => Some(b),
                        _ => Some((s.0, s.3)),
                    })
                    .map(|b| b.0)
            })
        };
        let record = Record {
            no: r.no,
            weaver: r.weaver,
            figure: outcome.and_then(|o| o.figure),
            resonance: outcome.map(|o| o.resonance),
            verified: false,
            truest: by_owner(|a, b| a > b),
        };
        for w in [Some(r.weaver), r.twin].into_iter().flatten() {
            if !night.weaved.contains(&w) {
                night.weaved.push(w);
            }
        }
        if let Some(f) = record.figure.filter(|_| {
            outcome.is_some_and(|o| !matches!(o.verify, Verify::SealBroken | Verify::Missing))
        }) {
            night.used |= 1 << f;
        }
        let id = night.id;
        night.history.push(record.clone());
        if night.lamplighter() == Some(me) {
            let pending = std::mem::take(&mut night.pending);
            for p in pending {
                if night.players.len() < MAX_PLAYERS && !night.players.contains(&p) {
                    night.players.push(p);
                    night.spectators.remove(&p);
                }
            }
            night.roster_due = true;
        }
        host.show_card(
            &view::round_card_id(id, record.no),
            view::round_card(self, &record),
        );
        self.refresh(host);
    }

    // ----- Output -----

    fn send_roster(&mut self, host: &mut dyn Host) {
        let Some(night) = &mut self.night else {
            return;
        };
        night.rev = night.rev.wrapping_add(1);
        let players = night.players.clone();
        night.revs.push((night.rev, players.clone()));
        if night.revs.len() > 8 {
            night.revs.remove(0);
        }
        let message = Message::Roster {
            night: night.id,
            rev: night.rev,
            round: night.round_no,
            settings: night.settings,
            weaved: night.weaved_mask(),
            players,
        };
        host.send(&message, false);
    }

    fn play(&self, host: &mut dyn Host, sound: &str) {
        if !self.prefs.muted && self.prefs.volume > 0 && self.joined {
            host.play(sound, f32::from(self.prefs.volume) / 100.0);
        }
    }

    fn say(&self, host: &mut dyn Host, text: &str) {
        if self.page_ready {
            host.post(view::say(text));
        }
    }

    /// Phase announcements for the screen reader.
    fn announce(&self, host: &mut dyn Host, what: &str) {
        if let Some(text) = view::announcement(self, what) {
            self.say(host, &text);
        }
    }

    fn clear_badges(&self, host: &mut dyn Host) {
        if let Some(night) = &self.night {
            for p in &night.players {
                host.set_badge(*p, false);
            }
        }
    }

    /// Sends the whole view to the page.
    pub fn refresh(&mut self, host: &mut dyn Host) {
        if self.page_ready {
            host.post(view::view(self));
            self.page_stars_dirty = false;
        }
    }

    fn push_page_stars(&mut self, host: &mut dyn Host) {
        if self.page_ready
            && self.page_stars_dirty
            && self.now >= self.last_page_stars + PAGE_STARS_MS
        {
            self.page_stars_dirty = false;
            self.last_page_stars = self.now;
            if let Some(json) = view::stars(self) {
                host.post(json);
            }
        }
    }

    /// A star a Threader may see under Fog.
    pub fn visible(&self, r: &Round, pos: Pos, owner: u32) -> bool {
        if r.twist != Twist::Fog || !r.is_threader(self.me) || owner == self.me {
            return true;
        }
        r.positions
            .get(&self.me)
            .into_iter()
            .flatten()
            .any(|p| distance(*p, pos) <= u64::from(rules::FOG_RADIUS))
            || r.all_stars()
                .iter()
                .filter(|s| s.0 == self.me)
                .any(|s| distance(s.2, pos) <= u64::from(rules::FOG_RADIUS))
    }
}

pub fn distance(a: Pos, b: Pos) -> u64 {
    let dx = i64::from(a.0) - i64::from(b.0);
    let dy = i64::from(a.1) - i64::from(b.1);
    score::isqrt((dx * dx + dy * dy) as u64)
}
