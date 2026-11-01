//! The plugin state: the fits seen in the chat, their cards and the open dialog. Host calls go
//! through [`Host`] so the logic runs in native tests.

use std::collections::VecDeque;

use mumcord_plugin_sdk::{events::Event, i18n::Strings, types::Error, ui::El};

use crate::eft::{self, Fit};
use crate::view;

/// Fits kept for their Open button; older cards lose the button. A fit is at most 4 KiB.
pub const KEPT_FITS: usize = 30;

pub trait Host {
    fn show_card(&mut self, id: &str, tree: El);
    fn show_dialog(&mut self, tree: El);
    fn open_dialog(&mut self, title: &str, subtitle: Option<&str>) -> Result<(), Error>;
    fn warn(&mut self, message: &str);
}

/// One fit pasted into the chat.
pub struct Pasted {
    pub card: u32,
    pub sender: String,
    pub fit: Fit,
}

pub struct App {
    strings: Strings,
    fits: VecDeque<Pasted>,
    /// Cards whose fit was dropped from `fits`; they stay in the chat without an Open button.
    dropped: Vec<(u32, String)>,
    next_card: u32,
    /// The card whose fit the dialog shows.
    open: Option<u32>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            strings: Strings::new(mumcord_plugin_sdk::i18n_all!()),
            fits: VecDeque::new(),
            dropped: Vec::new(),
            next_card: 1,
            open: None,
        }
    }
}

pub fn card_id(card: u32) -> String {
    format!("fit-{card}")
}

fn parse_card_id(id: &str) -> Option<u32> {
    id.strip_prefix("fit-")?.parse().ok()
}

impl App {
    /// Feeds the language event in; renders everything again when the language changed.
    pub fn on_event(&mut self, host: &mut impl Host, event: &Event) {
        if self.strings.on_event(event) {
            self.render_all(host);
        }
    }

    /// A chat message matched the EFT pattern. Text that only looks like a fit is ignored.
    pub fn on_chat_match(&mut self, host: &mut impl Host, sender: &str, text: &str) {
        let Some(fit) = eft::parse(text) else {
            return;
        };
        let card = self.next_card;
        self.next_card = self.next_card.wrapping_add(1).max(1);
        let pasted = Pasted {
            card,
            sender: sender.to_owned(),
            fit,
        };
        host.show_card(&card_id(card), view::card(&self.strings, &pasted));
        self.fits.push_back(pasted);
        if self.fits.len() > KEPT_FITS
            && let Some(old) = self.fits.pop_front()
        {
            host.show_card(
                &card_id(old.card),
                view::dropped_card(&self.strings, &old.fit.ship),
            );
            if self.open == Some(old.card) {
                // The dialog keeps showing what it shows; only the Open buttons go.
                self.open = None;
            }
            // Keep just enough to render the dropped card again in another language.
            self.dropped.push((old.card, old.fit.ship));
            if self.dropped.len() > KEPT_FITS * 4 {
                self.dropped.remove(0);
            }
        }
    }

    /// A button on a card was clicked.
    pub fn on_card_action(&mut self, host: &mut impl Host, id: &str, action: &str) {
        if action != view::ACTION_OPEN {
            return;
        }
        let Some(card) = parse_card_id(id) else {
            return;
        };
        let Some(pasted) = self.fits.iter().find(|p| p.card == card) else {
            return;
        };
        let title = view::dialog_title(&pasted.fit);
        let subtitle = view::dialog_subtitle(&self.strings, pasted);
        if let Err(e) = host.open_dialog(&title, Some(&subtitle)) {
            host.warn(&format!("could not open the fit dialog: {e:?}"));
            return;
        }
        host.show_dialog(view::dialog(&self.strings, &pasted.fit));
        self.open = Some(card);
    }

    pub fn on_dialog_closed(&mut self) {
        self.open = None;
    }

    fn render_all(&mut self, host: &mut impl Host) {
        for pasted in &self.fits {
            host.show_card(&card_id(pasted.card), view::card(&self.strings, pasted));
        }
        for (card, ship) in &self.dropped {
            host.show_card(&card_id(*card), view::dropped_card(&self.strings, ship));
        }
        if let Some(pasted) = self
            .open
            .and_then(|c| self.fits.iter().find(|p| p.card == c))
        {
            let title = view::dialog_title(&pasted.fit);
            let subtitle = view::dialog_subtitle(&self.strings, pasted);
            if let Err(e) = host.open_dialog(&title, Some(&subtitle)) {
                host.warn(&format!("could not retitle the fit dialog: {e:?}"));
            }
            host.show_dialog(view::dialog(&self.strings, &pasted.fit));
        }
    }
}
