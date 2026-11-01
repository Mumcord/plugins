//! The Snack Race card: invite, lobby and results as a chat card (or the panel without the
//! chat-card permission). The race itself runs in the web view.

use mumcord_plugin_sdk::ui::{self, Align, ButtonKind, Direction, El};

use super::{GOAL, Phase, Race, Score, action};
use crate::ui::i18n::I18n;

pub struct RaceView<'a> {
    pub race: &'a Race,
    pub t: &'a I18n,
    /// The web view could not open; the card says so instead of pointing at it.
    pub no_view: bool,
}

impl RaceView<'_> {
    /// One line: who started it, the lobby, the race or the results.
    pub fn status(&self) -> String {
        let t = self.t;
        match self.race.phase {
            Phase::Invited => {
                let host = self
                    .race
                    .host
                    .as_ref()
                    .map_or("", |(_, name)| name.as_str());
                t.f("race.invited", &[("name", host)])
            }
            Phase::Waiting if self.race.is_host() => t.t("race.hosting"),
            Phase::Waiting => t.t("race.waiting"),
            Phase::Racing => t.t("race.racing"),
            Phase::Finished => t.t("race.done"),
            Phase::Over if self.race.never_started() => t.t("race.notStarted"),
            Phase::Over if self.race.results().is_empty() => t.t("race.noScores"),
            Phase::Over => self.results().join(" · "),
        }
    }

    /// One line per finished run, best first.
    pub fn results(&self) -> Vec<String> {
        let you = self.t.t("race.you");
        self.race
            .results()
            .into_iter()
            .enumerate()
            .map(|(i, (name, score))| self.score_line(i + 1, name.unwrap_or(&you), score))
            .collect()
    }

    fn score_line(&self, place: usize, name: &str, score: Score) -> String {
        let place = place.to_string();
        if score.hops == GOAL {
            let s = (score.ms / 1000).to_string();
            let tenths = (score.ms % 1000 / 100).to_string();
            self.t.f(
                "race.scoreTime",
                &[("place", &place), ("name", name), ("s", &s), ("d", &tenths)],
            )
        } else {
            self.t.f(
                "race.scoreHops",
                &[
                    ("place", &place),
                    ("name", name),
                    ("n", &score.hops.to_string()),
                    ("goal", &GOAL.to_string()),
                ],
            )
        }
    }

    fn countdown(&self) -> El {
        match self.race.starts_at {
            Some(at) => ui::countdown(&self.t.t("race.startsIn"), at),
            None => ui::countdown(&self.t.t("race.endsIn"), self.race.ends_at),
        }
    }

    fn open_button(&self) -> El {
        ui::button(action::OPEN, &self.t.t("race.open")).kind(ButtonKind::Secondary)
    }

    pub fn card(&self) -> El {
        let t = self.t;
        let mut body = ui::stack(Direction::Column, 8, Align::Stretch)
            .child(ui::heading(&t.t("race.title")))
            .child(ui::text(&self.status()));
        if self.no_view && self.race.playing() && !self.race.is_over() {
            body = body.child(ui::caption(&t.t("race.noView")));
        }
        let row = match self.race.phase {
            Phase::Invited => vec![
                ui::button(action::JOIN, &t.t("race.join")).kind(ButtonKind::Primary),
                self.countdown(),
            ],
            Phase::Waiting if self.race.is_host() => vec![
                ui::button(action::START_NOW, &t.t("race.startNow")).kind(ButtonKind::Primary),
                self.open_button(),
                self.countdown(),
            ],
            Phase::Waiting | Phase::Racing | Phase::Finished => {
                vec![self.open_button(), self.countdown()]
            }
            Phase::Over => vec![ui::button(action::CLOSE, &t.t("race.close"))],
        };
        body.child(ui::row().children(row))
    }
}
