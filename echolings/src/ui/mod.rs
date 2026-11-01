//! What the pet looks like in each slot. Pure tree building, so it runs in host tests; the
//! plugin entry point decides when to show which tree.

pub mod i18n;
#[cfg(test)]
mod tests;

use mumcord_plugin_sdk::ui::{self, Align, Animation, ButtonKind, Direction, El};

use crate::pet::{Mood, Now, Pet, Reaction, Refusal, SMALL_MAX, Session, Species, balance};
use crate::storage::AnimationLevel;
use i18n::I18n;

/// `ui-action` names.
pub mod action {
    pub const PAT: &str = "pat";
    pub const TREAT: &str = "treat";
    pub const PLAY: &str = "play";
    pub const NAP: &str = "nap";
    pub const RENAME: &str = "rename";
    pub const COLOUR: &str = "colour";
    pub const PICK_SPECIES: &str = "pick-species";
    pub const PICK_COLOUR: &str = "pick-colour";
    pub const PICK_NAME: &str = "pick-name";
    pub const HATCH: &str = "hatch";
    pub const ANIMATION: &str = "animation";
}

/// Text inputs take at most this many bytes (16 characters of up to 4 bytes).
const NAME_INPUT_BYTES: u8 = 64;

/// How much motion the user wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Motion {
    pub level: AnimationLevel,
    /// The system's reduced-motion setting.
    pub reduced: bool,
}

impl Motion {
    /// Full plays everything, Subtle only the calm talking bob and nap, Still nothing.
    pub(crate) fn allow(self, animation: Animation) -> Option<Animation> {
        match self.level {
            _ if self.reduced => None,
            AnimationLevel::Still => None,
            AnimationLevel::Subtle => {
                matches!(animation, Animation::Bob | Animation::Nap).then_some(animation)
            }
            AnimationLevel::Full => Some(animation),
        }
    }
}

/// The species picker before the first hatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub species: Species,
    pub colour: u8,
    pub name: String,
}

impl Default for Draft {
    fn default() -> Self {
        Draft {
            species: Species::Fizzle,
            colour: 0,
            name: String::new(),
        }
    }
}

/// Everything a pet view needs.
pub struct View<'a> {
    pub pet: &'a Pet,
    pub session: &'a Session,
    pub now: Now,
    pub t: &'a I18n,
    pub motion: Motion,
    /// The latest care reaction, while it plays.
    pub reaction: Option<Reaction>,
    /// The latest refused action, shown until the next action.
    pub refusal: Option<Refusal>,
}

