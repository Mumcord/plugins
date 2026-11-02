//! The Echo Echo card: a 440×160 chat card, or the panel without the chat-card permission.

use mumcord_plugin_sdk::ui::{self, Align, Animation, ButtonKind, Direction, El};

use super::{Game, PADS, Phase, action};
use crate::ui::{Motion, i18n::I18n};

pub struct GameView<'a> {
    pub game: &'a Game,
    pub t: &'a I18n,
    pub motion: Motion,
}

impl GameView<'_> {
    fn len(&self) -> String {
        self.game.tune.len().to_string()
    }

    fn status(&self) -> String {
        let t = self.t;
        match self.game.phase {
            Phase::Invited => {
                let host = self
                    .game
                    .host
                    .as_ref()
                    .map_or("", |(_, name)| name.as_str());
                t.f("echo.invited", &[("name", host)])
            }
            Phase::Waiting if self.game.is_host() => t.t("echo.hosting"),
            Phase::Waiting => t.t("echo.waiting"),
            Phase::Listen { step } => t.f(
                "echo.listen",
                &[("n", &(step + 1).to_string()), ("len", &self.len())],
            ),
            Phase::Repeat { entered } => t.f(
                "echo.repeat",
                &[("n", &entered.to_string()), ("len", &self.len())],
            ),
            Phase::Finished { correct } => t.f(
                "echo.finished",
                &[("n", &correct.to_string()), ("len", &self.len())],
            ),
            Phase::Over if self.game.never_started() => t.t("echo.notStarted"),
            Phase::Over => self.results(),
        }
    }

    /// Time left in the lobby, or until the scores once the round started.
    fn countdown(&self) -> El {
        match self.game.starts_at {
            Some(at) => ui::countdown(&self.t.t("echo.startsIn"), at),
            None => ui::countdown(&self.t.t("echo.endsIn"), self.game.ends_at),
        }
    }

    fn results(&self) -> String {
        let you = &self.t.t("echo.you");
        let len = self.len();
        let lines: Vec<String> = self
            .game
            .results()
            .into_iter()
            .map(|(name, correct)| {
                self.t.f(
                    "echo.score",
                    &[
                        ("name", name.unwrap_or(you)),
                        ("n", &correct.to_string()),
                        ("len", &len),
                    ],
                )
            })
            .collect();
        if lines.is_empty() {
            self.t.t("echo.noScores")
        } else {
            lines.join(" · ")
        }
    }

    fn pads(&self) -> El {
        let playing = self.game.playing();
        let open = matches!(self.game.phase, Phase::Repeat { .. });
        ui::row().children((0..PADS).map(|pad| {
            let label = self.t.f("echo.pad", &[("n", &(pad + 1).to_string())]);
            let lit = playing == Some(pad);
            let button = ui::button(&action::pad(pad), &label)
                .kind(if lit {
                    ButtonKind::Primary
                } else {
                    ButtonKind::Secondary
                })
                .disabled(!open);
            match self.motion.allow(Animation::Hop).filter(|_| lit) {
                Some(animation) => button.animate(animation),
                None => button,
            }
        }))
    }

    pub fn card(&self) -> El {
        let mut body = ui::stack(Direction::Column, 8, Align::Stretch)
            .child(ui::heading(&self.t.t("echo.title")))
            .child(ui::text(&self.status()));
        match self.game.phase {
            Phase::Invited => {
                body = body.child(ui::row().children([
                    ui::button(action::JOIN, &self.t.t("echo.join")).kind(ButtonKind::Primary),
                    self.countdown(),
                ]));
            }
            Phase::Waiting if self.game.is_host() => {
                body = body.child(
                    ui::row().children([
                        ui::button(action::START_NOW, &self.t.t("echo.startNow"))
                            .kind(ButtonKind::Primary),
                        self.countdown(),
                    ]),
                );
            }
            Phase::Waiting => body = body.child(self.countdown()),
            Phase::Listen { .. } | Phase::Repeat { .. } => body = body.child(self.pads()),
            Phase::Finished { .. } => {
                body = body.child(ui::countdown(&self.t.t("echo.endsIn"), self.game.ends_at));
            }
            Phase::Over => {
                body = body.child(ui::button(action::CLOSE, &self.t.t("echo.close")));
            }
        }
        body
    }
}
