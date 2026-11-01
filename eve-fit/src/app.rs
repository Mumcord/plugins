//! The plugin state: the fits seen in the chat, their cards and the open dialog. Host calls go
//! through [`Host`] so the logic runs in native tests.

use std::collections::VecDeque;

use mumcord_plugin_sdk::{events::Event, i18n::Strings, types::Error, ui::El};

use crate::eft::{self, Fit};
use crate::history::{History, Store};
use crate::names;
use crate::view;

/// Fits kept for their Open button; older cards lose the button. A fit is at most 4 KiB.
pub const KEPT_FITS: usize = 30;

pub trait Host: Store {
    fn show_card(&mut self, id: &str, tree: El);
    fn show_dialog(&mut self, tree: El);
    fn open_dialog(&mut self, title: &str, subtitle: Option<&str>) -> Result<(), Error>;
    /// Shows the fitting wheel page (`web/index.html`) in the open dialog.
    fn show_webview(&mut self) -> Result<(), Error>;
    /// Sends a message to the page in the dialog.
    fn post_webview(&mut self, message: &str) -> Result<(), Error>;
    fn show_panel(&mut self, tree: El);
    /// Opens the large panel popover; needs a fresh click, which the caller just received.
    fn open_popover(&mut self);
    /// Seconds since 1970 (UTC) and the user's offset from UTC in minutes.
    fn now(&mut self) -> (u64, i32);
    fn warn(&mut self, message: &str);
    /// Whether the event being handled was replayed from stored chat history.
    fn is_replay(&mut self) -> bool;
    /// Shows card `id` in place of the chat message behind the current event.
    fn replace_message(&mut self, id: &str);
}

/// What the page sends once it listens, asking for the fit.
pub const WEBVIEW_READY: &str = "ready";

/// One fit pasted into the chat, or opened from the history.
#[derive(Clone)]
pub struct Pasted {
    /// The chat card; 0 for a fit opened from the history.
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
    /// The fit the dialog shows.
    open: Option<Pasted>,
    /// Whether the dialog shows the fitting wheel page rather than the native list.
    web: bool,
    /// The host's language tag, passed on to the page.
    language: String,
    history: History,
    /// Whether the optional `ui.panel` is granted; the history lives in the panel.
    panel: bool,
    /// Whether the panel is the large popover rather than the small inline one.
    expanded: bool,
    /// Whether the history section is unfolded; in memory only, so it starts collapsed.
    history_open: bool,
    /// First entry of each page up to the shown one; the last is the shown page.
    pages: Vec<usize>,
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
            history: History::default(),
            panel: false,
            expanded: false,
            history_open: false,
            pages: vec![0],
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
    /// Reads the stored history; called once when the plugin starts.
    pub fn start(&mut self, host: &mut impl Host) {
        self.history = History::load(host);
    }

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
        // A replayed fit was added when it was first seen; adding it again would move it to the top.
        if !host.is_replay() {
            let (now, _) = host.now();
            self.history.add(host, sender, now, text, &fit);
            self.pages = vec![0];
            self.show_panel(host);
        }
        let card = self.next_card;
        self.next_card = self.next_card.wrapping_add(1).max(1);
        let pasted = Pasted {
            card,
            sender: sender.to_owned(),
            fit,
            text: text.to_owned(),
        };
        // Replace first, so the card takes the message's place instead of appearing below it.
        host.replace_message(&card_id(card));
        host.show_card(&card_id(card), view::card(&self.strings, &pasted));
        self.fits.push_back(pasted);
        if self.fits.len() > KEPT_FITS
            && let Some(old) = self.fits.pop_front()
        {
            host.show_card(
                &card_id(old.card),
                view::dropped_card(&self.strings, &old.fit.ship),
            );
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
        let Some(pasted) = self.fits.iter().find(|p| p.card == card).cloned() else {
            return;
        };
        let (now, _) = host.now();
        self.history
            .add(host, &pasted.sender, now, &pasted.text, &pasted.fit);
        self.pages = vec![0];
        self.show_panel(host);
        self.open_fit_dialog(host, pasted);
    }