impl View<'_> {
    fn species_name(&self) -> String {
        self.t.t(species_key(self.pet.species))
    }

    /// Screen-reader label, e.g. "Biscuit the Fizzle, cheerful".
    pub fn label(&self) -> String {
        let mood = self.pet.mood(self.session, self.now);
        self.t.f(
            "pet.label",
            &[
                ("name", &self.pet.name),
                ("species", &self.species_name()),
                ("mood", &self.t.t(mood_key(mood))),
            ],
        )
    }

    fn kind(&self) -> String {
        self.t.f(
            "pet.kind",
            &[
                ("species", &self.species_name()),
                (
                    "stage",
                    &self.t.t(stage_key(self.pet.stage().id()).as_str()),
                ),
            ],
        )
    }

    fn animation(&self) -> Option<Animation> {
        let wanted = if self.pet.asleep(self.session, self.now) {
            Some(Animation::Nap)
        } else if let Some(reaction) = self.reaction {
            Some(match reaction {
                Reaction::Hop => Animation::Hop,
                Reaction::Wave => Animation::Wave,
                Reaction::Sparkle => Animation::Sparkle,
                Reaction::Nap => Animation::Nap,
            })
        } else if self.session.talking {
            Some(Animation::Bob)
        } else if self.pet.stage() == crate::pet::Stage::Echo {
            Some(Animation::Sparkle)
        } else {
            None
        };
        wanted.and_then(|a| self.motion.allow(a))
    }

    fn picture(&self, size: u16) -> El {
        let asleep = self.pet.asleep(self.session, self.now);
        let image = ui::image(
            &self.pet.asset(size <= SMALL_MAX, asleep),
            &self.label(),
            size,
            size,
        );
        match self.animation() {
            Some(animation) => image.animate(animation),
            None => image,
        }
    }

    fn meters(&self) -> [El; 2] {
        [
            ui::meter(&self.t.t("meter.cheer"), fraction(self.pet.cheer)),
            ui::meter(&self.t.t("meter.energy"), fraction(self.pet.energy)),
        ]
    }

    fn notes_chip(&self) -> El {
        ui::chip(
            &self
                .t
                .f("stat.notes", &[("notes", &self.pet.notes.to_string())]),
        )
    }

    fn actions(&self) -> El {
        let asleep = self.pet.asleep(self.session, self.now);
        let cost = balance::TREAT_COST.to_string();
        ui::row().children([
            ui::button(action::PAT, &self.t.t("action.pat")).kind(ButtonKind::Primary),
            ui::button(action::TREAT, &self.t.f("action.treat", &[("cost", &cost)]))
                .kind(ButtonKind::Secondary)
                .disabled(self.pet.notes < balance::TREAT_COST),
            ui::button(action::PLAY, &self.t.t("action.play"))
                .kind(ButtonKind::Secondary)
                .disabled(asleep || self.pet.energy < balance::PLAY_ENERGY_COST),
            ui::button(action::NAP, &self.t.t("action.nap"))
                .kind(ButtonKind::Secondary)
                .disabled(asleep),
        ])
    }

    /// One row of small icon buttons; the labels are what screen readers and tooltips say.
    fn icon_actions(&self) -> El {
        let asleep = self.pet.asleep(self.session, self.now);
        let cost = balance::TREAT_COST.to_string();
        ui::stack(Direction::Row, 8, Align::Center).children([
            ui::icon_button(action::PAT, "assets/icons/pat.svg", &self.t.t("action.pat")),
            ui::icon_button(
                action::TREAT,
                "assets/icons/treat.svg",
                &self.t.f("action.treat", &[("cost", &cost)]),
            )
            .disabled(self.pet.notes < balance::TREAT_COST),
            ui::icon_button(
                action::PLAY,
                "assets/icons/play.svg",
                &self.t.t("action.play"),
            )
            .disabled(asleep || self.pet.energy < balance::PLAY_ENERGY_COST),
            ui::icon_button(action::NAP, "assets/icons/nap.svg", &self.t.t("action.nap"))
                .disabled(asleep),
        ])
    }

    fn refusal_text(&self) -> Option<El> {
        let key = match self.refusal? {
            Refusal::NotEnoughNotes => "refusal.notEnoughNotes",
            Refusal::TooTired => "refusal.tooTired",
            Refusal::Asleep => "refusal.asleep",
        };
        Some(ui::caption(&self.t.f(key, &[("name", &self.pet.name)])))
    }

    fn growth(&self) -> El {
        let bond = self.pet.bond.to_string();
        let stage = self.pet.stage();
        match stage.next_bond() {
            Some(next) => {
                let next_stage = crate::pet::Stage::for_bond(next);
                let label = self.t.f(
                    "growth.next",
                    &[
                        ("bond", &bond),
                        ("stage", &self.t.t(stage_key(next_stage.id()).as_str())),
                        ("next", &next.to_string()),
                    ],
                );
                let done =
                    f64::from(self.pet.bond * balance::POINTS_PER_BOND + self.pet.bond_points)
                        / f64::from(next * balance::POINTS_PER_BOND);
                ui::progress(&label, Some(done.min(1.0) as f32))
            }
            None => ui::progress(&self.t.f("growth.done", &[("bond", &bond)]), Some(1.0)),
        }
    }

    fn stats(&self) -> El {
        let today = self.pet.notes_today(self.now).to_string();
        let streak = self.pet.streak.current(self.now.day()).to_string();
        ui::row().children([
            self.notes_chip(),
            ui::chip(&self.t.f("stat.today", &[("notes", &today)])),
            ui::chip(&self.t.f("stat.streak", &[("days", &streak)])),
            ui::chip(
                &self
                    .t
                    .n("stat.together", u64::from(self.pet.streak.days_together)),
            ),
        ])
    }

    fn colour_select(&self, action: &str, label: &str) -> El {
        colour_select(
            self.t,
            self.pet.species,
            self.pet.colour,
            self.pet.unlocked_colours(),
            action,
            label,
        )
    }

    /// The 260×160 panel in the Plugins strip: big pet, two meters and the four care actions.
    /// Everything else lives in the popover, so nothing here needs to scroll.
    pub fn panel_inline(&self) -> El {
        // A refusal replaces the species line so the panel keeps its height.
        let subtitle = match self.refusal_text() {
            Some(refusal) => refusal,
            None => ui::caption(&self.kind()),
        };
        let details = ui::stack(Direction::Column, 2, Align::Stretch)
            .child(ui::heading(&self.pet.name))
            .child(subtitle)
            .children(self.meters());
        ui::stack(Direction::Column, 4, Align::Stretch)
            .child(
                ui::stack(Direction::Row, 8, Align::Center)
                    .child(self.picture(96))
                    .child(details),
            )
            .child(self.icon_actions())
    }

    /// The 420×560 popover: meters, growth, stats, care actions and customising.
    pub fn panel_expanded(&self) -> El {
        let mood = self.pet.mood(self.session, self.now);
        let header = ui::stack(Direction::Row, 12, Align::Center)
            .child(self.picture(128))
            .child(
                ui::column()
                    .child(ui::heading(&self.pet.name))
                    .child(ui::text(&self.kind()))
                    .child(ui::caption(&self.t.t(mood_key(mood)))),
            );
        let mut body = ui::column()
            .child(header)
            .children(self.meters())
            .child(self.growth())
            .child(self.stats())
            .child(self.actions());
        if let Some(refusal) = self.refusal_text() {
            body = body.child(refusal);
        }
        body.child(ui::divider())
            .child(ui::text_input(
                action::RENAME,
                &self.t.t("customise.name"),
                &self.pet.name,
                NAME_INPUT_BYTES,
            ))
            .child(self.colour_select(action::COLOUR, &self.t.t("customise.colour")))
    }

    /// The 24 px pet next to the user's own name.
    pub fn badge(&self) -> El {
        self.picture(24)
    }

    /// The pet's section on the user's own profile card.
    pub fn profile_card(&self) -> El {
        let mood = self.pet.mood(self.session, self.now);
        let bond = self.pet.bond.to_string();
        let streak = self.pet.streak.current(self.now.day()).to_string();
        ui::column()
            .child(
                ui::stack(Direction::Row, 12, Align::Center)
                    .child(self.picture(96))
                    .child(
                        ui::column()
                            .child(ui::heading(&self.pet.name))
                            .child(ui::text(&self.kind()))
                            .child(ui::caption(&self.t.t(mood_key(mood)))),
                    ),
            )
            .child(
                ui::row().children([
                    ui::chip(&self.t.f("stat.bond", &[("bond", &bond)])),
                    ui::chip(
                        &self
                            .t
                            .n("stat.together", u64::from(self.pet.streak.days_together)),
                    ),
                    ui::chip(&self.t.f("stat.streak", &[("days", &streak)])),
                ]),
            )
    }
}

