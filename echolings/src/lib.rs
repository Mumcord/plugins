//! Echolings: a little creature made of sound that grows while you hang out with friends.
//!
//! This file only wires host events to the pet logic (`pet`), the views (`ui`) and plugin
//! storage (`storage`).

pub mod pet;
pub mod storage;
pub mod ui;

use std::cell::RefCell;

use mumcord_plugin_sdk::{
    Plugin, clock,
    events::{ActionValue, Event, PanelVisibility, SelfState, UiAction},
    log, theme, timers,
    types::{Error, StartInfo},
    ui::{self as host_ui, El, Slot},
};

use pet::{Now, Pet, Reaction, Refusal, Session, Species, clean_name};
use storage::{AnimationLevel, Settings};
use ui::{Draft, Motion, View, action, i18n::I18n};

/// The care loop advances once a minute; nothing else polls, so the plugin idles cheaply.
const TICK_MS: u32 = 60_000;
/// How long a care reaction plays before the pet goes back to its resting animation.
const REACTION_MS: u32 = 3_000;

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
    }

    fn render_panel(&self, now: Now) {
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
            (action::PLAY, Some(pet)) => match pet.play(&self.session, now) {
                Ok(reaction) => self.react(Some(reaction)),
                Err(refusal) => self.refusal = Some(refusal),
            },
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
    }

    fn on_event(&mut self, event: Event) {
        let now = now();
        let me = self.session.me;
        let is_me = |session: u32| me == Some(session);
        match event {
            Event::Talking((peer, talking)) if is_me(peer.session) => {
                self.own_talking(talking, now);
            }
            Event::Joined(peer) if !is_me(peer.session) => self.session.joined(peer.session, now),
            Event::Left(peer) => self.session.left(peer.session),
            Event::Mentioned(_) => self.session.mentioned(now),
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
                self.t = I18n::new(&language);
                self.render(now);
                self.render_settings();
            }
            Event::UiAction(action) => self.ui_action(action, now),
            Event::Timer(id) if Some(id) == self.tick_timer => self.tick(now),
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
            // Other people's pets, visits and reactions come with the social part.
            _ => {}
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
