//! The Poll logic (FR-6.81), independent of the real host so it can be tested natively.
//!
//! The creator's client is the authority: voters send their choice directly to the creator,
//! who counts it and tells the channel the tally. Everything else only mirrors what the creator
//! says, and accepts it only from the creator's session.

use std::collections::VecDeque;

use mumcord_plugin_sdk::{
    types::{Error, Peer},
    ui::{El, Slot},
};

use crate::view::{self, Role};
use crate::wire::{
    self, Create, KIND_CREATE, KIND_TALLY, KIND_VOTE, KIND_VOTERS, MAX_OPTIONS, MAX_VOTERS,
    MIN_OPTIONS, Tally, Vote, VoterEntry, Voters,
};
use mumcord_plugin_sdk::{events::Event, i18n::Strings};

/// The tally goes out at most once per 2 s (FR-6.81) and right after a 2 s wait for more votes.
pub const TALLY_DELAY_MS: u32 = 2_000;
/// When the host is out of send budget, the queue is tried again after this delay.
pub const RETRY_DELAY_MS: u32 = 2_000;
/// A late joiner may announce the plugin a moment after joining; one resend follows this delay.
pub const RESEND_DELAY_MS: u32 = 3_000;
/// Other users' polls kept in memory; older ones keep their last card but stop updating.
const MAX_TRACKED: usize = 16;
/// Ballots counted per poll, so a flood of fake sessions cannot grow memory without bound.
const MAX_BALLOTS: usize = 1_000;
/// Messages waiting for send budget; the oldest are dropped beyond this.
const MAX_OUTBOX: usize = 32;
/// Who a message goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum To {
    Channel,
    Direct(u32),
}

/// Everything the logic needs from Mumcord. `lib.rs` implements it with the real host calls;
/// tests use a recording fake.
pub trait Host {
    /// Unix seconds.
    fn now(&mut self) -> u64;
    fn send(&mut self, to: To, kind: u8, payload: &[u8]) -> Result<(), Error>;
    fn chat_send(&mut self, text: &str) -> Result<(), Error>;
    fn line_hash(&mut self, text: &str) -> u32;
    fn supersede(&mut self, card: &str, sender: u32, hash: u32);
    fn notify(&mut self, title: &str, body: &str);
    /// A one-shot timer; `None` if the host refused it.
    fn start_timer(&mut self, ms: u32) -> Option<u32>;
    fn show(&mut self, slot: Slot, tree: El);
    /// The user's own display name, if the host can tell.
    fn my_name(&mut self) -> Option<String>;
    /// The user's own session, if the host can tell.
    fn my_session(&mut self) -> Option<u32>;
    /// Opens the panel popover; only right after a click in the plugin's UI.
    fn open_popover(&mut self) -> Result<(), Error>;
    fn close_popover(&mut self);
    fn warn(&mut self, message: &str);
}

/// A poll as one client sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Poll {
    pub id: u32,
    pub question: String,
    pub options: Vec<String>,
    pub counts: Vec<u16>,
    pub closed: bool,
    /// The creator left; the poll stays as it was and takes no more votes.
    pub frozen: bool,
    /// The option this user chose.
    pub my_vote: Option<usize>,
    pub voters: Vec<VoterEntry>,
    pub creator: String,
}

impl Poll {
    pub fn new(id: u32, question: String, options: Vec<String>, creator: String) -> Self {
        let counts = vec![0; options.len()];
        Self {
            id,
            question,
            options,
            counts,
            closed: false,
            frozen: false,
            my_vote: None,
            voters: Vec::new(),
            creator,
        }
    }

    pub fn total(&self) -> u32 {
        self.counts.iter().map(|&c| u32::from(c)).sum()
    }
}

/// Why the creation form was refused. Kept as a code, not text, so a language change can
/// translate it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftError {
    PollOpen,
    Incomplete,
}

