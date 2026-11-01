//! The roll protocol (FR-6.82): commit, witness, reveal. Host calls go through [`Host`], so the
//! whole flow runs in native tests.
//!
//! The roller commits, everyone else with the plugin may witness after a random delay unless
//! two witnesses were already seen, and the roller reveals after two witnesses or 5 s. Every
//! client checks the reveal against what it saw itself and computes the dice on its own.

use mumcord_plugin_sdk::types::Error;
use mumcord_plugin_sdk::ui::{El, Slot};

use crate::fair::{self, Hash, Outcome, SECRET_BYTES, Secret};
use crate::notation::{self, Notation};
use crate::view::{self, Card, Entry, Problem, Reason, Status, Who};
use crate::wire::{MAX_WITNESSES, Message};

/// Witnesses wait a random 0 … 1.5 s so they do not all answer at once.
pub const WITNESS_DELAY_MAX_MS: u16 = 1500;
/// The roller reveals at the latest this long after the commit.
pub const REVEAL_TIMEOUT_MS: u32 = 5000;
/// A receiver gives up this long after the roller should have revealed: after the second
/// witness, or after the reveal timeout.
pub const NOT_REVEALED_AFTER_MS: u32 = 5000;
/// Rolls of other people tracked at once; a flood of commits pushes out the oldest.
pub const MAX_TRACKED_ROLLS: usize = 16;
pub const HISTORY_LIMIT: usize = 50;

/// Cards kept for drawing again after a language change.
const MAX_SHOWN_CARDS: usize = 32;

/// What a chat card shows.
enum Shown {
    Roll {
        card: String,
        who: Who,
        notation: Notation,
        status: Status,
        outcome: Option<Outcome>,
    },
    Error {
        card: String,
        problem: Problem,
    },
}

impl Shown {
    fn card(&self) -> &str {
        match self {
            Shown::Roll { card, .. } | Shown::Error { card, .. } => card,
        }
    }

    fn render(&self) -> El {
        match self {
            Shown::Roll {
                who,
                notation,
                status,
                outcome,
                ..
            } => view::card(&Card {
                who,
                notation,
                status,
                outcome: outcome.as_ref(),
            }),
            Shown::Error { problem, .. } => view::error_card(*problem),
        }
    }
}

/// Calls into Mumcord. The plugin talks only to the user's current channel.
pub trait Host {
    fn random(&mut self, len: u32) -> Option<Vec<u8>>;
    fn send(&mut self, message: &Message) -> Result<(), Error>;
    fn start_game_burst(&mut self);
    fn chat_send(&mut self, text: &str) -> Result<(), Error>;
    fn line_hash(&mut self, text: &str) -> u32;
    fn supersede(&mut self, card: &str, sender: u32, hash: u32);
    fn start_timer(&mut self, ms: u32) -> Option<u32>;
    fn show(&mut self, slot: Slot, tree: El);
    /// Opens the large panel popover; needs a fresh click, which the caller just received.
    fn open_popover(&mut self);
    fn warn(&mut self, message: &str);
}

/// The local user's roll in flight. One at a time.
struct OwnRoll {
    id: u32,
    notation: Notation,
    secret: Secret,
    witnesses: Vec<Secret>,
    card: String,
}

/// Someone else's roll, from the commit until it is settled.
struct OtherRoll {
    roller: u32,
    name: String,
    id: u32,
    notation: Notation,
    commitment: Hash,
    /// Witness values from other people, in arrival order (the channel order).
    seen: Vec<Secret>,
    /// The value this client sent as a witness, if any.
    mine: Option<Secret>,
    /// Only the newest deadline timer counts; older ones are ignored when they fire.
    deadline: Option<u32>,
    card: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Reveal { id: u32 },
    Witness { roller: u32, id: u32 },
    Deadline { roller: u32, id: u32 },
}

#[derive(Default)]
pub struct App {
    own: Option<OwnRoll>,
    others: Vec<OtherRoll>,
    timers: Vec<(u32, Purpose)>,
    history: Vec<Entry>,
    shown: Vec<Shown>,
    panel: bool,
    expanded: bool,
    error_cards: u32,
}

impl App {
    /// The granted permissions changed; the history panel needs the optional `ui.panel`.
    pub fn set_granted(&mut self, host: &mut impl Host, granted: &[String]) {
        let panel = granted.iter().any(|p| p == "ui.panel");
        let turned_on = panel && !self.panel;
        self.panel = panel;
        if turned_on {
            self.show_history(host);
        }
    }

