//! What other people's pets look like: member badge and profile-card section with a pat.

use mumcord_plugin_sdk::ui::{self, Align, Animation, ButtonKind, Direction, El};

use super::{OtherPet, PatNote, action};
use crate::pet::{Mood, Stage};
use crate::ui::{Motion, i18n::I18n, mood_key, species_key, stage_key};

pub struct OtherView<'a> {
    pub pet: &'a OtherPet,
    pub t: &'a I18n,
    pub motion: Motion,
}

impl OtherView<'_> {
    fn species_name(&self) -> &str {
        self.t.t(species_key(self.pet.card.species))
    }

    /// Screen-reader label, e.g. "Biscuit the Fizzle, cheerful".
    pub fn label(&self) -> String {
        self.t.f(
            "pet.label",
            &[
                ("name", &self.pet.card.name),
                ("species", self.species_name()),
                ("mood", self.t.t(mood_key(self.pet.card.mood))),
            ],
        )
    }

    fn animation(&self) -> Option<Animation> {
        let wanted = if self.pet.card.mood == Mood::Sleeping {
            Some(Animation::Nap)
        } else if self.pet.talking {
            Some(Animation::Bob)
        } else if self.pet.card.stage() == Stage::Echo {
            Some(Animation::Sparkle)
        } else {
            None
        };
        wanted.and_then(|a| self.motion.allow(a))
    }

    fn picture(&self, size: u16) -> El {
        let image = ui::image(&self.pet.card.asset(), &self.label(), size, size);
        match self.animation() {
            Some(animation) => image.animate(animation),
            None => image,
        }
    }

    /// The 24 px pet next to its owner's name; it bobs while they talk.
    pub fn badge(&self) -> El {
        self.picture(24)
    }

    /// The pet's section on its owner's profile card, with the visit pat. `can_send` is false
    /// without the `peers.messages` permission.
    pub fn profile_card(
        &self,
        session: u32,
        patted_today: bool,
        can_send: bool,
        note: Option<PatNote>,
    ) -> El {
        let card = &self.pet.card;
        let kind = self.t.f(
            "pet.kind",
            &[
                ("species", self.species_name()),
                ("stage", self.t.t(stage_key(card.stage().id()).as_str())),
            ],
        );
        let name = [("name", card.name.as_str())];
        let mut body = ui::column()
            .child(
                ui::stack(Direction::Row, 12, Align::Center)
                    .child(self.picture(96))
                    .child(
                        ui::column()
                            .child(ui::heading(&card.name))
                            .child(ui::text(&kind))
                            .child(ui::caption(self.t.t(mood_key(card.mood)))),
                    ),
            )
            .child(
                ui::row().children([
                    ui::chip(&self.t.f("stat.bond", &[("bond", &card.bond.to_string())])),
                    ui::chip(&self.t.f(
                        "stat.together",
                        &[("days", &card.days_together.to_string())],
                    )),
                    ui::chip(
                        &self
                            .t
                            .f("stat.streak", &[("days", &card.streak.to_string())]),
                    ),
                ]),
            )
            .child(
                ui::button(&action::pat(session), &self.t.f("visit.pat", &name))
                    .kind(ButtonKind::Primary)
                    .disabled(patted_today || !can_send),
            );
        let note_key = match note {
            Some(PatNote::Given) => Some("visit.given"),
            Some(PatNote::AlreadyToday) => Some("visit.already"),
            Some(PatNote::Failed) => Some("visit.failed"),
            None if patted_today => Some("visit.already"),
            None => None,
        };
        if let Some(key) = note_key {
            body = body.child(ui::caption(&self.t.f(key, &name)));
        }
        body
    }
}