/// What the user typed into the creation form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub question: String,
    pub options: Vec<String>,
    pub error: Option<DraftError>,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            question: String::new(),
            options: vec![String::new(); MIN_OPTIONS],
            error: None,
        }
    }
}

/// Who cast a ballot: votes count per `peer.key`, or per session for users without one.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Voter {
    Me,
    Key(String),
    Session(u32),
}

#[derive(Debug, Clone)]
struct Ballot {
    voter: Voter,
    option: u8,
    name: String,
}

/// The user's own poll, with the ballots only the creator knows.
struct Own {
    poll: Poll,
    ballots: Vec<Ballot>,
    tally_due: bool,
}

impl Own {
    fn recount(&mut self) {
        let mut counts = vec![0u16; self.poll.options.len()];
        for ballot in &self.ballots {
            if let Some(c) = counts.get_mut(usize::from(ballot.option)) {
                *c = c.saturating_add(1);
            }
        }
        self.poll.counts = counts;
        self.poll.voters = self
            .ballots
            .iter()
            .map(|b| VoterEntry {
                option: b.option,
                name: b.name.clone(),
            })
            .collect();
    }

    fn tally(&self, close_hash: u32) -> Tally {
        Tally {
            poll_id: self.poll.id,
            closed: self.poll.closed,
            close_hash,
            counts: self.poll.counts.clone(),
        }
    }

    fn voter_parts(&self) -> Vec<Voters> {
        let entries: Vec<VoterEntry> = self.poll.voters.iter().take(MAX_VOTERS).cloned().collect();
        Voters::split(self.poll.id, &entries)
    }
}

/// Another user's poll, identified by the creator's session.
struct Other {
    session: u32,
    poll: Poll,
}

struct Outgoing {
    to: To,
    kind: u8,
    payload: Vec<u8>,
    /// For a late-joiner resend: whether one more try follows if nobody receives it yet.
    retry: bool,
}

/// The embedded translations; English until the host reports the language.
pub struct Texts(Strings);

impl Default for Texts {
    fn default() -> Self {
        Self(Strings::new(mumcord_plugin_sdk::i18n_all!()))
    }
}

#[derive(Default)]
pub struct App {
    strings: Texts,
    draft: Draft,
    own: Option<Own>,
    others: Vec<Other>,
    outbox: VecDeque<Outgoing>,
    flush_timer: Option<u32>,
    tally_timer: Option<u32>,
    /// (timer id, session) of pending late-joiner resends.
    resend_timers: Vec<(u32, u32)>,
    granted: Vec<String>,
    last_poll_id: u32,
    /// The creation form is open; it only shows in the expanded panel.
    creating: bool,
    /// The panel shows as the large popover rather than inline.
    expanded: bool,
}

/// The poll a vote is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Own,
    Other(u32),
}

pub fn own_card_id(poll_id: u32) -> String {
    format!("own-{poll_id}")
}

pub fn other_card_id(session: u32, poll_id: u32) -> String {
    format!("p-{session}-{poll_id}")
}

impl App {
    pub fn start(&mut self, host: &mut impl Host) {
        self.render_panel(host);
    }

    /// Switches the language and shows everything again. Returns false for other events.
    pub fn on_event(&mut self, host: &mut impl Host, event: &Event) -> bool {
        if !self.strings.0.on_event(event) {
            return false;
        }
        self.render_own(host);
        let sessions: Vec<u32> = self.others.iter().map(|o| o.session).collect();
        for session in sessions {
            self.render_other(host, session);
        }
        true
    }

    pub fn set_granted(&mut self, host: &mut impl Host, granted: Vec<String>) {
        self.granted = granted;
        self.render_panel(host);
    }

    fn granted(&self, permission: &str) -> bool {
        self.granted.iter().any(|p| p == permission)
    }

    /// The panel was hidden, shown inline or expanded. Leaving the popover leaves the form, so
    /// the small inline panel never shows it; the draft is kept.
    pub fn set_expanded(&mut self, host: &mut impl Host, expanded: bool) {
        self.expanded = expanded;
        if !expanded {
            self.creating = false;
        }
        self.render_panel(host);
    }

