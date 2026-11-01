//! Echolings: a little creature made of sound that grows while you hang out with friends.
//!
//! This file only wires host events to the pet logic (`pet`), other people's pets (`social`),
//! the mini-games (`games`), the views (`ui`) and plugin storage (`storage`).

pub mod games;
pub mod pet;
pub mod social;
pub mod storage;
pub mod ui;

use std::cell::RefCell;

use mumcord_plugin_sdk::{
    Plugin, clock,
    events::{
        ActionValue, Event, PanelVisibility, PeerMessage, PluginSettings, SelfState, UiAction,
        Visibility,
    },
    log, notify,
    peers::{self, Target},
    theme, timers,
    types::{Error, StartInfo},
    ui::{self as host_ui, El, Slot},
};

use games::echo_echo::{self, Game, GameView, Invite, Score, Start};
use pet::{Now, Pet, Reaction, Refusal, Session, Species, clean_name};
use social::{Happening, OtherView, PatNote, Social, card::Card, display_name, throttle::Decision};
use storage::{AnimationLevel, Settings};
use ui::{Draft, Motion, View, action, i18n::I18n};

/// The care loop advances once a minute; nothing else polls, so the plugin idles cheaply.
const TICK_MS: u32 = 60_000;
/// How long a care reaction plays before the pet goes back to its resting animation.
const REACTION_MS: u32 = 3_000;
/// A game message the host refused for its rate limit is tried again after this.
const RETRY_MS: u32 = 2_500;

struct App {
    pet: Option<Pet>,
    session: Session,
    channel: Option<u32>,
    settings: Settings,
    t: I18n,
    reduced_motion: bool,
    panel: PanelVisibility,
    draft: Draft,
    need_name: bool,
    reaction: Option<Reaction>,
    reaction_timer: Option<u32>,
    tick_timer: Option<u32>,
    refusal: Option<Refusal>,
    /// The session whose member badge currently shows the pet.
    badge: Option<u32>,
    social: Social,
    /// Permissions the user granted, from `settings-changed`.
    granted: Vec<String>,
    visibility: Visibility,
    /// Wakes the card throttle when a send had to wait.
    card_timer: Option<u32>,
    game: Option<Game>,
    step_timer: Option<u32>,
    end_timer: Option<u32>,
    /// Closes the lobby of a game the user hosts.
    lobby_timer: Option<u32>,
    /// One game message waiting for the rate limit.
    retry: Option<(Target, u8, Vec<u8>)>,
    retry_timer: Option<u32>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn now() -> Now {
    let now = clock::now();
    Now {
        secs: now.unix_seconds,
        utc_offset_minutes: now.utc_offset_minutes,
    }
}

/// Shows a tree; a revoked permission is expected and not worth a log line.
fn show(slot: Slot, tree: El) {
    match host_ui::show(slot, tree) {
        Ok(()) | Err(Error::NotPermitted) => {}
        Err(e) => log::warn(&format!("could not show the pet: {e:?}")),
    }
}

impl App {
    fn view(&self, now: Now) -> Option<View<'_>> {
        Some(View {
            pet: self.pet.as_ref()?,
            session: &self.session,
            now,
            t: &self.t,
            motion: self.motion(),
            reaction: self.reaction,
            refusal: self.refusal,
        })
    }

    fn motion(&self) -> Motion {
        Motion {
            level: self.settings.animation,
            reduced: self.reduced_motion,
        }
    }

    fn render(&mut self, now: Now) {
        self.render_panel(now);
        self.render_badge(now);
        self.share_card(now);
    }

    fn granted(&self, permission: &str) -> bool {
        self.granted.iter().any(|g| g == permission)
    }

    /// The game is a chat card when allowed and connected, else it takes over the panel.
    fn game_in_chat(&self) -> bool {
        self.granted("ui.chat-card") && self.session.me.is_some()
    }