/// The inline panel before the first hatch only points to the expanded panel.
pub fn hatch_prompt(t: &I18n) -> El {
    ui::column()
        .child(ui::heading(&t.t("picker.title")))
        .child(ui::caption(&t.t("panel.hatchPrompt")))
}

/// Species, name and colour for a new pet.
pub fn picker(t: &I18n, draft: &Draft, need_name: bool, motion: Motion) -> El {
    let options: Vec<(String, String)> = Species::ALL
        .iter()
        .map(|s| {
            let label = t.f(
                "picker.speciesOption",
                &[
                    ("species", &t.t(species_key(*s))),
                    ("trait", &t.t(trait_key(*s))),
                ],
            );
            (s.id().to_owned(), label)
        })
        .collect();
    let options: Vec<(&str, &str)> = options
        .iter()
        .map(|(v, l)| (v.as_str(), l.as_str()))
        .collect();
    let colour = draft.species.colours()[usize::from(draft.colour.min(2))];
    let preview_alt = t.f(
        "picker.speciesOption",
        &[
            ("species", &t.t(species_key(draft.species))),
            ("trait", &t.t(trait_key(draft.species))),
        ],
    );
    // Show what the Hum will grow into, so the species choice is visible.
    let preview = ui::image(
        &format!("assets/pets/{}-buddy-{colour}.svg", draft.species.id()),
        &preview_alt,
        112,
        112,
    );
    let preview = match motion.allow(Animation::Bob) {
        Some(a) => preview.animate(a),
        None => preview,
    };
    let mut tree = ui::column()
        .child(ui::heading(&t.t("picker.title")))
        .child(ui::caption(&t.t("picker.intro")))
        .child(ui::stack(Direction::Row, 8, Align::Center).child(preview))
        .child(ui::select(
            action::PICK_SPECIES,
            &t.t("picker.species"),
            &options,
            Some(draft.species.id()),
        ))
        .child(ui::text_input(
            action::PICK_NAME,
            &t.t("picker.name"),
            &draft.name,
            NAME_INPUT_BYTES,
        ))
        .child(colour_select(
            t,
            draft.species,
            draft.colour,
            2,
            action::PICK_COLOUR,
            &t.t("picker.colour"),
        ));
    if need_name {
        tree = tree.child(ui::caption(&t.t("picker.needName")));
    }
    tree.child(ui::button(action::HATCH, &t.t("picker.hatch")).kind(ButtonKind::Primary))
}