    /// A `ui-action`: `card` is the chat card it came from, or `None` for the panel.
    pub fn on_action(
        &mut self,
        host: &mut impl Host,
        card: Option<&str>,
        action: &str,
        text: Option<String>,
    ) {
        match (card, action) {
            (None, "question") => {
                self.draft.question = text.unwrap_or_default();
                self.render_panel(host);
            }
            (None, "add-option") => {
                if self.draft.options.len() < MAX_OPTIONS {
                    self.draft.options.push(String::new());
                }
                self.render_panel(host);
            }
            (None, "remove-option") => {
                if self.draft.options.len() > MIN_OPTIONS {
                    self.draft.options.pop();
                }
                self.render_panel(host);
            }
            (None, "create") => self.create(host),
            (None, "new-poll") => self.open_form(host),
            (None, "cancel") => {
                self.creating = false;
                self.leave_popover(host);
            }
            (None, "show-all") => {
                if self.open_popover(host) {
                    self.render_panel(host);
                }
            }
            (_, "close") => self.close(host),
            (None, other) => {
                if let Some(option) = parse_index(other, "vote-") {
                    if let Some(target) = self.current() {
                        self.vote(host, target, option);
                    }
                } else if let Some(slot) =
                    parse_index(other, "option-").and_then(|i| self.draft.options.get_mut(i))
                {
                    *slot = text.unwrap_or_default();
                    self.render_panel(host);
                }
            }
            (Some(card), other) => {
                if let Some(option) = parse_index(other, "vote-")
                    && let Some(target) = self.target_of_card(card)
                {
                    self.vote(host, target, option);
                }
            }
        }
    }

    /// "New poll": the form opens in the popover. While the user's own poll is open the button
    /// is disabled, so this is a stale click.
    fn open_form(&mut self, host: &mut impl Host) {
        if self.own_open() {
            return;
        }
        if self.open_popover(host) || self.expanded {
            self.creating = true;
            self.draft.error = None;
        }
        self.render_panel(host);
    }

    fn open_popover(&mut self, host: &mut impl Host) -> bool {
        match host.open_popover() {
            Ok(()) => {
                self.expanded = true;
                true
            }
            Err(e) => {
                host.warn(&format!("could not open the panel popover: {e:?}"));
                false
            }
        }
    }

    fn leave_popover(&mut self, host: &mut impl Host) {
        host.close_popover();
        self.expanded = false;
        self.render_panel(host);
    }

    fn own_open(&self) -> bool {
        self.own.as_ref().is_some_and(|o| !o.poll.closed)
    }

    /// The poll the panel shows: the user's own open poll, else the newest open poll of
    /// someone still in the channel.
    fn current(&self) -> Option<Target> {
        if self.own_open() {
            return Some(Target::Own);
        }
        self.others
            .iter()
            .rev()
            .find(|o| !o.poll.closed && !o.poll.frozen)
            .map(|o| Target::Other(o.session))
    }

    fn target_of_card(&self, card: &str) -> Option<Target> {
        if self
            .own
            .as_ref()
            .is_some_and(|o| own_card_id(o.poll.id) == card)
        {
            return Some(Target::Own);
        }
        self.others
            .iter()
            .find(|o| other_card_id(o.session, o.poll.id) == card)
            .map(|o| Target::Other(o.session))
    }