    fn render_game(&self) {
        let Some(game) = &self.game else { return };
        let tree = GameView {
            game,
            t: &self.t,
            motion: self.motion(),
        }
        .card();
        if self.game_in_chat() {
            show(Slot::ChatCard(game.card_id()), tree);
        } else if self.panel != PanelVisibility::Hidden {
            show(Slot::Panel, tree);
        }
    }

    fn render_panel(&self, now: Now) {
        if self.game.is_some() && !self.game_in_chat() {
            return self.render_game();
        }
        let tree = match (self.panel, self.view(now)) {
            (PanelVisibility::Hidden, _) => return,
            (PanelVisibility::Inline, Some(view)) => view.panel_inline(),
            (PanelVisibility::Expanded, Some(view)) => view.panel_expanded(),
            (PanelVisibility::Inline, None) => ui::hatch_prompt(&self.t),
            (PanelVisibility::Expanded, None) => {
                ui::picker(&self.t, &self.draft, self.need_name, self.motion())
            }
        };
        show(Slot::Panel, tree);
    }

    fn render_badge(&mut self, now: Now) {
        let target = self.pet.as_ref().and(self.session.me);
        if let Some(old) = self.badge.filter(|old| Some(*old) != target) {
            host_ui::clear(&Slot::MemberBadge(old));
        }
        self.badge = target;
        if let (Some(me), Some(view)) = (target, self.view(now)) {
            show(Slot::MemberBadge(me), view.badge());
        }
    }

    fn render_settings(&self) {
        show(
            Slot::Settings,
            ui::settings(&self.t, self.settings.animation),
        );
    }

    fn save(&self) {
        if let Some(pet) = &self.pet {
            storage::save_pet(pet);
        }
    }

    fn react(&mut self, reaction: Option<Reaction>) {
        if let Some(id) = self.reaction_timer.take() {
            timers::cancel(id);
        }
        self.reaction = reaction;
        if reaction.is_some() {
            match timers::start(REACTION_MS, false) {
                Ok(id) => self.reaction_timer = Some(id),
                Err(e) => log::warn(&format!("could not time the reaction: {e:?}")),
            }
        }
    }

    fn self_changed(&mut self, state: SelfState, now: Now) {
        let me = state.me.map(|p| p.session);
        let channel = state.channel.map(|c| c.id);
        // The host re-sends `joined` for everyone in a channel the user moves into.
        if me != self.session.me || channel != self.channel {
            self.session.others.clear();
            self.session.joined_at = None;
            storage::forget_saved();
            for session in self.social.moved(now.secs) {
                host_ui::clear(&Slot::MemberBadge(session));
            }
            self.drop_game();
        }
        self.session.me = me;
        self.channel = channel;
        self.session.deafened = state.deafened;
        self.session.do_not_disturb = state.do_not_disturb;
        if state.do_not_disturb {
            self.react(None);
        }
        self.render(now);
    }

    fn own_talking(&mut self, talking: bool, now: Now) {
        if self.session.talking == talking {
            return;
        }
        match &mut self.pet {
            Some(pet) => {
                pet.on_own_talking(&mut self.session, talking, now);
            }
            None => self.session.talking = talking,
        }
        self.render(now);
    }

    fn tick(&mut self, now: Now) {
        if let Some(pet) = &mut self.pet {
            pet.tick(&mut self.session, now);
            storage::save_pet(pet);
        }
        self.render(now);
    }