    /// `/roll <args>` from the chat box.
    pub fn on_command(&mut self, host: &mut impl Host, args: &str) {
        if self.own.is_some() {
            return self.error(host, Problem::Busy);
        }
        let notation = match notation::parse(args) {
            Ok(notation) => notation,
            Err(e) => return self.error(host, Problem::Notation(e)),
        };
        let Some(bytes) = host.random(4 + SECRET_BYTES as u32) else {
            return self.error(host, Problem::NoRandom);
        };
        let Some((id, secret)) = split_id_secret(&bytes) else {
            return self.error(host, Problem::NoRandom);
        };
        let canonical = notation.canonical();
        let roll = OwnRoll {
            id,
            notation,
            secret,
            witnesses: Vec::new(),
            card: format!("roll-me-{id:08x}"),
        };
        // Commit and reveal fit the normal budget too; the burst only makes the roll faster.
        host.start_game_burst();
        let commit = Message::Commit {
            roll_id: id,
            commitment: fair::commitment(id, &canonical, &secret),
            notation: canonical,
        };
        match host.send(&commit) {
            Ok(()) => {
                self.show_card(
                    host,
                    &roll.card,
                    &Who::Me,
                    &notation,
                    &Status::Rolling,
                    None,
                );
                self.start_timer(host, REVEAL_TIMEOUT_MS, Purpose::Reveal { id });
                self.own = Some(roll);
            }
            // Nobody in the channel runs the plugin, or the user shares with nobody.
            Err(Error::NoRecipients | Error::Hidden) => self.settle_own(host, roll, false),
            Err(Error::RateLimited) => self.error(host, Problem::RateLimited),
            Err(e) => {
                host.warn(&format!("could not send the commit: {e:?}"));
                self.error(host, Problem::NotSent);
            }
        }
    }

    pub fn on_peer_message(
        &mut self,
        host: &mut impl Host,
        sender: u32,
        name: &str,
        kind: u8,
        payload: &[u8],
        direct: bool,
    ) {
        // Rolls are channel-wide; direct messages are not part of the protocol.
        if direct {
            return;
        }
        let Some(message) = Message::decode(kind, payload) else {
            host.warn("ignored a malformed dice message");
            return;
        };
        match message {
            Message::Commit {
                roll_id,
                commitment,
                notation,
            } => self.on_commit(host, sender, name, roll_id, commitment, &notation),
            Message::Witness {
                roller,
                roll_id,
                value,
            } => self.on_witness(host, sender, roller, roll_id, value),
            Message::Reveal {
                roll_id,
                secret,
                witnesses,
            } => self.on_reveal(host, sender, roll_id, secret, witnesses),
        }
    }

    pub fn on_timer(&mut self, host: &mut impl Host, timer: u32) {
        let Some(index) = self.timers.iter().position(|(id, _)| *id == timer) else {
            return;
        };
        let (_, purpose) = self.timers.swap_remove(index);
        match purpose {
            Purpose::Reveal { id } => {
                if self.own.as_ref().is_some_and(|roll| roll.id == id) {
                    self.reveal(host);
                }
            }
            Purpose::Witness { roller, id } => self.witness(host, roller, id),
            Purpose::Deadline { roller, id } => {
                let Some(index) = self.find(roller, id) else {
                    return;
                };
                if self.others[index].deadline == Some(timer) {
                    let roll = self.others.remove(index);
                    self.settle_other(host, &roll, Status::NotRevealed, None);
                }
            }
        }
    }

    fn on_commit(
        &mut self,
        host: &mut impl Host,
        roller: u32,
        name: &str,
        id: u32,
        commitment: Hash,
        text: &str,
    ) {
        let notation = match notation::parse(text) {
            Ok(notation) if notation.canonical() == text => notation,
            _ => {
                host.warn("ignored a commit with an invalid notation");
                return;
            }
        };
        if self.find(roller, id).is_some() {
            return;
        }
        if self.others.len() >= MAX_TRACKED_ROLLS {
            self.others.remove(0);
        }
        let mut roll = OtherRoll {
            roller,
            name: name.to_string(),
            id,
            notation,
            commitment,
            seen: Vec::new(),
            mine: None,
            deadline: None,
            card: format!("roll-{roller}-{id:08x}"),
        };
        let delay = host
            .random(2)
            .and_then(|b| b.get(..2).map(|b| u16::from_be_bytes([b[0], b[1]])))
            .map_or(0, |value| value % (WITNESS_DELAY_MAX_MS + 1));
        self.start_timer(host, u32::from(delay), Purpose::Witness { roller, id });
        roll.deadline = self.start_timer(
            host,
            REVEAL_TIMEOUT_MS + NOT_REVEALED_AFTER_MS,
            Purpose::Deadline { roller, id },
        );
        self.show_card(
            host,
            &roll.card,
            &Who::Peer(roll.name.clone()),
            &notation,
            &Status::Rolling,
            None,
        );
        self.others.push(roll);
    }