    fn create(&mut self, host: &mut impl Host) {
        if self.own_open() {
            self.draft.error = Some(DraftError::PollOpen);
            self.render_panel(host);
            return;
        }
        let question = self.draft.question.trim().to_owned();
        let options: Vec<String> = self
            .draft
            .options
            .iter()
            .map(|o| o.trim().to_owned())
            .collect();
        if !wire::valid_poll(&question, &options) {
            self.draft.error = Some(DraftError::Incomplete);
            self.render_panel(host);
            return;
        }
        let now = u32::try_from(host.now() & u64::from(u32::MAX)).unwrap_or(0);
        let id = now.max(self.last_poll_id.wrapping_add(1));
        self.last_poll_id = id;

        // The click is the gesture that allows exactly one chat line (FR-6.44).
        let triggers = self.granted("chat.triggers");
        let line = view::creation_line(&self.strings.0, &question, &options, triggers);
        let line_hash = match host.chat_send(&line) {
            Ok(()) => host.line_hash(&line),
            Err(e) => {
                host.warn(&format!("could not post the poll line: {e:?}"));
                0
            }
        };
        let create = Create {
            poll_id: id,
            line_hash,
            question: question.clone(),
            options: options.clone(),
        };
        self.queue(To::Channel, KIND_CREATE, create.encode(), false);
        let creator = host
            .my_name()
            .unwrap_or_else(|| self.strings.0.t("creator_fallback"));
        self.own = Some(Own {
            poll: Poll::new(id, question, options, creator),
            ballots: Vec::new(),
            tally_due: false,
        });
        self.draft = Draft::default();
        self.creating = false;
        self.flush(host);
        if self.expanded {
            host.close_popover();
            self.expanded = false;
        }
        self.render_own(host);
    }

    fn close(&mut self, host: &mut impl Host) {
        let Some(own) = self.own.as_mut().filter(|o| !o.poll.closed) else {
            return;
        };
        own.poll.closed = true;
        own.tally_due = false;
        let line = view::close_line(&self.strings.0, &own.poll);
        let close_hash = match host.chat_send(&line) {
            Ok(()) => host.line_hash(&line),
            Err(e) => {
                host.warn(&format!("could not post the closing line: {e:?}"));
                0
            }
        };
        let tally = own.tally(close_hash).encode();
        let parts: Vec<Vec<u8>> = own.voter_parts().iter().map(Voters::encode).collect();
        let poll_id = own.poll.id;
        // The final tally replaces any running one.
        self.outbox.retain(|m| {
            !(m.kind == KIND_TALLY && m.to == To::Channel && poll_of(m) == Some(poll_id))
        });
        self.queue(To::Channel, KIND_TALLY, tally, false);
        for part in parts {
            self.queue(To::Channel, KIND_VOTERS, part, false);
        }
        self.flush(host);
        self.render_own(host);
    }

    fn vote(&mut self, host: &mut impl Host, target: Target, option: usize) {
        let Target::Other(session) = target else {
            let Some(own) = self.own.as_mut() else {
                return;
            };
            if own.poll.closed || option >= own.poll.options.len() {
                return;
            }
            let name = host
                .my_name()
                .unwrap_or_else(|| self.strings.0.t("creator_fallback"));
            own.poll.my_vote = Some(option);
            record_ballot(own, Voter::Me, option, name);
            self.tally_changed(host);
            self.render_own(host);
            return;
        };
        let Some(other) = self.others.iter_mut().find(|o| o.session == session) else {
            return;
        };
        let poll = &mut other.poll;
        if poll.closed || poll.frozen || option >= poll.options.len() {
            return;
        }
        poll.my_vote = Some(option);
        let vote = Vote {
            poll_id: poll.id,
            option: u8::try_from(option).unwrap_or(u8::MAX),
        };
        self.queue(To::Direct(session), KIND_VOTE, vote.encode(), false);
        self.flush(host);
        self.render_other(host, session);
    }

    /// A message from the same plugin of another user. Payloads are untrusted.
    pub fn on_peer_message(
        &mut self,
        host: &mut impl Host,
        sender: &Peer,
        kind: u8,
        payload: &[u8],
        direct: bool,
    ) {
        match kind {
            KIND_CREATE => {
                if let Some(create) = Create::decode(payload) {
                    self.received_create(host, sender, create, direct);
                }
            }
            KIND_VOTE if direct => {
                if let Some(vote) = Vote::decode(payload) {
                    self.received_vote(host, sender, vote);
                }
            }
            KIND_TALLY => {
                if let Some(tally) = Tally::decode(payload) {
                    self.received_tally(host, sender.session, tally);
                }
            }
            KIND_VOTERS => {
                if let Some(voters) = Voters::decode(payload) {
                    self.received_voters(host, sender.session, voters);
                }
            }
            _ => {}
        }
    }