    fn ui_action(&mut self, action: UiAction, now: Now) {
        let text = match &action.value {
            ActionValue::Text(text) => Some(text.as_str()),
            _ => None,
        };
        self.refusal = None;
        match (action.action.as_str(), &mut self.pet) {
            (action::PAT, Some(pet)) => {
                let reaction = pet.pat(&self.session, now);
                self.react(reaction);
            }
            (action::TREAT, Some(pet)) => match pet.treat(&self.session, now) {
                Ok(reaction) => self.react(reaction),
                Err(refusal) => self.refusal = Some(refusal),
            },
            (action::PLAY, Some(_)) => self.start_game(now),
            (echo_echo::action::JOIN, _) => self.join_game(now),
            (echo_echo::action::CLOSE, _) => self.drop_game(),
            (echo_echo::action::START_NOW, _) => self.begin_game(now),
            (other, _) if social::action::parse_pat(other).is_some() => {
                if let Some(session) = social::action::parse_pat(other) {
                    self.pat_other(session, now);
                }
            }
            (other, _) if echo_echo::action::parse_pad(other).is_some() => {
                if let Some(pad) = echo_echo::action::parse_pad(other) {
                    self.press_pad(pad, now);
                }
            }
            (action::NAP, Some(pet)) => match pet.nap(&self.session, now) {
                Ok(reaction) => self.react(Some(reaction)),
                Err(refusal) => self.refusal = Some(refusal),
            },
            (action::RENAME, Some(pet)) => {
                if let Some(name) = text.and_then(clean_name) {
                    pet.name = name;
                }
            }
            (action::COLOUR, Some(pet)) => {
                if let Some(colour) = text.and_then(|t| t.parse().ok()) {
                    pet.set_colour(colour);
                }
            }
            (action::PICK_SPECIES, None) => {
                if let Some(species) = text.and_then(Species::from_id) {
                    self.draft.species = species;
                }
            }
            (action::PICK_COLOUR, None) => {
                if let Some(colour) = text.and_then(|t| t.parse::<u8>().ok()) {
                    self.draft.colour = colour.min(1);
                }
            }
            (action::PICK_NAME, None) => {
                if let Some(name) = text {
                    self.draft.name = clean_name(name).unwrap_or_default();
                    self.need_name = false;
                }
            }
            (action::HATCH, None) => match clean_name(&self.draft.name) {
                Some(name) => {
                    self.pet = Some(Pet::hatch(name, self.draft.species, self.draft.colour, now));
                    self.need_name = false;
                }
                None => self.need_name = true,
            },
            (action::ANIMATION, _) => {
                if let Some(level) = text.and_then(AnimationLevel::from_id) {
                    self.settings.animation = level;
                    storage::save_settings(&self.settings);
                    self.render_settings();
                }
            }
            // Stale clicks from a tree that was just replaced.
            _ => return self.render(now),
        }
        self.save();
        self.render(now);
        // The panel shows the game itself when it is not a chat card.
        if self.game_in_chat() {
            self.render_game();
        }
    }

    // --- Other people's pets -------------------------------------------------------------

    /// Sends the pet card to the channel when the throttle allows it.
    fn share_card(&mut self, now: Now) {
        if self.card_timer.is_some() || !self.can_message() {
            return;
        }
        let (Some(pet), Some(_)) = (&self.pet, self.channel) else {
            return;
        };
        let card = Card::of(pet, &self.session, now).encode();
        match self.social.throttle.decide(&card, now.secs) {
            Decision::Idle => {}
            Decision::Send => {
                let result = peers::send(Target::Channel, social::CARD, &card);
                match result {
                    Ok(()) | Err(Error::NoRecipients | Error::Hidden | Error::NotPermitted) => {}
                    Err(e) => log::warn(&format!("could not send the pet card: {e:?}")),
                }
                // Nobody to send to also counts as delivered: whoever joins invalidates it.
                let delivered = matches!(result, Ok(()) | Err(Error::NoRecipients));
                self.social.throttle.attempted(&card, delivered, now.secs);
            }
            Decision::Wait(secs) => {
                let ms = u32::try_from(secs * 1000).unwrap_or(u32::MAX);
                self.card_timer = start_timer(ms, "card");
            }
        }
    }

    fn can_message(&self) -> bool {
        self.granted("peers.messages") && self.visibility != Visibility::Nobody
    }