    fn on_witness(
        &mut self,
        host: &mut impl Host,
        sender: u32,
        roller: u32,
        id: u32,
        value: Secret,
    ) {
        // The roller's own session is unknown to the plugin; the random roll id identifies
        // the own roll.
        if let Some(own) = self.own.as_mut().filter(|roll| roll.id == id) {
            if own.witnesses.len() < MAX_WITNESSES && !own.witnesses.contains(&value) {
                own.witnesses.push(value);
                if own.witnesses.len() == MAX_WITNESSES {
                    self.reveal(host);
                }
            }
            return;
        }
        let Some(index) = self.find(roller, id) else {
            return;
        };
        let roll = &mut self.others[index];
        if sender == roll.roller || roll.seen.len() >= MAX_WITNESSES || roll.seen.contains(&value) {
            return;
        }
        roll.seen.push(value);
        if roll.seen.len() == MAX_WITNESSES {
            // The roller reveals now; the not-revealed deadline moves closer.
            let deadline = self.start_timer(
                host,
                NOT_REVEALED_AFTER_MS,
                Purpose::Deadline { roller, id },
            );
            self.others[index].deadline = deadline;
        }
    }

    fn on_reveal(
        &mut self,
        host: &mut impl Host,
        sender: u32,
        id: u32,
        secret: Secret,
        witnesses: Vec<Secret>,
    ) {
        let Some(index) = self.find(sender, id) else {
            return;
        };
        let roll = self.others.remove(index);
        let canonical = roll.notation.canonical();
        if fair::commitment(id, &canonical, &secret) != roll.commitment {
            let reason = Reason::Commitment;
            return self.settle_other(host, &roll, Status::Failed { reason }, None);
        }
        if !witnesses_match(&witnesses, &roll.seen, roll.mine) {
            let left_out = roll.mine.is_some_and(|mine| !witnesses.contains(&mine));
            let reason = if left_out {
                Reason::LeftOut
            } else {
                Reason::Witness
            };
            return self.settle_other(host, &roll, Status::Failed { reason }, None);
        }
        let outcome = fair::roll(&roll.notation, fair::seed(id, &secret, &witnesses));
        let status = if witnesses.is_empty() {
            Status::Alone
        } else {
            Status::Verified {
                witnesses: witnesses.len(),
            }
        };
        self.settle_other(host, &roll, status.clone(), Some(&outcome));
        // Hide the roller's text line behind this card; the hash comes from the numbers this
        // client computed, so a line with other numbers stays visible.
        let line = view::fallback_line(&roll.notation, &outcome, &status);
        let hash = host.line_hash(&line);
        host.supersede(&roll.card, roll.roller, hash);
    }

    fn witness(&mut self, host: &mut impl Host, roller: u32, id: u32) {
        let Some(index) = self.find(roller, id) else {
            return;
        };
        if self.others[index].seen.len() >= MAX_WITNESSES {
            return;
        }
        let Some(value) = host
            .random(SECRET_BYTES as u32)
            .and_then(|bytes| Secret::try_from(bytes.as_slice()).ok())
        else {
            host.warn("no random bytes for a witness value");
            return;
        };
        let message = Message::Witness {
            roller,
            roll_id: id,
            value,
        };
        match host.send(&message) {
            Ok(()) => self.others[index].mine = Some(value),
            Err(e) => host.warn(&format!("could not send a witness value: {e:?}")),
        }
    }

    fn reveal(&mut self, host: &mut impl Host) {
        let Some(roll) = self.own.take() else {
            return;
        };
        let message = Message::Reveal {
            roll_id: roll.id,
            secret: roll.secret,
            witnesses: roll.witnesses.clone(),
        };
        match host.send(&message) {
            Ok(()) => self.settle_own(host, roll, true),
            Err(e) => {
                // The others will show "Not revealed"; the roller must not claim a result.
                host.warn(&format!("could not send the reveal: {e:?}"));
                self.show_card(
                    host,
                    &roll.card,
                    &Who::Me,
                    &roll.notation,
                    &Status::NotRevealed,
                    None,
                );
                self.record(host, Who::Me, roll.notation, None, Status::NotRevealed);
            }
        }
    }

    /// Shows the final own card and posts the text line. `shared` is false when nobody else
    /// saw the commit.
    fn settle_own(&mut self, host: &mut impl Host, roll: OwnRoll, shared: bool) {
        let witnesses = if shared {
            roll.witnesses.as_slice()
        } else {
            &[]
        };
        let outcome = fair::roll(&roll.notation, fair::seed(roll.id, &roll.secret, witnesses));
        let status = if witnesses.is_empty() {
            Status::Alone
        } else {
            Status::Verified {
                witnesses: witnesses.len(),
            }
        };
        self.show_card(
            host,
            &roll.card,
            &Who::Me,
            &roll.notation,
            &status,
            Some(&outcome),
        );
        if let Err(e) = host.chat_send(&view::fallback_line(&roll.notation, &outcome, &status)) {
            host.warn(&format!("could not post the text line: {e:?}"));
        }
        self.record(host, Who::Me, roll.notation, Some(outcome.total), status);
    }