    /// Opens the dialog for a fit: the fitting wheel, or the native list without a web view.
    fn open_fit_dialog(&mut self, host: &mut impl Host, pasted: Pasted) {
        let title = view::dialog_title(&pasted.fit);
        let subtitle = view::dialog_subtitle(&self.strings, &pasted);
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
                host.show_dialog(view::dialog(&self.strings, &pasted));
                false
            }
        };
        self.open = Some(pasted);
    }

    /// The granted permissions changed; the history panel needs the optional `ui.panel`.
    pub fn set_granted(&mut self, host: &mut impl Host, granted: &[String]) {
        let panel = granted.iter().any(|p| p == "ui.panel");
        let turned_on = panel && !self.panel;
        self.panel = panel;
        if turned_on {
            self.show_panel(host);
        }
    }

    /// The panel moved between the strip and the large popover; each has its own layout.
    pub fn on_panel_visibility(&mut self, host: &mut impl Host, expanded: bool) {
        if self.expanded != expanded {
            self.expanded = expanded;
            self.show_panel(host);
        }
    }

    /// A click in the panel.
    pub fn on_panel_action(&mut self, host: &mut impl Host, action: &str) {
        if action == view::ACTION_SHOW_HISTORY {
            // The inline panel is too small for the list; it opens the popover unfolded.
            self.history_open = true;
            host.open_popover();
        } else if action == view::ACTION_TOGGLE_HISTORY {
            self.history_open = !self.history_open;
        } else if action == view::ACTION_OLDER {
            let start = self.page_start();
            let end = self.history.page_end(start);
            if end < self.history.entries().len() {
                self.pages.push(end);
            }
        } else if action == view::ACTION_NEWER {
            if self.pages.len() > 1 {
                self.pages.pop();
            }
        } else if let Some(hash) = action
            .strip_prefix(view::HISTORY_OPEN_PREFIX)
            .and_then(|hex| u64::from_str_radix(hex, 16).ok())
        {
            let Some(entry) = self.history.get(hash) else {
                return;
            };
            let pasted = Pasted {
                card: 0,
                sender: entry.sender.clone(),
                fit: entry.fit.clone(),
                text: entry.text.clone(),
            };
            self.open_fit_dialog(host, pasted);
            return;
        } else {
            return;
        }
        self.show_panel(host);
    }

    fn page_start(&self) -> usize {
        self.pages.last().copied().unwrap_or(0)
    }

    fn show_panel(&self, host: &mut impl Host) {
        if !self.panel {
            return;
        }
        let tree = if self.expanded {
            let start = self.page_start();
            let page = view::Page {
                start,
                end: self.history.page_end(start),
                newer: self.pages.len() > 1,
            };
            let (now, offset) = host.now();
            view::panel_expanded(
                &self.strings,
                self.history.entries(),
                self.history_open,
                page,
                (now, offset),
            )
        } else {
            view::panel_inline(&self.strings, self.history.entries())
        };
        host.show_panel(tree);
    }

    /// A message from the page. Only `ready` means anything; the page is untrusted, so
    /// everything else is ignored.
    pub fn on_webview_message(&mut self, host: &mut impl Host, message: &str) {
        if message == WEBVIEW_READY && self.web {
            self.post_fit(host);
        }
    }

    fn post_fit(&self, host: &mut impl Host) {
        let Some(pasted) = &self.open else {
            return;
        };
        // The page reads English names only, and needs the drones apart from the cargo; Copy
        // fitting keeps the text as pasted.
        let fit = eft::separate_drones(&names::canonical(&pasted.text)).into_owned();
        let message = view::webview_message(&self.strings, &self.language, &fit);
        if let Err(e) = host.post_webview(&message) {
            host.warn(&format!(
                "could not send the fit to the fitting wheel: {e:?}"
            ));
        }
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
        self.show_panel(host);
        if let Some(pasted) = &self.open {
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