    fn other_view<'a>(&'a self, pet: &'a social::OtherPet) -> OtherView<'a> {
        OtherView {
            pet,
            t: &self.t,
            motion: self.motion(),
        }
    }

    fn render_other_badge(&self, session: u32) {
        if let Some(pet) = self.social.pets.get(&session) {
            show(Slot::MemberBadge(session), self.other_view(pet).badge());
        }
    }

    fn render_other_card(&self, session: u32, now: Now) {
        let Some(pet) = self.social.pets.get(&session) else {
            return;
        };
        let note = self
            .social
            .pat_note
            .filter(|(s, _)| *s == session)
            .map(|(_, note)| note);
        let patted = !self.social.can_pat(session, now.day());
        let tree = self
            .other_view(pet)
            .profile_card(session, patted, self.can_message(), note);
        show(Slot::ProfileCard(session), tree);
    }

    fn react_to(&mut self, happening: Happening, now: Now) {
        let awake = self
            .pet
            .as_ref()
            .is_some_and(|pet| !pet.asleep(&self.session, now));
        if let Some(reaction) = social::reaction(happening, now.secs, self.social.quiet_until)
            && awake
        {
            self.react(Some(reaction));
        }
    }

    fn pat_other(&mut self, session: u32, now: Now) {
        let note = if !self.social.can_pat(session, now.day()) {
            PatNote::AlreadyToday
        } else {
            match peers::send(Target::Direct(session), social::PAT, &social::PAT_PAYLOAD) {
                Ok(()) => {
                    self.social.give_pat(session, now.day());
                    social::visits::save(&self.social.ledger);
                    if let Some(pet) = &mut self.pet {
                        pet.visit(now);
                    }
                    PatNote::Given
                }
                Err(e) => {
                    log::warn(&format!("could not send the pat: {e:?}"));
                    PatNote::Failed
                }
            }
        };
        self.social.pat_note = Some((session, note));
        self.render_other_card(session, now);
    }

    fn pat_received(&mut self, message: &PeerMessage, now: Now) {
        let sender = &message.sender;
        if !message.direct
            || !self.social.pat_received(
                sender.session,
                sender.key.as_deref(),
                &message.payload,
                now.day(),
            )
        {
            return;
        }
        social::visits::save(&self.social.ledger);
        let Some(pet) = &mut self.pet else { return };
        let reaction = pet.visited(&self.session, now);
        storage::save_pet(pet);
        let pet_name = pet.name.clone();
        self.react(reaction);
        if self.granted("notify") && !self.session.do_not_disturb {
            let visitor = display_name(&sender.name);
            let title = self.t.f("visit.notifyTitle", &[("pet", &pet_name)]);
            let body = self.t.f(
                "visit.notifyBody",
                &[("name", &visitor), ("pet", &pet_name)],
            );
            if let Err(e) = notify::send(&title, &body) {
                log::debug(&format!("visit notification not shown: {e:?}"));
            }
        }
    }

    fn peer_message(&mut self, message: PeerMessage, now: Now) {
        let session = message.sender.session;
        match message.kind {
            social::CARD => {
                let key = message.sender.key.clone();
                if self.social.card_received(session, key, &message.payload) {
                    self.render_other_badge(session);
                    if self.social.open_card == Some(session) {
                        self.render_other_card(session, now);
                    }
                }
            }
            social::PAT => self.pat_received(&message, now),
            echo_echo::INVITE => self.game_invited(&message, now),
            echo_echo::START => self.start_received(&message, now),
            echo_echo::SCORE => {
                let score = Score::decode(&message.payload);
                if let (Some(game), Some(score)) = (&mut self.game, score)
                    && game.score_received(session, display_name(&message.sender.name), score)
                {
                    self.render_game();
                }
            }
            _ => {}
        }
        self.render(now);
    }

    // --- Echo Echo ---------------------------------------------------------------------------