    fn received_create(
        &mut self,
        host: &mut impl Host,
        sender: &Peer,
        create: Create,
        direct: bool,
    ) {
        // A repeated resend of a poll we already have changes nothing.
        if self
            .others
            .iter()
            .any(|o| o.session == sender.session && o.poll.id == create.poll_id)
        {
            return;
        }
        // One poll per creator: an older one from the same session ends here.
        if let Some(old) = self.others.iter_mut().find(|o| o.session == sender.session) {
            old.poll.closed = true;
            let card = view::card(&self.strings.0, &old.poll, Role::Voter);
            host.show(
                Slot::ChatCard(other_card_id(old.session, old.poll.id)),
                card,
            );
        }
        self.others.retain(|o| o.session != sender.session);
        if self.others.len() >= MAX_TRACKED {
            self.others.remove(0);
        }
        let card = other_card_id(sender.session, create.poll_id);
        let poll = Poll::new(
            create.poll_id,
            create.question,
            create.options,
            sender.name.clone(),
        );
        self.others.push(Other {
            session: sender.session,
            poll,
        });
        self.render_other(host, sender.session);
        if create.line_hash != 0 {
            host.supersede(&card, sender.session, create.line_hash);
        }
        if !direct && self.granted.iter().any(|p| p == "notify") {
            let question = self
                .others
                .last()
                .map(|o| o.poll.question.clone())
                .unwrap_or_default();
            let s = &self.strings.0;
            let body = s.t_with(
                "notify.body",
                &[("name", &sender.name), ("question", &question)],
            );
            host.notify(&s.t("notify.title"), wire::truncate(&body, 200));
        }
    }

    fn received_vote(&mut self, host: &mut impl Host, sender: &Peer, vote: Vote) {
        let Some(own) = self.own.as_mut() else {
            return;
        };
        if own.poll.closed
            || own.poll.id != vote.poll_id
            || usize::from(vote.option) >= own.poll.options.len()
        {
            return;
        }
        let voter = match &sender.key {
            Some(key) => Voter::Key(key.clone()),
            None => Voter::Session(sender.session),
        };
        record_ballot(own, voter, usize::from(vote.option), sender.name.clone());
        self.tally_changed(host);
        self.render_own(host);
    }

    /// `chat.triggers`: someone in the channel wrote "!poll <n>". Only the creator counts it,
    /// like a vote from the plugin: one per person, and a later one replaces it. The arguments
    /// are untrusted text.
    pub fn on_chat_trigger(&mut self, host: &mut impl Host, sender: &Peer, args: &str) {
        let Some(own) = self.own.as_ref().filter(|o| !o.poll.closed) else {
            return;
        };
        let Some(option) = parse_choice(args, own.poll.options.len()) else {
            return;
        };
        let voter = if host.my_session() == Some(sender.session) {
            Voter::Me
        } else {
            match &sender.key {
                Some(key) => Voter::Key(key.clone()),
                None => Voter::Session(sender.session),
            }
        };
        let Some(own) = self.own.as_mut() else {
            return;
        };
        if voter == Voter::Me {
            own.poll.my_vote = Some(option);
        }
        record_ballot(own, voter, option, sender.name.clone());
        self.tally_changed(host);
        self.render_own(host);
    }

    fn received_tally(&mut self, host: &mut impl Host, sender: u32, tally: Tally) {
        let Some(other) = self
            .others
            .iter_mut()
            .find(|o| o.session == sender && o.poll.id == tally.poll_id)
        else {
            return;
        };
        if tally.counts.len() != other.poll.options.len() || other.poll.closed {
            return;
        }
        other.poll.counts = tally.counts;
        if tally.closed {
            other.poll.closed = true;
            if tally.close_hash != 0 {
                host.supersede(
                    &other_card_id(sender, other.poll.id),
                    sender,
                    tally.close_hash,
                );
            }
        }
        self.render_other(host, sender);
    }