/// This plugin's form in Settings → Plugins.
pub fn settings(t: &I18n, level: AnimationLevel) -> El {
    let labels: Vec<(&str, String)> = AnimationLevel::ALL
        .iter()
        .map(|a| (a.id(), t.t(animation_key(*a))))
        .collect();
    let options: Vec<(&str, &str)> = labels.iter().map(|(v, l)| (*v, l.as_str())).collect();
    ui::column().child(ui::select(
        action::ANIMATION,
        &t.t("settings.animation"),
        &options,
        Some(level.id()),
    ))
}

fn colour_select(
    t: &I18n,
    species: Species,
    selected: u8,
    unlocked: u8,
    action: &str,
    label: &str,
) -> El {
    let names: Vec<(String, String)> = species
        .colours()
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let name = t.t(&format!("colour.{id}"));
            let name = if i < usize::from(unlocked) {
                name
            } else {
                t.f(
                    "colour.locked",
                    &[
                        ("colour", &name),
                        ("bond", &balance::THIRD_COLOUR_BOND.to_string()),
                    ],
                )
            };
            (i.to_string(), name)
        })
        .collect();
    let options: Vec<(&str, &str)> = names
        .iter()
        .map(|(v, l)| (v.as_str(), l.as_str()))
        .collect();
    ui::select(action, label, &options, Some(&selected.to_string()))
}

pub(crate) fn fraction(meter: u8) -> f32 {
    f32::from(meter) / f32::from(balance::METER_MAX)
}

pub(crate) fn species_key(species: Species) -> &'static str {
    match species {
        Species::Fizzle => "species.fizzle",
        Species::Burrble => "species.burrble",
        Species::Mossmo => "species.mossmo",
        Species::Zapling => "species.zapling",
        Species::Hushmoth => "species.hushmoth",
    }
}

fn trait_key(species: Species) -> &'static str {
    match species {
        Species::Fizzle => "trait.fizzle",
        Species::Burrble => "trait.burrble",
        Species::Mossmo => "trait.mossmo",
        Species::Zapling => "trait.zapling",
        Species::Hushmoth => "trait.hushmoth",
    }
}

pub(crate) fn stage_key(stage: &str) -> String {
    format!("stage.{stage}")
}

pub(crate) fn mood_key(mood: Mood) -> &'static str {
    match mood {
        Mood::Sleeping => "mood.sleeping",
        Mood::Sleepy => "mood.sleepy",
        Mood::MissesYou => "mood.missesYou",
        Mood::Content => "mood.content",
        Mood::Cheerful => "mood.cheerful",
    }
}

fn animation_key(level: AnimationLevel) -> &'static str {
    match level {
        AnimationLevel::Full => "animation.full",
        AnimationLevel::Subtle => "animation.subtle",
        AnimationLevel::Still => "animation.still",
    }
}