    /// Play starts a game; the pet must be awake and rested enough.
    fn start_game(&mut self, now: Now) {
        let Some(pet) = &mut self.pet else { return };
        if self.game.as_ref().is_some_and(|g| !g.is_over()) {
            return;
        }
        match pet.play(&self.session, now) {
            Ok(reaction) => self.react(Some(reaction)),
            Err(refusal) => {
                self.refusal = Some(refusal);
                return;
            }
        }
        let me = u64::from(self.session.me.unwrap_or(0));
        let seed = now.secs ^ (me << 32);
        let id = u32::try_from(seed & 0xffff_ffff).unwrap_or(0);
        self.drop_game();
        let game = Game::open(id, echo_echo::tune(seed), now.secs);
        let invite = Invite {
            id,
            tune: game.tune.clone(),
            lobby: u8::try_from(echo_echo::LOBBY_SECS).unwrap_or(u8::MAX),
        };
        self.game = Some(game);
        if self.can_message() && self.session.me.is_some() {
            self.send_game(Target::Channel, echo_echo::INVITE, invite.encode());
            self.lobby_timer = start_timer(echo_echo::LOBBY_MS, "game lobby");
        } else {
            // Nobody can join without messages, so there is no lobby to wait for.
            self.begin_game(now);
        }
    }

    /// The host closes the lobby (timer or Start now): the round starts for everyone who joined
    /// when the start message arrives. Nobody joined = the host plays alone.
    fn begin_game(&mut self, now: Now) {
        if let Some(id) = self.lobby_timer.take() {
            timers::cancel(id);
        }
        let Some(game) = &mut self.game else { return };
        if !game.is_host() || !game.begin(now.secs) {
            return;
        }
        let start = Start { id: game.id };
        if self.can_message() && self.session.me.is_some() {
            self.send_game(Target::Channel, echo_echo::START, start.encode());
        }
        self.round_started();
    }

    fn start_received(&mut self, message: &PeerMessage, now: Now) {
        let Some(start) = Start::decode(&message.payload) else {
            return;
        };
        if let Some(game) = &mut self.game
            && game.start_received(message.sender.session, start, now.secs)
        {
            self.round_started();
        }
    }

    /// Timers for a round that just started: the tune plays if the user is in, and the scores
    /// show [`echo_echo::GAME_SECS`] later.
    fn round_started(&mut self) {
        for id in [self.step_timer.take(), self.end_timer.take()]
            .into_iter()
            .flatten()
        {
            timers::cancel(id);
        }
        if self.game.as_ref().is_some_and(|g| g.playing().is_some()) {
            self.step_timer = start_timer_repeating(echo_echo::STEP_MS);
        }
        self.end_timer = start_timer(echo_echo::GAME_MS, "game end");
        self.render_game();
    }

    fn game_invited(&mut self, message: &PeerMessage, now: Now) {
        if self.game.as_ref().is_some_and(|g| !g.is_over()) {
            return;
        }
        let Some(invite) = Invite::decode(&message.payload) else {
            return;
        };
        self.drop_game();
        let host = display_name(&message.sender.name);
        let game = Game::invited(invite, message.sender.session, host, now.secs);
        let end_ms = game.ends_at.saturating_sub(now.secs).saturating_mul(1000);
        self.game = Some(game);
        self.end_timer = start_timer(u32::try_from(end_ms).unwrap_or(u32::MAX), "game end");
        if self.game_in_chat() {
            self.render_game();
        }
    }

    fn join_game(&mut self, now: Now) {
        let (Some(pet), Some(game)) = (&mut self.pet, &mut self.game) else {
            return;
        };
        if game.phase != echo_echo::Phase::Invited {
            return;
        }
        match pet.play(&self.session, now) {
            Ok(_) => {
                game.join();
                if game.playing().is_some() {
                    self.step_timer = start_timer_repeating(echo_echo::STEP_MS);
                }
                // A chat card is not part of the panel render, so show the new phase here.
                self.render_game();
            }
            Err(refusal) => self.refusal = Some(refusal),
        }
    }

