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
    /// Shows the fitting wheel page (`web/index.html`) in the open dialog.
    fn show_webview(&mut self) -> Result<(), Error>;
    /// Sends a message to the page in the dialog.
    fn post_webview(&mut self, message: &str) -> Result<(), Error>;
    fn warn(&mut self, message: &str);
}

/// What the page sends once it listens, asking for the fit.
pub const WEBVIEW_READY: &str = "ready";

/// One fit pasted into the chat.
pub struct Pasted {
    pub card: u32,
    pub sender: String,
    pub fit: Fit,
    /// The message as pasted (at most 4 KiB), for the Copy fitting button.
    pub text: String,
}

pub struct App {
    strings: Strings,
    fits: VecDeque<Pasted>,
    /// Cards whose fit was dropped from `fits`; they stay in the chat without an Open button.
    dropped: Vec<(u32, String)>,
    next_card: u32,
    /// The card whose fit the dialog shows.
    open: Option<u32>,
    /// Whether the dialog shows the fitting wheel page rather than the native list.
    web: bool,
    /// The host's language tag, passed on to the page.
    language: String,
}

impl Default for App {
    fn default() -> Self {
        Self {
            strings: Strings::new(mumcord_plugin_sdk::i18n_all!()),
            fits: VecDeque::new(),
            dropped: Vec::new(),
            next_card: 1,
            open: None,
            web: false,
            language: "en".to_owned(),
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
        if let Event::LanguageChanged(tag) = event {
            self.language.clone_from(tag);
        }
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
            text: text.to_owned(),
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
        // The page asks for the fit once it has loaded (see `on_webview_message`). Without a web
        // view (e.g. the permission was refused) the dialog shows the native list.
        self.web = match host.show_webview() {
            Ok(()) => true,
            Err(e) => {
                host.warn(&format!(
                    "could not show the fitting wheel, showing the list: {e:?}"
                ));
                host.show_dialog(view::dialog(&self.strings, pasted));
                false
            }
        };
        self.open = Some(card);
    }

    /// A message from the page. Only `ready` means anything; the page is untrusted, so
    /// everything else is ignored.
    pub fn on_webview_message(&mut self, host: &mut impl Host, message: &str) {
        if message == WEBVIEW_READY && self.web {
            self.post_fit(host);
        }
    }

    fn post_fit(&self, host: &mut impl Host) {
        let Some(pasted) = self.open_fit() else {
            return;
        };
        let message = view::webview_message(&self.strings, &self.language, &pasted.text);
        if let Err(e) = host.post_webview(&message) {
            host.warn(&format!(
                "could not send the fit to the fitting wheel: {e:?}"
            ));
        }
    }

    fn open_fit(&self) -> Option<&Pasted> {
        self.open
            .and_then(|c| self.fits.iter().find(|p| p.card == c))
    }

    pub fn on_dialog_closed(&mut self) {
        self.open = None;
        self.web = false;
    }

    fn render_all(&mut self, host: &mut impl Host) {
        for pasted in &self.fits {
            host.show_card(&card_id(pasted.card), view::card(&self.strings, pasted));
        }
        for (card, ship) in &self.dropped {
            host.show_card(&card_id(*card), view::dropped_card(&self.strings, ship));
        }
        if let Some(pasted) = self.open_fit() {
            let title = view::dialog_title(&pasted.fit);
            let subtitle = view::dialog_subtitle(&self.strings, pasted);
            if let Err(e) = host.open_dialog(&title, Some(&subtitle)) {
                host.warn(&format!("could not retitle the fit dialog: {e:?}"));
            }
            if self.web {
                // The open page takes the new texts with the fit.
                self.post_fit(host);
            } else {
                host.show_dialog(view::dialog(&self.strings, pasted));
            }
        }
    }
}