    fn received_voters(&mut self, host: &mut impl Host, sender: u32, voters: Voters) {
        let Some(other) = self
            .others
            .iter_mut()
            .find(|o| o.session == sender && o.poll.id == voters.poll_id)
        else {
            return;
        };
        if voters.part == 0 {
            other.poll.voters.clear();
        }
        let options = other.poll.options.len();
        let room = MAX_VOTERS.saturating_sub(other.poll.voters.len());
        other.poll.voters.extend(
            voters
                .entries
                .into_iter()
                .filter(|e| usize::from(e.option) < options)
                .take(room),
        );
        self.render_other(host, sender);
    }

    /// `events.presence`: someone joined the channel. A late joiner gets the open poll.
    pub fn on_joined(&mut self, host: &mut impl Host, peer: &Peer) {
        self.resend(host, peer.session, true);
    }

    /// `events.presence`: someone left. If it was a creator, their poll freezes.
    pub fn on_left(&mut self, host: &mut impl Host, peer: &Peer) {
        self.resend_timers.retain(|&(_, s)| s != peer.session);
        self.outbox.retain(|m| m.to != To::Direct(peer.session));
        let Some(other) = self.others.iter_mut().find(|o| o.session == peer.session) else {
            return;
        };
        if !other.poll.closed && !other.poll.frozen {
            other.poll.frozen = true;
            self.render_other(host, peer.session);
        }
    }

    pub fn on_timer(&mut self, host: &mut impl Host, id: u32) {
        if self.flush_timer == Some(id) {
            self.flush_timer = None;
            self.flush(host);
        } else if self.tally_timer == Some(id) {
            self.tally_timer = None;
            if let Some(own) = self.own.as_mut().filter(|o| o.tally_due && !o.poll.closed) {
                own.tally_due = false;
                let poll_id = own.poll.id;
                let tally = own.tally(0).encode();
                self.outbox.retain(|m| {
                    !(m.kind == KIND_TALLY && m.to == To::Channel && poll_of(m) == Some(poll_id))
                });
                self.queue(To::Channel, KIND_TALLY, tally, false);
                self.flush(host);
            }
        } else if let Some(pos) = self.resend_timers.iter().position(|&(t, _)| t == id) {
            let (_, session) = self.resend_timers.remove(pos);
            self.resend(host, session, false);
        }
    }

    fn resend(&mut self, host: &mut impl Host, session: u32, retry: bool) {
        let Some(own) = self.own.as_ref().filter(|o| !o.poll.closed) else {
            return;
        };
        let create = Create {
            poll_id: own.poll.id,
            // The fallback line is long gone for a late joiner; nothing to supersede.
            line_hash: 0,
            question: own.poll.question.clone(),
            options: own.poll.options.clone(),
        };
        let tally = own.tally(0).encode();
        let parts: Vec<Vec<u8>> = own.voter_parts().iter().map(Voters::encode).collect();
        let to = To::Direct(session);
        self.queue(to, KIND_CREATE, create.encode(), retry);
        self.queue(to, KIND_TALLY, tally, retry);
        for part in parts {
            self.queue(to, KIND_VOTERS, part, retry);
        }
        self.flush(host);
    }

    fn tally_changed(&mut self, host: &mut impl Host) {
        if let Some(own) = self.own.as_mut() {
            own.tally_due = true;
        }
        if self.tally_timer.is_none() {
            self.tally_timer = host.start_timer(TALLY_DELAY_MS);
        }
    }

    fn queue(&mut self, to: To, kind: u8, payload: Vec<u8>, retry: bool) {
        if self.outbox.len() >= MAX_OUTBOX {
            self.outbox.pop_front();
        }
        self.outbox.push_back(Outgoing {
            to,
            kind,
            payload,
            retry,
        });
    }