    fn game_step(&mut self) {
        if let Some(game) = &mut self.game
            && !game.step()
            && let Some(id) = self.step_timer.take()
        {
            timers::cancel(id);
        }
        self.render_game();
    }

    fn press_pad(&mut self, pad: u8, now: Now) {
        let Some(game) = &mut self.game else { return };
        let Some(correct) = game.press(pad) else {
            return;
        };
        let score = Score {
            id: game.id,
            correct,
        };
        if let Some(pet) = &mut self.pet {
            pet.game_finished(now);
        }
        if self.can_message() {
            self.send_game(Target::Channel, echo_echo::SCORE, score.encode());
        }
    }

    fn end_game(&mut self) {
        self.end_timer = None;
        if let Some(id) = self.step_timer.take() {
            timers::cancel(id);
        }
        if let Some(game) = &mut self.game {
            game.end();
        }
        self.render_game();
    }

    /// Removes the current game's card and timers.
    fn drop_game(&mut self) {
        for id in [
            self.step_timer.take(),
            self.end_timer.take(),
            self.lobby_timer.take(),
            self.retry_timer.take(),
        ]
        .into_iter()
        .flatten()
        {
            timers::cancel(id);
        }
        self.retry = None;
        if let Some(game) = self.game.take()
            && self.game_in_chat()
        {
            host_ui::clear(&Slot::ChatCard(game.card_id()));
        }
    }

    /// Sends a game message; one refused for the rate limit is tried again shortly.
    fn send_game(&mut self, to: Target, kind: u8, payload: Vec<u8>) {
        match peers::send(to, kind, &payload) {
            Ok(()) | Err(Error::NoRecipients | Error::Hidden) => {}
            Err(Error::RateLimited) if self.retry.is_none() => {
                self.retry = Some((to, kind, payload));
                self.retry_timer = start_timer(RETRY_MS, "game retry");
            }
            Err(e) => log::warn(&format!("could not send a game message: {e:?}")),
        }
    }

    fn retry_send(&mut self) {
        self.retry_timer = None;
        if let Some((to, kind, payload)) = self.retry.take() {
            self.send_game(to, kind, payload);
        }
    }

    fn settings_changed(&mut self, settings: PluginSettings, now: Now) {
        self.granted = settings.granted;
        self.visibility = settings.visibility;
        if !self.can_message() {
            self.social.throttle.invalidate();
        }
        self.render(now);
    }