    fn settle_other(
        &mut self,
        host: &mut impl Host,
        roll: &OtherRoll,
        status: Status,
        outcome: Option<&Outcome>,
    ) {
        let who = Who::Peer(roll.name.clone());
        self.show_card(host, &roll.card, &who, &roll.notation, &status, outcome);
        self.record(host, who, roll.notation, outcome.map(|o| o.total), status);
    }

    fn record(
        &mut self,
        host: &mut impl Host,
        who: Who,
        notation: Notation,
        total: Option<i32>,
        status: Status,
    ) {
        if self.history.len() >= HISTORY_LIMIT {
            self.history.remove(0);
        }
        self.history.push(Entry {
            who,
            notation,
            total,
            status,
        });
        if self.panel {
            self.show_history(host);
        }
    }

    fn show_history(&self, host: &mut impl Host) {
        let tree = if self.expanded {
            view::panel_expanded(&self.history)
        } else {
            view::panel_inline(&self.history)
        };
        host.show(Slot::Panel, tree);
    }

    /// The panel moved between the strip and the large popover; each has its own layout.
    pub fn on_panel_visibility(&mut self, host: &mut impl Host, expanded: bool) {
        if self.expanded != expanded {
            self.expanded = expanded;
            if self.panel {
                self.show_history(host);
            }
        }
    }

    /// A click in the panel: a quick roll behaves exactly like the typed `/roll`.
    pub fn on_panel_action(&mut self, host: &mut impl Host, action: &str) {
        if action == view::HELP_ACTION {
            host.open_popover();
        } else if let Some(notation) = action.strip_prefix(view::QUICK_PREFIX) {
            self.on_command(host, notation);
        }
    }

    fn error(&mut self, host: &mut impl Host, problem: Problem) {
        self.error_cards = self.error_cards.wrapping_add(1);
        let card = format!("error-{}", self.error_cards);
        self.remember(Shown::Error {
            card: card.clone(),
            problem,
        });
        host.show(Slot::ChatCard(card), view::error_card(problem));
    }

    fn show_card(
        &mut self,
        host: &mut impl Host,
        card: &str,
        who: &Who,
        notation: &Notation,
        status: &Status,
        outcome: Option<&Outcome>,
    ) {
        let shown = Shown::Roll {
            card: card.to_string(),
            who: who.clone(),
            notation: *notation,
            status: status.clone(),
            outcome: outcome.cloned(),
        };
        host.show(Slot::ChatCard(card.to_string()), shown.render());
        self.remember(shown);
    }

    /// Keeps what a card shows, so it can be drawn again in another language. A card shown
    /// again (rolling, then final) replaces its older state.
    fn remember(&mut self, shown: Shown) {
        self.shown.retain(|old| old.card() != shown.card());
        if self.shown.len() >= MAX_SHOWN_CARDS {
            self.shown.remove(0);
        }
        self.shown.push(shown);
    }

    /// The language changed: draw every card and the history again.
    pub fn on_language_changed(&mut self, host: &mut impl Host) {
        for shown in &self.shown {
            host.show(Slot::ChatCard(shown.card().to_string()), shown.render());
        }
        if self.panel {
            self.show_history(host);
        }
    }

    fn find(&self, roller: u32, id: u32) -> Option<usize> {
        self.others
            .iter()
            .position(|roll| roll.roller == roller && roll.id == id)
    }

    fn start_timer(&mut self, host: &mut impl Host, ms: u32, purpose: Purpose) -> Option<u32> {
        let id = host.start_timer(ms)?;
        self.timers.push((id, purpose));
        Some(id)
    }
}

fn split_id_secret(bytes: &[u8]) -> Option<(u32, Secret)> {
    let (id, secret) = bytes.split_at_checked(4)?;
    Some((
        u32::from_be_bytes(id.try_into().ok()?),
        secret.try_into().ok()?,
    ))
}

/// The roller must use the first two witness values in channel order. This client knows that
/// order for everyone else's values; its own value may have reached the server before, between
/// or after them, so each of those positions is accepted.
pub fn witnesses_match(revealed: &[Secret], seen: &[Secret], mine: Option<Secret>) -> bool {
    let first_two = |list: &[Secret]| {
        let mut set: Vec<Secret> = list.iter().take(MAX_WITNESSES).copied().collect();
        set.sort_unstable();
        set
    };
    let mut revealed_set = revealed.to_vec();
    revealed_set.sort_unstable();
    if revealed_set == first_two(seen) {
        return true;
    }
    let Some(mine) = mine else {
        return false;
    };
    (0..=seen.len()).any(|position| {
        let mut order = seen.to_vec();
        order.insert(position, mine);
        revealed_set == first_two(&order)
    })
}