    /// Sends queued messages in order until the host runs out of budget.
    fn flush(&mut self, host: &mut impl Host) {
        while let Some(message) = self.outbox.front() {
            match host.send(message.to, message.kind, &message.payload) {
                Ok(()) => {
                    self.outbox.pop_front();
                }
                Err(Error::RateLimited) => {
                    if self.flush_timer.is_none() {
                        self.flush_timer = host.start_timer(RETRY_DELAY_MS);
                    }
                    return;
                }
                Err(Error::NoRecipients) => {
                    let to = message.to;
                    let retry = message.retry;
                    if let To::Direct(session) = to {
                        // Everything else queued for this person would fail the same way.
                        self.outbox.retain(|m| m.to != to);
                        if retry && let Some(timer) = host.start_timer(RESEND_DELAY_MS) {
                            self.resend_timers.push((timer, session));
                        }
                    } else {
                        // Nobody else in the channel runs Poll: a normal case, not an error.
                        self.outbox.pop_front();
                    }
                }
                Err(e) => {
                    host.warn(&format!("could not send a poll message: {e:?}"));
                    self.outbox.pop_front();
                }
            }
        }
    }

    fn render_panel(&self, host: &mut impl Host) {
        let current = match self.current() {
            Some(Target::Own) => self.own.as_ref().map(|o| (&o.poll, Role::Creator)),
            Some(Target::Other(session)) => self
                .others
                .iter()
                .find(|o| o.session == session)
                .map(|o| (&o.poll, Role::Voter)),
            None => None,
        };
        let panel = view::Panel {
            draft: &self.draft,
            creating: self.creating && self.expanded,
            expanded: self.expanded,
            current,
            own_open: self.own_open(),
            triggers: self.granted("chat.triggers"),
        };
        host.show(Slot::Panel, view::panel(&self.strings.0, &panel));
    }

    /// The card, then the panel, which shows the current poll live. Also shows the panel
    /// when there is no own poll.
    fn render_own(&self, host: &mut impl Host) {
        if let Some(own) = &self.own {
            host.show(
                Slot::ChatCard(own_card_id(own.poll.id)),
                view::card(&self.strings.0, &own.poll, Role::Creator),
            );
        }
        self.render_panel(host);
    }

    fn render_other(&self, host: &mut impl Host, session: u32) {
        if let Some(other) = self.others.iter().find(|o| o.session == session) {
            host.show(
                Slot::ChatCard(other_card_id(session, other.poll.id)),
                view::card(&self.strings.0, &other.poll, Role::Voter),
            );
        }
        self.render_panel(host);
    }
}

fn record_ballot(own: &mut Own, voter: Voter, option: usize, name: String) {
    let option = u8::try_from(option).unwrap_or(u8::MAX);
    if let Some(ballot) = own.ballots.iter_mut().find(|b| b.voter == voter) {
        ballot.option = option;
        ballot.name = name;
    } else if own.ballots.len() < MAX_BALLOTS {
        own.ballots.push(Ballot {
            voter,
            option,
            name,
        });
    }
    own.recount();
}

/// "2", "2.", "#2" or "2 pizza" → option index 1, if the poll has that option.
fn parse_choice(args: &str, options: usize) -> Option<usize> {
    let first = args.split_whitespace().next()?;
    let digits = first.trim_start_matches('#').trim_end_matches(['.', ')']);
    let number: usize = digits.parse().ok()?;
    (1..=options).contains(&number).then(|| number - 1)
}

fn parse_index(action: &str, prefix: &str) -> Option<usize> {
    action.strip_prefix(prefix)?.parse().ok()
}

/// The poll id at the start of every payload.
fn poll_of(message: &Outgoing) -> Option<u32> {
    message
        .payload
        .get(..4)?
        .try_into()
        .ok()
        .map(u32::from_be_bytes)
}