    fn on_event(&mut self, event: Event) {
        let now = now();
        let me = self.session.me;
        let is_me = |session: u32| me == Some(session);
        match event {
            Event::Talking((peer, talking)) if is_me(peer.session) => {
                self.own_talking(talking, now);
            }
            Event::Talking((peer, talking)) => {
                if self.social.talking(peer.session, talking) {
                    self.render_other_badge(peer.session);
                }
            }
            Event::Joined(peer) if !is_me(peer.session) => {
                self.session.joined(peer.session, now);
                self.social.joined();
                self.react_to(Happening::Joined, now);
                self.render(now);
            }
            Event::Left(peer) => {
                self.session.left(peer.session);
                if self.social.left(peer.session) {
                    host_ui::clear(&Slot::MemberBadge(peer.session));
                }
            }
            Event::ScreenShareStarted(peer) if !is_me(peer.session) => {
                self.react_to(Happening::ScreenShare, now);
                self.render(now);
            }
            Event::Mentioned(_) => {
                self.session.mentioned(now);
                self.react_to(Happening::Mentioned, now);
                self.render(now);
            }
            Event::PeerMessage(message) => self.peer_message(message, now),
            Event::SettingsChanged(settings) => self.settings_changed(settings, now),
            Event::ChatActivity((peer, _)) => {
                let mine = is_me(peer.session);
                if let Some(pet) = &mut self.pet {
                    pet.on_chat(&mut self.session, mine, now);
                }
            }
            Event::SelfChanged(state) => self.self_changed(state, now),
            Event::ThemeChanged(theme) => {
                self.reduced_motion = theme.reduced_motion;
                self.render(now);
            }
            Event::LanguageChanged(language) => {
                self.t.set_language(&language);
                self.render(now);
                self.render_settings();
            }
            Event::UiAction(action) => self.ui_action(action, now),
            Event::Timer(id) if Some(id) == self.tick_timer => self.tick(now),
            Event::Timer(id) if Some(id) == self.card_timer => {
                self.card_timer = None;
                self.share_card(now);
            }
            Event::Timer(id) if Some(id) == self.step_timer => self.game_step(),
            Event::Timer(id) if Some(id) == self.end_timer => self.end_game(),
            Event::Timer(id) if Some(id) == self.lobby_timer => {
                self.lobby_timer = None;
                self.begin_game(now);
            }
            Event::Timer(id) if Some(id) == self.retry_timer => self.retry_send(),
            Event::Timer(id) if Some(id) == self.reaction_timer => {
                self.reaction_timer = None;
                self.reaction = None;
                self.render(now);
            }
            Event::PanelVisibilityChanged(panel) => {
                self.panel = panel;
                self.render_panel(now);
            }
            Event::ProfileCardOpened(peer) if is_me(peer.session) => {
                if let Some(view) = self.view(now) {
                    show(Slot::ProfileCard(peer.session), view.profile_card());
                }
            }
            Event::ProfileCardOpened(peer) => {
                self.social.open_card = Some(peer.session);
                self.social.pat_note = None;
                self.render_other_card(peer.session, now);
            }
            _ => {}
        }
    }
}

fn start_timer(ms: u32, what: &str) -> Option<u32> {
    match timers::start(ms, false) {
        Ok(id) => Some(id),
        Err(e) => {
            log::warn(&format!("could not start the {what} timer: {e:?}"));
            None
        }
    }
}

fn start_timer_repeating(ms: u32) -> Option<u32> {
    match timers::start(ms, true) {
        Ok(id) => Some(id),
        Err(e) => {
            log::warn(&format!("could not start the game timer: {e:?}"));
            None
        }
    }
}

struct Echolings;

impl Plugin for Echolings {
    fn start(_info: StartInfo) -> Result<(), String> {
        let now = now();
        let mut pet = storage::load_pet()?;
        if let Some(pet) = &mut pet {
            pet.catch_up(now);
        }
        let tick_timer = timers::start(TICK_MS, true)
            .map_err(|e| format!("could not start the care timer: {e:?}"))?;
        let mut app = App {
            pet,
            session: Session::default(),
            channel: None,
            settings: storage::load_settings(),
            t: I18n::new("en"),
            reduced_motion: theme::current().reduced_motion,
            panel: PanelVisibility::Inline,
            draft: Draft::default(),
            need_name: false,
            reaction: None,
            reaction_timer: None,
            tick_timer: Some(tick_timer),
            refusal: None,
            badge: None,
            social: Social {
                ledger: social::visits::load(),
                ..Social::default()
            },
            granted: Vec::new(),
            visibility: Visibility::Nobody,
            card_timer: None,
            game: None,
            step_timer: None,
            end_timer: None,
            lobby_timer: None,
            retry: None,
            retry_timer: None,
        };
        app.save();
        app.render(now);
        app.render_settings();
        APP.with(|cell| *cell.borrow_mut() = Some(app));
        Ok(())
    }

    fn on_event(event: Event) {
        APP.with(|cell| {
            if let Some(app) = cell.borrow_mut().as_mut() {
                app.on_event(event);
            }
        });
    }

    fn stop() {
        APP.with(|cell| {
            if let Some(app) = cell.borrow_mut().take() {
                app.save();
            }
        });
    }
}

mumcord_plugin_sdk::export!(Echolings);
