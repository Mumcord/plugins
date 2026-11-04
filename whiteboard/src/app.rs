//! The plugin glue: Mumcord events in, sync engine and store in the middle, host calls out.
//!
//! Every host call goes through the [`Host`] trait, so the whole flow runs in native tests with
//! a recording fake (see `tests.rs`); `lib.rs` implements it with the SDK.
//!
//! How a board comes to life:
//! - The starter picks "Start whiteboard" (Apps menu) or types `/whiteboard`: the plugin starts a
//!   stage activity showing `web/board/index.html` and shows an invite card in chat. When the
//!   page is ready it gets the saved boards ("Continue …") or, with none saved, a new board.
//!   Opening a board announces it (BOARD) to the channel; continuing a saved one re-shares it.
//! - Everyone else sees an invite card with Join. After joining (card or stage header) the page
//!   waits for a BOARD from a participant (the host re-announces on every join), then loads the
//!   board from the others and merges its own saved copy of the same board, if any.
//! - A repeating timer (250 ms, the host minimum) drives the engine, autosave, the page
//!   outbox and announcements while a board is open.
//!
//! Host realities this file works around: the clock has a 1 s resolution, so time inside the
//! plugin advances by timer ticks; the page bridge takes at most 60 messages per second, so page
//! updates are coalesced and paced; and only one web view per plugin exists, so the side panel
//! mini view replaces the stage page while the stage tab is hidden and the stage page comes back
//! when it is visible again.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use mumcord_plugin_sdk::chat::PostOutcome;
use mumcord_plugin_sdk::events::{ActionSource, ActionValue, Event};
use mumcord_plugin_sdk::theme::Mode;
use mumcord_plugin_sdk::types::{Error, Peer};
use mumcord_plugin_sdk::ui::{self, ButtonKind, El, Slot, Tone};
use mumcord_plugin_sdk::webview::Place;

use crate::bridge::{
    self, BoardListEntry, BoardSettings, Init, Me, PageMsg, PaperSetting, PeerJson, PxRect,
    StatusJson, ToPage,
};
use crate::codec::{self, BoardId, BoardMsg, FORMAT, Msg};
use crate::dialogs::{self, Area, Ctx, Dialog, Effect, ExportReq, FileFormat, Line, View};
use crate::export::{self, Options, Paper, Scope};
use crate::model::{
    AuthorInfo, AuthorKey, Board, BoardField, Edit, Fields, Geom, Kind, MAX_NAME_CHARS, ObjId,
    Policy, Reject, z_after, z_before,
};
use crate::store::{self, Ago, Autosave, BoardFile, IndexEntry, Storage, StoreError};
use crate::sync::{
    Announce, Changed, Config, LocalError, Outgoing, PeerInfo, Session, SessionId, Target, Ui,
};
use crate::text::{self, clip, clip_to, t, t_with, tn};

/// The page on the stage and the side panel's mini view (paths inside `web/`).
pub const STAGE_ENTRY: &str = "board/index.html";
pub const MINI_ENTRY: &str = "board/mini.html";
/// Apps-menu item and slash command from the manifest.
pub const MENU_START: &str = "start";
pub const COMMAND: &str = "whiteboard";
/// Plain-text line announcement over `peers.messages`: `[FORMAT][which][hash u32 LE][board id]`,
/// so other Mumcord clients can hide the line behind the invite card (`chat.supersede`). It is
/// handled here, outside the board codec, because it is about chat, not the board.
pub const KIND_LINE: u8 = 0x20;
const LINE_LEN: usize = 1 + 1 + 4 + 16;
/// Timer interval; the host raises anything shorter to 250 ms.
pub const TICK_MS: u64 = 250;
/// Page bridge pacing: the host drops messages above 60 per second.
const PAGE_RATE_PER_S: f64 = 50.0;
const PAGE_BURST: f64 = 50.0;
/// Live points kept per pending stroke chunk and laser trail while the bridge is busy.
const MAX_PENDING_POINTS: usize = 2_000;
/// Screen-reader announcements: at most one per 2 s, a short queue of the newest.
const ANNOUNCE_EVERY_MS: u64 = 2_000;
const ANNOUNCE_QUEUE: usize = 3;
const MAX_ANNOUNCE_CHARS: usize = 300;
/// A host call needing a gesture must follow the click within 8 s.
const GESTURE_MS: u64 = 8_000;
const RENDER_TIMEOUT_MS: u64 = 15_000;
const UNDO_CLEAR_MS: u64 = 30_000;
/// A chat line is at most 500 characters.
const MAX_LINE_CHARS: usize = 480;
/// Offset of duplicated objects (quarter pixels).
const DUP_OFFSET: i32 = 64;
const MAX_INVITES: usize = 16;
const MAX_SEEN_BOARDS: usize = 64;
/// Storage key of the local paper choice (outside the `board/` prefix of saved boards).
const PAPER_KEY: &str = "paper";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Debug,
    Warn,
    Error,
}

/// Every host call the plugin makes. Storage is the plugin's key-value store.
pub trait Host: Storage {
    /// Wall clock in ms (1 s resolution on the real host).
    fn wall_ms(&mut self) -> u64;
    fn random(&mut self, len: u32) -> Option<Vec<u8>>;
    /// The channel, the local user first; empty without `events.presence`.
    fn presence(&mut self) -> Vec<PeerInfo>;
    fn peers_send(&mut self, target: Target, kind: u8, payload: &[u8]) -> Result<(), Error>;
    fn game_burst(&mut self) -> bool;
    fn relay_available(&mut self) -> bool;
    fn relay_send(&mut self, target: Target, payload: &[u8]) -> Result<(), Error>;
    fn stage_start(&mut self, title: &str, entry: &str) -> Result<String, Error>;
    fn stage_join(&mut self, activity: &str) -> Result<(), Error>;
    fn webview_show(&mut self, place: Place, entry: &str) -> Result<(), Error>;
    fn webview_post(&mut self, message: &str) -> Result<(), Error>;
    fn show(&mut self, slot: Slot, tree: El) -> Result<(), Error>;
    fn open_dialog(&mut self, title: &str) -> Result<(), Error>;
    fn close_dialog(&mut self);
    fn chat_send(&mut self, text: &str) -> Result<(), Error>;
    fn line_hash(&mut self, text: &str) -> u32;
    fn supersede(&mut self, card: &str, sender: SessionId, hash: u32) -> Result<(), Error>;
    fn post_image(&mut self, png: &[u8], alt: &str) -> Result<PostOutcome, Error>;
    fn save_file(&mut self, name: &str, data: &[u8]) -> Result<bool, Error>;
    /// A repeating timer.
    fn start_timer(&mut self, ms: u32) -> Option<u32>;
    fn cancel_timer(&mut self, id: u32);
    fn log(&mut self, level: Level, message: &str);
}

pub fn peer_info(p: &Peer) -> PeerInfo {
    PeerInfo {
        session: p.session,
        name: p.name.clone(),
        key: p.key.clone(),
    }
}

/// The stage activity the local user is in.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Activity {
    /// Empty when the user joined from the stage header before the plugin learned the id.
    id: String,
    mine: bool,
    starter: SessionId,
}

/// An invite card in chat for one board someone started.
#[derive(Clone, Debug)]
struct Invite {
    card: String,
    starter: SessionId,
    name: String,
    activity: Option<String>,
    board: Option<BoardMsg>,
    ended: bool,
}

/// The open board.
struct Open {
    s: Session,
    id: BoardId,
    started_ms: u64,
    autosave: Autosave,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SaveState {
    #[default]
    None,
    Saved,
    Full,
    Failed,
}

/// Points of one live stroke waiting for the page, and whether it ended.
type LiveChunk = (Vec<(i32, i32)>, bool);

/// Updates waiting for the page, coalesced until the bridge has room.
#[derive(Default)]
struct Outbox {
    tokens: f64,
    last_ms: u64,
    queue: VecDeque<String>,
    reset: bool,
    ids: BTreeSet<ObjId>,
    peers: bool,
    status: bool,
    cursors: BTreeMap<AuthorKey, (i32, i32, u8)>,
    view: Option<(AuthorKey, i32, i32, u32)>,
    live: BTreeMap<(AuthorKey, u16), LiveChunk>,
    laser: BTreeMap<AuthorKey, Vec<(i32, i32)>>,
}

struct Page {
    place: Place,
    ready: bool,
    out: Outbox,
}

impl Page {
    fn new(place: Place, now: u64) -> Page {
        Page {
            place,
            ready: false,
            out: Outbox {
                tokens: PAGE_BURST,
                last_ms: now,
                ..Outbox::default()
            },
        }
    }

    fn max_bytes(&self) -> usize {
        if self.place == Place::Stage {
            bridge::STAGE_MAX_BYTES
        } else {
            bridge::PANEL_MAX_BYTES
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Purpose {
    Export { name: String },
    Post { scale: u8 },
}

/// A PNG the page is rendering for a dialog click.
struct Render {
    req: u32,
    purpose: Purpose,
    area: Area,
    transparent: bool,
    parts: Vec<Option<String>>,
    /// When the click that asked for it happened (the host's gesture window starts there).
    clicked: u64,
    started: u64,
}

#[derive(Default)]
struct Announcer {
    last: Option<u64>,
    queue: VecDeque<String>,
}

impl Announcer {
    fn push(&mut self, text: String) {
        if self.queue.len() >= ANNOUNCE_QUEUE {
            self.queue.pop_front();
        }
        self.queue.push_back(clip_to(&text, MAX_ANNOUNCE_CHARS));
    }

    fn take(&mut self, now: u64) -> Option<String> {
        if self.last.is_some_and(|l| now < l + ANNOUNCE_EVERY_MS) {
            return None;
        }
        let text = self.queue.pop_front()?;
        self.last = Some(now);
        Some(text)
    }
}

#[derive(Default)]
pub struct App {
    /// Granted permissions; `None` until the host reports them (then assume the manifest's).
    granted: Option<BTreeSet<String>>,
    dark: bool,
    reduced_motion: bool,
    /// Plugin time in ms: the wall clock, advanced by timer ticks between its 1 s steps.
    now: u64,
    timer: Option<u32>,
    me: Option<PeerInfo>,
    presence: Vec<PeerInfo>,
    relay: bool,
    activity: Option<Activity>,
    /// Activities others started that the user may join: id -> starter.
    activities: BTreeMap<String, SessionId>,
    pending_join: Option<String>,
    participants: BTreeSet<SessionId>,
    invites: Vec<Invite>,
    invite_seq: u32,
    seen_boards: BTreeMap<SessionId, BoardMsg>,
    open: Option<Open>,
    /// A saved board to open once the page is ready (chosen in the boards dialog).
    pending_open: Option<BoardId>,
    page: Option<Page>,
    stage_hidden: bool,
    side_closed: bool,
    dialog: Option<Dialog>,
    render: Option<Render>,
    render_seq: u32,
    announcer: Announcer,
    talking: BTreeSet<SessionId>,
    view: Option<PxRect>,
    selection: Vec<ObjId>,
    paper: Option<PaperSetting>,
    clear_undo_until: Option<u64>,
    index: Option<Vec<IndexEntry>>,
    save_state: SaveState,
    /// Page messages dropped by validation.
    dropped_page: u64,
}

fn error_text(e: Error) -> String {
    format!("{e:?}")
}

impl App {
    fn has(&self, permission: &str) -> bool {
        self.granted.as_ref().is_none_or(|g| g.contains(permission))
    }

    #[cfg(test)]
    pub fn dropped_page_messages(&self) -> u64 {
        self.dropped_page
    }

    // ---- events ---------------------------------------------------------------------------------

    pub fn on_event<H: Host>(&mut self, host: &mut H, event: Event) {
        let wall = host.wall_ms();
        self.now = if matches!(event, Event::Timer(id) if Some(id) == self.timer) {
            (self.now + TICK_MS).max(wall)
        } else {
            self.now.max(wall)
        };
        let language_changed = text::on_event(&event);
        match event {
            Event::SettingsChanged(s) => {
                self.granted = Some(s.granted.into_iter().collect());
                self.check_relay(host);
            }
            Event::LanguageChanged(_) => {
                if language_changed {
                    self.refresh_texts(host);
                }
            }
            Event::ThemeChanged(info) => {
                self.dark = info.mode == Mode::Dark;
                self.reduced_motion = info.reduced_motion;
                self.queue_init(host);
            }
            Event::AppMenuActivated(id) => {
                if id == MENU_START {
                    self.start(host);
                }
            }
            Event::UiAction(a) => match a.source {
                ActionSource::Command => {
                    if a.action == COMMAND {
                        self.start(host);
                    }
                }
                ActionSource::Slot(Slot::Dialog) => self.dialog_action(host, &a.action, &a.value),
                ActionSource::Slot(Slot::ChatCard(card)) => {
                    self.card_action(host, &card, &a.action)
                }
                ActionSource::Slot(_) => {}
            },
            Event::DialogClosed => {
                self.dialog = None;
                self.render = None;
            }
            Event::StageActivityStarted((peer, id)) => self.activity_started(host, &peer, id),
            Event::StageActivityEnded(id) => self.activity_ended(host, &id),
            Event::StageJoined(peer) => self.stage_joined(host, &peer),
            Event::StageLeft(peer) => self.stage_left(host, &peer),
            Event::StageVisibilityChanged(visible) => self.stage_visibility(host, visible),
            Event::SurfaceClosed(place) => self.surface_closed(place),
            Event::PeerMessage(m) => {
                let from = peer_info(&m.sender);
                self.peer_message(host, &from, m.kind, &m.payload, m.direct);
            }
            Event::RelayMessage((peer, data)) => self.relay_message(host, &peer_info(&peer), &data),
            Event::RelayStatusChanged(_) => self.check_relay(host),
            Event::Joined(_) | Event::PeerReady(_) => self.presence_changed(host),
            Event::Left(p) => {
                self.talking.remove(&p.session);
                self.participants.remove(&p.session);
                if let Some(o) = &mut self.open {
                    let out = o.s.on_peer_left(p.session, self.now);
                    self.run(host, out, None);
                }
                self.presence_changed(host);
            }
            Event::Talking((p, on)) => {
                let changed = if on {
                    self.talking.insert(p.session)
                } else {
                    self.talking.remove(&p.session)
                };
                if changed && let Some(page) = &mut self.page {
                    page.out.peers = true;
                }
            }
            Event::WebviewMessage(message) => self.page_message(host, &message),
            Event::Timer(id) if Some(id) == self.timer => self.tick(host),
            _ => {}
        }
        self.flush_announcements();
        self.flush_page(host);
    }

    fn refresh_me<H: Host>(&mut self, host: &mut H) {
        let list = host.presence();
        if let Some(first) = list.first() {
            self.me = Some(first.clone());
        }
        self.presence = list;
    }

    fn my_session(&self) -> Option<SessionId> {
        self.me.as_ref().map(|m| m.session)
    }

    fn check_relay<H: Host>(&mut self, host: &mut H) {
        let on = self.has("peers.relay") && host.relay_available();
        if on != self.relay {
            self.relay = on;
            if let Some(o) = &mut self.open {
                let out = o.s.set_relay_available(on, self.now);
                self.run(host, out, None);
            }
        }
    }

    fn presence_changed<H: Host>(&mut self, host: &mut H) {
        self.refresh_me(host);
        if let Some(o) = &mut self.open {
            let out = o.s.on_presence(&self.presence, self.now);
            self.run(host, out, None);
        }
    }

    fn ensure_timer<H: Host>(&mut self, host: &mut H) {
        if self.timer.is_none() {
            self.timer = host.start_timer(TICK_MS as u32);
            if self.timer.is_none() {
                host.log(
                    Level::Warn,
                    "no timer: the board will only update on events",
                );
            }
        }
    }

    fn stop_timer<H: Host>(&mut self, host: &mut H) {
        if let Some(id) = self.timer.take() {
            host.cancel_timer(id);
        }
    }

    // ---- starting, joining, leaving -------------------------------------------------------------

    /// "Start whiteboard" or `/whiteboard` (both are user gestures).
    fn start<H: Host>(&mut self, host: &mut H) {
        if self.activity.is_some() {
            // Already on a board: bring the board page back to the stage.
            if self.page.as_ref().is_none_or(|p| p.place != Place::Stage) {
                self.show_stage(host);
            }
            return;
        }
        self.refresh_me(host);
        match host.stage_start(&clip_to(&t("stage.title"), 64), STAGE_ENTRY) {
            Ok(id) => {
                let starter = self.my_session().unwrap_or(0);
                self.activity = Some(Activity {
                    id,
                    mine: true,
                    starter,
                });
                self.participants = BTreeSet::from([starter]);
                self.page = Some(Page::new(Place::Stage, self.now));
                self.ensure_timer(host);
                let card = self.invite_for(starter);
                self.invites[card].name = self
                    .me
                    .as_ref()
                    .map_or_else(String::new, |m| m.name.clone());
                self.show_invite(host, card);
            }
            Err(e) => {
                host.log(Level::Warn, &format!("could not start the stage: {e:?}"));
                let reason = match e {
                    Error::QuotaExceeded => t("notice.start.busy"),
                    Error::NotPermitted => t("notice.start.notPermitted"),
                    _ => t_with("notice.start.other", &[("error", &error_text(e))]),
                };
                self.open_dialog(host, Dialog::notice(&t("notice.start.title"), vec![reason]));
            }
        }
    }

    fn activity_started<H: Host>(&mut self, host: &mut H, peer: &Peer, id: String) {
        if Some(peer.session) == self.my_session() {
            return;
        }
        if self.activities.len() >= MAX_INVITES {
            self.activities.pop_first();
        }
        self.activities.insert(id.clone(), peer.session);
        let i = match self
            .invites
            .iter()
            .position(|v| v.starter == peer.session && v.activity.is_none() && !v.ended)
        {
            Some(i) => i,
            None => self.invite_for(peer.session),
        };
        self.invites[i].activity = Some(id);
        self.invites[i].name = peer.name.clone();
        self.show_invite(host, i);
    }

    fn activity_ended<H: Host>(&mut self, host: &mut H, id: &str) {
        self.activities.remove(id);
        for i in 0..self.invites.len() {
            if self.invites[i].activity.as_deref() == Some(id) {
                self.invites[i].ended = true;
                self.show_invite(host, i);
            }
        }
        if self.activity.as_ref().is_some_and(|a| a.id == id) {
            self.close_board(host);
        }
    }

    fn stage_joined<H: Host>(&mut self, host: &mut H, peer: &Peer) {
        if self.me.is_none() {
            self.refresh_me(host);
        }
        if Some(peer.session) == self.my_session() {
            if self.activity.is_none() {
                // Joined with a card's Join or the stage header's Join button.
                let id = self.pending_join.take().or_else(|| {
                    (self.activities.len() == 1)
                        .then(|| self.activities.keys().next().cloned())
                        .flatten()
                });
                let starter = id
                    .as_ref()
                    .and_then(|id| self.activities.get(id).copied())
                    .unwrap_or(0);
                self.activity = Some(Activity {
                    id: id.unwrap_or_default(),
                    mine: false,
                    starter,
                });
                self.participants = BTreeSet::from([peer.session, starter]);
                self.page = Some(Page::new(Place::Stage, self.now));
                self.ensure_timer(host);
                if let Some(b) = self.seen_boards.get(&starter).cloned() {
                    self.adopt(host, b);
                }
                for i in 0..self.invites.len() {
                    self.show_invite(host, i);
                }
            }
            return;
        }
        self.participants.insert(peer.session);
        // The joiner needs the board id: the host says it again.
        if let Some(o) = &mut self.open
            && o.s.effective_host() == Some(o.s.me())
        {
            let out = o.s.announce_board(o.started_ms, self.now);
            self.run(host, out, None);
        }
    }

    fn stage_left<H: Host>(&mut self, host: &mut H, peer: &Peer) {
        if Some(peer.session) == self.my_session() {
            self.close_board(host);
            for i in 0..self.invites.len() {
                self.show_invite(host, i);
            }
        } else {
            self.participants.remove(&peer.session);
        }
    }

    /// Leaves the board locally: saves it, closes the page, dialogs and the timer.
    fn close_board<H: Host>(&mut self, host: &mut H) {
        if let Some(o) = &self.open
            && o.autosave.is_dirty()
        {
            self.save_now(host);
        }
        self.open = None;
        self.activity = None;
        self.participants.clear();
        self.page = None;
        self.render = None;
        self.view = None;
        self.selection.clear();
        self.clear_undo_until = None;
        if self
            .dialog
            .as_ref()
            .is_some_and(|d| !matches!(d.view, View::Boards { .. } | View::Notice { .. }))
        {
            host.close_dialog();
            self.dialog = None;
        }
        self.stop_timer(host);
    }

    fn show_stage<H: Host>(&mut self, host: &mut H) {
        match host.webview_show(Place::Stage, STAGE_ENTRY) {
            Ok(()) => self.page = Some(Page::new(Place::Stage, self.now)),
            Err(e) => host.log(Level::Warn, &format!("could not show the board: {e:?}")),
        }
    }

    fn stage_visibility<H: Host>(&mut self, host: &mut H, visible: bool) {
        self.stage_hidden = !visible;
        if self.activity.is_none() {
            return;
        }
        if visible {
            if self.page.as_ref().is_none_or(|p| p.place != Place::Stage) {
                self.show_stage(host);
            }
        } else if self.has("ui.side-panel") && !self.side_closed && self.open.is_some() {
            // One web view per plugin: the mini view takes the stage page's place until the stage
            // tab is visible again.
            match host.webview_show(Place::SidePanel, MINI_ENTRY) {
                Ok(()) => self.page = Some(Page::new(Place::SidePanel, self.now)),
                Err(e) => host.log(Level::Debug, &format!("no side panel: {e:?}")),
            }
        }
    }

    fn surface_closed(&mut self, place: Place) {
        if self.page.as_ref().is_some_and(|p| p.place == place) {
            self.page = None;
        }
        if place == Place::SidePanel {
            self.side_closed = true;
        }
    }

    // ---- boards ---------------------------------------------------------------------------------

    fn index<H: Host>(&mut self, host: &mut H) -> &[IndexEntry] {
        if self.index.is_none() {
            let list = store::read_index(host).unwrap_or_else(|e| {
                host.log(Level::Warn, &format!("saved boards unreadable: {e:?}"));
                Vec::new()
            });
            self.index = Some(list);
        }
        self.index.as_deref().unwrap_or_default()
    }

    fn new_author<H: Host>(&mut self, host: &mut H) -> Option<AuthorKey> {
        let b = host.random(4)?;
        let key = u32::from_le_bytes(b.get(..4)?.try_into().ok()?);
        // Zero stays free as "nobody" on the page.
        Some(key.max(1))
    }

    fn new_board<H: Host>(&mut self, host: &mut H, title: &str) {
        let id: Option<BoardId> = host.random(16).and_then(|b| b.try_into().ok());
        let (Some(id), Some(author)) = (id, self.new_author(host)) else {
            host.log(Level::Error, "no random bytes: cannot create a board");
            self.open_dialog(
                host,
                Dialog::notice(&t("notice.start.title"), vec![t("notice.noRandom")]),
            );
            return;
        };
        let title = if title.trim().is_empty() {
            t("board.untitled")
        } else {
            title.trim().to_owned()
        };
        let started = host.wall_ms();
        self.open_board(host, id, Board::new(author), Some(&title), started, true);
    }

    /// Opens a board saved on this device and re-shares it with the channel.
    fn open_saved<H: Host>(&mut self, host: &mut H, id: BoardId) {
        let Some(author) = self.new_author(host) else {
            host.log(Level::Error, "no random bytes: cannot open a board");
            return;
        };
        match store::load_board(host, &id) {
            Ok(file) => {
                let (board, rejected) = file.into_board(author);
                if rejected > 0 {
                    host.log(
                        Level::Warn,
                        &format!("{rejected} saved changes were not valid"),
                    );
                }
                let started = host.wall_ms();
                self.open_board(host, id, board, None, started, true);
            }
            Err(e) => {
                host.log(Level::Warn, &format!("could not open a saved board: {e:?}"));
                self.open_dialog(
                    host,
                    Dialog::notice(&t("notice.open.title"), vec![t("notice.open.failed")]),
                );
            }
        }
    }

    /// Opens the board another participant announced, merged with an own saved copy.
    fn adopt<H: Host>(&mut self, host: &mut H, b: BoardMsg) {
        let Some(author) = self.new_author(host) else {
            host.log(Level::Error, "no random bytes: cannot join the board");
            return;
        };
        let board = match store::load_board(host, &b.board) {
            Ok(file) => file.into_board(author).0,
            Err(StoreError::NotFound) => Board::new(author),
            Err(e) => {
                host.log(Level::Warn, &format!("own copy unreadable: {e:?}"));
                Board::new(author)
            }
        };
        self.open_board(host, b.board, board, None, b.started_ms, false);
    }

    fn open_board<H: Host>(
        &mut self,
        host: &mut H,
        id: BoardId,
        board: Board,
        init_title: Option<&str>,
        started_ms: u64,
        announce: bool,
    ) {
        self.refresh_me(host);
        self.check_relay(host);
        let me = self.me.clone().unwrap_or(PeerInfo {
            session: 0,
            name: String::new(),
            key: None,
        });
        let author = board.me();
        let info = AuthorInfo {
            name: me.name.chars().take(MAX_NAME_CHARS).collect(),
            cert_key: me.key.clone(),
            shape: (author % 4) as u8,
            color: ((author >> 8) % 6) as u8,
        };
        let cfg = Config {
            board: id,
            session: me.session,
            info,
            joined_at_ms: host.wall_ms(),
            relay: self.relay,
        };
        let now = self.now;
        let mut s = Session::new(cfg, board, now);
        let mut out = s.on_presence(&self.presence, now);
        out.extend(s.start(now));
        if let Some(title) = init_title {
            match s.init_board(title, now) {
                Ok(more) => out.extend(more),
                Err(e) => host.log(Level::Warn, &format!("could not set up the board: {e:?}")),
            }
        }
        if announce {
            out.extend(s.announce_board(started_ms, now));
        }
        self.open = Some(Open {
            s,
            id,
            started_ms,
            autosave: Autosave::default(),
        });
        self.save_state = SaveState::None;
        self.run(host, out, None);
        self.queue_init(host);
        self.ensure_timer(host);
    }

    fn save_now<H: Host>(&mut self, host: &mut H) {
        let Some(o) = &self.open else {
            return;
        };
        let file = BoardFile::from_board(o.id, o.s.board(), host.wall_ms());
        let result = store::save_board(host, &file);
        let now = self.now;
        let Some(o) = &mut self.open else {
            return;
        };
        self.save_state = match result {
            Ok(_) => {
                o.autosave.saved();
                SaveState::Saved
            }
            Err(e) => {
                o.autosave.failed(now);
                host.log(Level::Warn, &format!("autosave failed: {e:?}"));
                if matches!(e, StoreError::Quota { .. }) {
                    SaveState::Full
                } else {
                    SaveState::Failed
                }
            }
        };
        self.index = None;
        if let Some(page) = &mut self.page {
            page.out.status = true;
        }
    }

    // ---- the engine -----------------------------------------------------------------------------

    fn tick<H: Host>(&mut self, host: &mut H) {
        let now = self.now;
        if let Some(o) = &mut self.open {
            let out = o.s.tick(now);
            self.run(host, out, None);
        }
        if self
            .open
            .as_ref()
            .is_some_and(|o| o.autosave.should_save(now))
        {
            self.save_now(host);
        }
        if self
            .render
            .as_ref()
            .is_some_and(|r| now.saturating_sub(r.started) > RENDER_TIMEOUT_MS)
        {
            self.render = None;
            self.dialog_note(host, Tone::Danger, t("dialog.render.timeout"));
        }
        if self.clear_undo_until.is_some_and(|u| now >= u) {
            self.clear_undo_until = None;
            if self
                .dialog
                .as_ref()
                .is_some_and(|d| matches!(d.view, View::Cleared { .. }))
            {
                host.close_dialog();
                self.dialog = None;
            }
        }
    }

    /// Carries out what the engine asked for. `from` is the sender of the message being
    /// handled, if any.
    fn run<H: Host>(&mut self, host: &mut H, out: Vec<Outgoing>, from: Option<&PeerInfo>) {
        let mut queue: VecDeque<Outgoing> = out.into();
        while let Some(o) = queue.pop_front() {
            match o {
                Outgoing::PeersSend {
                    kind,
                    payload,
                    target,
                } => {
                    if let Err(e) = host.peers_send(target, kind, &payload) {
                        host.log(Level::Debug, &format!("board message not sent: {e:?}"));
                    }
                }
                Outgoing::RelaySend { payload, target } => {
                    if let Err(e) = host.relay_send(target, &payload) {
                        host.log(Level::Debug, &format!("relay message not sent: {e:?}"));
                    }
                }
                Outgoing::StartGameBurst => {
                    let ok = host.game_burst();
                    if let Some(o) = &mut self.open {
                        queue.extend(o.s.game_burst_result(ok, self.now));
                    }
                }
                Outgoing::Ui(ui) => self.on_ui(host, ui, from),
            }
        }
    }

    fn on_ui<H: Host>(&mut self, host: &mut H, ui: Ui, from: Option<&PeerInfo>) {
        if let Ui::BoardSeen(b) = ui {
            if let Some(from) = from {
                self.board_seen(host, from, b);
            }
            return;
        }
        if let Ui::Announce(a) = &ui {
            let text = self.announce_text(a);
            self.announcer.push(text);
            return;
        }
        if matches!(ui, Ui::Objects(_)) {
            let now = self.now;
            if let Some(o) = &mut self.open {
                o.autosave.changed(now);
            }
        }
        let Some(page) = &mut self.page else {
            return;
        };
        let out = &mut page.out;
        match ui {
            Ui::Objects(Changed::All) => {
                out.reset = true;
                out.ids.clear();
            }
            Ui::Objects(Changed::Ids(ids)) => {
                if !out.reset {
                    out.ids.extend(ids);
                }
            }
            Ui::Peers => out.peers = true,
            Ui::Status => out.status = true,
            Ui::Cursor {
                author,
                x,
                y,
                status,
            } => {
                out.cursors.insert(author, (x, y, status));
            }
            Ui::Live {
                author,
                sid,
                points,
                done,
            } => {
                let e = out.live.entry((author, sid)).or_default();
                if e.0.len() + points.len() <= MAX_PENDING_POINTS {
                    e.0.extend(points);
                }
                e.1 |= done;
            }
            Ui::Laser { author, points } => {
                let e = out.laser.entry(author).or_default();
                e.extend(points);
                if e.len() > MAX_PENDING_POINTS {
                    let cut = e.len() - MAX_PENDING_POINTS;
                    e.drain(..cut);
                }
            }
            Ui::View {
                author,
                x,
                y,
                zoom_permille,
            } => out.view = Some((author, x, y, zoom_permille)),
            Ui::Announce(_) | Ui::BoardSeen(_) => {}
        }
        // The settings dialog lists people and the host state.
        if self
            .dialog
            .as_ref()
            .is_some_and(|d| matches!(d.view, View::Settings { .. }))
        {
            self.redraw_dialog(host);
        }
    }

    fn name_of(&self, author: AuthorKey) -> String {
        let Some(o) = &self.open else {
            return t("person.someone");
        };
        if author == o.s.me() {
            return t("person.you");
        }
        o.s.board()
            .author(author)
            .map(|a| a.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| t("person.someone"))
    }

    fn announce_text(&self, a: &Announce) -> String {
        match a {
            Announce::Joined(k) => t_with("announce.joined", &[("name", &self.name_of(*k))]),
            Announce::Left(k) => t_with("announce.left", &[("name", &self.name_of(*k))]),
            Announce::HostChanged(k) => {
                if self.open.as_ref().is_some_and(|o| o.s.me() == *k) {
                    t("announce.youHost")
                } else {
                    t_with("announce.host", &[("name", &self.name_of(*k))])
                }
            }
            Announce::Added { author, count } => tn(
                "announce.added",
                u64::from(*count),
                &[("name", &self.name_of(*author))],
            ),
            Announce::Presenting(Some(k)) => {
                t_with("announce.presenting", &[("name", &self.name_of(*k))])
            }
            Announce::Presenting(None) => t("announce.presentingEnded"),
        }
    }

    fn flush_announcements(&mut self) {
        if let Some(page) = &mut self.page
            && page.ready
            && let Some(text) = self.announcer.take(self.now)
        {
            page.out
                .queue
                .push_back(ToPage::Announce { text }.to_json());
        }
    }

    // ---- messages from other users --------------------------------------------------------------

    fn peer_message<H: Host>(
        &mut self,
        host: &mut H,
        from: &PeerInfo,
        kind: u8,
        payload: &[u8],
        direct: bool,
    ) {
        if kind == KIND_LINE {
            self.on_line(host, from, payload);
            return;
        }
        if let Some(o) = &mut self.open {
            let out = o.s.on_peer_message(from, kind, payload, direct, self.now);
            self.run(host, out, Some(from));
        } else if let Ok(Msg::Board(b)) = codec::decode_peers(kind, payload) {
            self.board_seen(host, from, b);
        }
    }

    fn relay_message<H: Host>(&mut self, host: &mut H, from: &PeerInfo, data: &[u8]) {
        if let Some(o) = &mut self.open {
            let out = o.s.on_relay_message(from, data, self.now);
            self.run(host, out, Some(from));
        } else if let Ok(Msg::Board(b)) = codec::decode_relay(data) {
            self.board_seen(host, from, b);
        }
    }

    fn board_seen<H: Host>(&mut self, host: &mut H, from: &PeerInfo, b: BoardMsg) {
        if Some(from.session) == self.my_session() {
            return;
        }
        if self.seen_boards.len() >= MAX_SEEN_BOARDS
            && !self.seen_boards.contains_key(&from.session)
        {
            self.seen_boards.pop_first();
        }
        self.seen_boards.insert(from.session, b.clone());
        let i = match self
            .invites
            .iter()
            .position(|v| v.board.as_ref().is_some_and(|x| x.board == b.board))
            .or_else(|| {
                self.invites
                    .iter()
                    .position(|v| v.starter == from.session && v.board.is_none() && !v.ended)
            }) {
            Some(i) => i,
            None => self.invite_for(from.session),
        };
        if self.invites[i].name.is_empty() {
            self.invites[i].name = from.name.clone();
        }
        self.invites[i].board = Some(b.clone());
        self.show_invite(host, i);
        if let Some(a) = &self.activity
            && !a.mine
            && self.open.is_none()
            && (a.starter == from.session || self.participants.contains(&from.session))
        {
            self.adopt(host, b);
        }
    }

    /// Another Mumcord's plain-text line: hide it behind that person's invite card.
    fn on_line<H: Host>(&mut self, host: &mut H, from: &PeerInfo, payload: &[u8]) {
        if payload.len() != LINE_LEN || payload[0] != FORMAT || payload[1] > 1 {
            host.log(Level::Debug, "dropped a malformed chat line notice");
            return;
        }
        if !(self.has("chat.supersedes") && self.has("ui.chat-card")) {
            return;
        }
        let hash = u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]);
        let i = match self
            .invites
            .iter()
            .rposition(|v| v.starter == from.session && !v.ended)
        {
            Some(i) => i,
            None => self.invite_for(from.session),
        };
        if self.invites[i].name.is_empty() {
            self.invites[i].name = from.name.clone();
        }
        self.show_invite(host, i);
        let card = self.invites[i].card.clone();
        if let Err(e) = host.supersede(&card, from.session, hash) {
            host.log(
                Level::Debug,
                &format!("could not hide the chat line: {e:?}"),
            );
        }
    }

    // ---- invite cards ---------------------------------------------------------------------------

    /// A new invite card for a board `starter` started; returns its index.
    fn invite_for(&mut self, starter: SessionId) -> usize {
        if self.invites.len() >= MAX_INVITES {
            self.invites.remove(0);
        }
        self.invite_seq += 1;
        self.invites.push(Invite {
            card: format!("wb-{}", self.invite_seq),
            starter,
            name: String::new(),
            activity: None,
            board: None,
            ended: false,
        });
        self.invites.len() - 1
    }

    fn show_invite<H: Host>(&mut self, host: &mut H, i: usize) {
        if !self.has("ui.chat-card") {
            return;
        }
        let Some(v) = self.invites.get(i) else {
            return;
        };
        let mine = Some(v.starter) == self.my_session();
        let joined = self
            .activity
            .as_ref()
            .is_some_and(|a| v.activity.as_ref().is_some_and(|id| *id == a.id) || (a.mine && mine));
        let name = clip_to(&v.name, 40);
        let mut col = ui::column();
        if v.ended {
            col = col.child(ui::text(&t("card.ended")));
        } else {
            col = col.child(ui::text(&clip(&if mine {
                t("card.youStarted")
            } else {
                t_with("card.started", &[("name", &name)])
            })));
        }
        if let Some(b) = &v.board
            && !b.title.is_empty()
        {
            col = col.child(ui::caption(&clip(&b.title)));
        }
        if !v.ended {
            if joined {
                col = col.child(ui::chip(&t("card.joined")).tone(Tone::Success));
            } else if v.activity.is_some() && !mine {
                col = col
                    .child(ui::caption(&t("card.hint")))
                    .child(ui::button("join", &t("card.join")).kind(ButtonKind::Primary));
            } else {
                col = col.child(ui::caption(&t("card.hint")));
            }
        }
        let card = v.card.clone();
        if let Err(e) = host.show(Slot::ChatCard(card), col) {
            host.log(
                Level::Debug,
                &format!("could not show the invite card: {e:?}"),
            );
        }
    }

    fn card_action<H: Host>(&mut self, host: &mut H, card: &str, action: &str) {
        if action != "join" {
            return;
        }
        let Some(v) = self.invites.iter().find(|v| v.card == card) else {
            return;
        };
        let Some(id) = v.activity.clone() else {
            return;
        };
        if v.ended || self.activity.is_some() {
            return;
        }
        match host.stage_join(&id) {
            Ok(()) => self.pending_join = Some(id),
            Err(e) => {
                host.log(
                    Level::Warn,
                    &format!("could not join the whiteboard: {e:?}"),
                );
                let reason = match e {
                    Error::QuotaExceeded => t("notice.join.full"),
                    Error::Invalid => t("notice.join.ended"),
                    _ => t_with("notice.start.other", &[("error", &error_text(e))]),
                };
                self.open_dialog(host, Dialog::notice(&t("notice.join.title"), vec![reason]));
            }
        }
    }

    // ---- messages from the page -----------------------------------------------------------------

    fn page_message<H: Host>(&mut self, host: &mut H, text: &str) {
        let Some(page) = &self.page else {
            return;
        };
        let msg = match bridge::parse(text, page.max_bytes()) {
            Ok(m) => m,
            Err(e) => {
                self.dropped_page += 1;
                host.log(Level::Debug, &format!("dropped a page message: {e:?}"));
                return;
            }
        };
        let now = self.now;
        match msg {
            PageMsg::Ready => self.page_ready(host),
            PageMsg::OpenStage => self.show_stage(host),
            PageMsg::Png {
                req,
                part,
                of,
                data,
            } => self.png_part(host, req, part, of, data),
            PageMsg::Menu {
                item,
                view,
                selection,
            } => {
                self.view = view;
                self.selection = selection;
                self.menu(host, item);
            }
            PageMsg::Open(id) => {
                if self.activity.as_ref().is_some_and(|a| a.mine) && self.open.is_none() {
                    self.open_saved(host, id);
                }
            }
            PageMsg::NewBoard(title) => {
                if self.activity.as_ref().is_some_and(|a| a.mine) && self.open.is_none() {
                    self.new_board(host, &title);
                }
            }
            PageMsg::DeleteBoard(id) => {
                self.open_dialog(
                    host,
                    Dialog::new(View::Boards {
                        confirm_delete: Some(id),
                    }),
                );
            }
            PageMsg::Settings {
                policy,
                allow,
                laser,
                paper,
            } => {
                if let Some(p) = paper {
                    self.set_paper(host, p);
                }
                let mut fields = Vec::new();
                fields.extend(policy.map(BoardField::Policy));
                fields.extend(allow.map(|(k, on)| BoardField::Allowed(k, on)));
                fields.extend(laser.map(BoardField::LaserForViewers));
                if !fields.is_empty() {
                    self.board_settings(host, fields);
                }
            }
            other => {
                let Some(o) = &mut self.open else {
                    return;
                };
                let pointer = matches!(
                    other,
                    PageMsg::Cursor(_)
                        | PageMsg::Live { .. }
                        | PageMsg::Laser(_)
                        | PageMsg::View { .. }
                        | PageMsg::Follow(_)
                );
                let result = match other {
                    PageMsg::Create(n) => {
                        let z = top_z(o);
                        let fields = Fields {
                            geom: Some(n.geom),
                            ink: n.ink,
                            width: n.width,
                            fill: n.fill,
                            text: n.text,
                            z,
                            ..Fields::default()
                        };
                        o.s.local_edit(
                            vec![Edit::Create {
                                kind: n.kind,
                                base: n.base,
                                fields,
                            }],
                            now,
                        )
                    }
                    PageMsg::Set { id, fields } => o.s.local_edit(
                        vec![Edit::Set {
                            id,
                            fields: Fields {
                                geom: fields.geom,
                                ink: fields.ink,
                                width: fields.width,
                                fill: fields.fill,
                                text: fields.text,
                                z: fields.z,
                                alt: fields.alt,
                                deleted: None,
                            },
                        }],
                        now,
                    ),
                    PageMsg::Delete(ids) => {
                        let edits = known(o, &ids)
                            .map(|id| Edit::Set {
                                id,
                                fields: Fields {
                                    deleted: Some(true),
                                    ..Fields::default()
                                },
                            })
                            .collect();
                        o.s.local_edit(edits, now)
                    }
                    PageMsg::Front(ids) => {
                        let edits = restack(o, &ids, true);
                        o.s.local_edit(edits, now)
                    }
                    PageMsg::Back(ids) => {
                        let edits = restack(o, &ids, false);
                        o.s.local_edit(edits, now)
                    }
                    PageMsg::Dup(ids) => {
                        let edits = duplicates(o, &ids);
                        o.s.local_edit(edits, now)
                    }
                    PageMsg::Undo => o.s.undo(now),
                    PageMsg::Redo => o.s.redo(now),
                    PageMsg::Present(on) => o.s.set_presenting(on, now),
                    PageMsg::Follow(on) => Ok(o.s.set_following(on, now)),
                    PageMsg::Cursor(Some((x, y, status))) => {
                        Ok(o.s.local_cursor(x, y, status, 0, now))
                    }
                    // The engine has no "cursor gone"; it simply stops sending.
                    PageMsg::Cursor(None) => Ok(Vec::new()),
                    PageMsg::Live { sid, points, done } => {
                        Ok(o.s.local_live(sid, &points, done, now))
                    }
                    PageMsg::Laser(points) => Ok(o.s.local_laser(&points, now)),
                    PageMsg::View {
                        x,
                        y,
                        zoom_permille,
                    } => Ok(o.s.local_view(x, y, zoom_permille, now)),
                    PageMsg::Hide { author, on } => Ok(o.s.set_hidden(author, on, now)),
                    PageMsg::RemoveAuthor(a) => o.s.remove_author(a, now),
                    PageMsg::Handover(a) => o.s.handover(a, now),
                    _ => Ok(Vec::new()),
                };
                match result {
                    Ok(out) => {
                        if !pointer {
                            // Any newer own action makes "Undo clear" undo something else.
                            self.clear_undo_until = None;
                        }
                        self.run(host, out, None);
                    }
                    Err(e) => self.local_refused(host, e),
                }
            }
        }
    }

    fn local_refused<H: Host>(&mut self, host: &mut H, e: LocalError) {
        host.log(Level::Debug, &format!("local change refused: {e:?}"));
        let text = match e {
            LocalError::NotAllowed => t("announce.notAllowed"),
            LocalError::Model(Reject::TooManyObjects | Reject::StateTooLarge) => {
                t("announce.boardFull")
            }
            LocalError::Model(_) => return,
        };
        self.announcer.push(text);
    }

    fn page_ready<H: Host>(&mut self, host: &mut H) {
        let now = self.now;
        let Some(page) = &mut self.page else {
            return;
        };
        page.ready = true;
        page.out = Outbox {
            tokens: PAGE_BURST,
            last_ms: now,
            ..Outbox::default()
        };
        if self.open.is_none()
            && self.activity.as_ref().is_some_and(|a| a.mine)
            && let Some(id) = self.pending_open.take()
        {
            self.open_saved(host, id);
            return;
        }
        self.queue_init(host);
        if self.open.is_none() && self.activity.as_ref().is_some_and(|a| a.mine) {
            if self.index(host).is_empty() {
                self.new_board(host, "");
            } else {
                self.queue_boards(host);
            }
        }
    }

    // ---- page output ----------------------------------------------------------------------------

    fn paper<H: Host>(&mut self, host: &mut H) -> PaperSetting {
        if let Some(p) = self.paper {
            return p;
        }
        let p = match host.get(PAPER_KEY) {
            Ok(Some(v)) => std::str::from_utf8(&v)
                .ok()
                .and_then(PaperSetting::from_name)
                .unwrap_or_default(),
            _ => PaperSetting::Auto,
        };
        self.paper = Some(p);
        p
    }

    fn set_paper<H: Host>(&mut self, host: &mut H, p: PaperSetting) {
        self.paper = Some(p);
        if let Err(e) = host.set(PAPER_KEY, p.name().as_bytes()) {
            host.log(Level::Debug, &format!("paper choice not saved: {e}"));
        }
        if let Some(page) = &mut self.page {
            page.out.status = true;
        }
    }

    /// Queues `init` (and everything after it) for a ready page.
    fn queue_init<H: Host>(&mut self, host: &mut H) {
        if !self.page.as_ref().is_some_and(|p| p.ready) {
            return;
        }
        let paper = self.paper(host);
        let me = self.me.clone();
        let (me_json, board, settings) = match &self.open {
            Some(o) => {
                let a = o.s.board().author(o.s.me()).cloned();
                let settings = o.s.board().settings();
                let status = o.s.status();
                (
                    Me {
                        author: o.s.me(),
                        name: a.as_ref().map_or_else(String::new, |a| a.name.clone()),
                        shape: a.as_ref().map_or(0, |a| a.shape),
                        color: a.as_ref().map_or(0, |a| a.color),
                    },
                    bridge::board_ref(&o.id, &board_title(o)),
                    BoardSettings {
                        policy: bridge::policy_name(
                            settings.policy.as_ref().map_or(Policy::Everyone, |r| r.val),
                        ),
                        laser: settings.laser_for_viewers.as_ref().is_some_and(|r| r.val),
                        host: o.s.effective_host().map_or(-1, i64::from),
                        can_draw: status.can_draw,
                    },
                )
            }
            None => (
                Me {
                    author: 0,
                    name: me.map_or_else(String::new, |m| m.name),
                    shape: 0,
                    color: 0,
                },
                bridge::BoardRef {
                    id: String::new(),
                    title: String::new(),
                },
                BoardSettings {
                    policy: "everyone",
                    laser: false,
                    host: -1,
                    can_draw: false,
                },
            ),
        };
        let init = ToPage::Init(Box::new(Init {
            me: me_json,
            board,
            strings: text::page_strings(),
            settings,
            relay: self.relay,
            paper: paper.name(),
            theme: if self.dark { "dark" } else { "light" },
            reduced_motion: self.reduced_motion,
            lang: text::language(),
        }));
        let has_board = self.open.is_some();
        let Some(page) = &mut self.page else {
            return;
        };
        page.out.queue.clear();
        page.out.queue.push_back(init.to_json());
        page.out.reset = has_board;
        page.out.ids.clear();
        page.out.peers = has_board;
        page.out.status = true;
    }

    fn queue_boards<H: Host>(&mut self, host: &mut H) {
        let now = host.wall_ms();
        let list: Vec<BoardListEntry> = self
            .index(host)
            .iter()
            .map(|e| BoardListEntry {
                id: store::hex_id(&e.id),
                title: if e.title.is_empty() {
                    t("board.untitled")
                } else {
                    e.title.clone()
                },
                saved_ago: ago_text(store::saved_ago(now, e.saved_at_ms)),
                bytes: e.size,
            })
            .collect();
        if let Some(page) = &mut self.page {
            page.out.queue.push_back(ToPage::Boards { list }.to_json());
        }
    }

    fn status_json<H: Host>(&mut self, host: &mut H) -> StatusJson {
        let paper = self.paper(host);
        let used = store::used_bytes(self.index(host));
        let saved = match self.save_state {
            SaveState::None => String::new(),
            SaveState::Saved => t("status.saved"),
            SaveState::Full => t("status.saveFull"),
            SaveState::Failed => t("status.saveFailed"),
        };
        let storage = bridge::Storage {
            used,
            quota: store::QUOTA_BYTES,
        };
        match &self.open {
            Some(o) => {
                let s = o.s.status();
                StatusJson {
                    relay: s.relay,
                    queue: s.queue,
                    saved,
                    host: s.host,
                    presenting: s.presenting,
                    following: s.following,
                    can_draw: s.can_draw,
                    loading: s.loading,
                    undo: s.undo,
                    redo: s.redo,
                    storage,
                    laser: o
                        .s
                        .board()
                        .settings()
                        .laser_for_viewers
                        .as_ref()
                        .is_some_and(|r| r.val),
                    paper: paper.name(),
                }
            }
            None => StatusJson {
                relay: self.relay,
                queue: 0,
                saved,
                host: false,
                presenting: None,
                following: false,
                can_draw: false,
                // A joiner waits for the board id: show the loading state.
                loading: self.activity.as_ref().filter(|a| !a.mine).map(|_| 0),
                undo: false,
                redo: false,
                storage,
                laser: false,
                paper: paper.name(),
            },
        }
    }

    fn peers_json(&self) -> Vec<PeerJson> {
        let Some(o) = &self.open else {
            return Vec::new();
        };
        o.s.peers()
            .into_iter()
            .map(|p| PeerJson {
                author: p.author,
                name: p.name,
                shape: p.shape,
                color: p.color,
                cursor: p.cursor.map(|(x, y)| [bridge::px(x), bridge::px(y)]),
                ago: p.ago_s,
                status: bridge::status_word(p.status),
                speaking: self.talking.contains(&p.session),
                host: p.host,
                can_draw: p.can_draw,
                hidden: p.hidden,
            })
            .collect()
    }

    /// Turns coalesced state into messages and posts as many as the bridge allows.
    fn flush_page<H: Host>(&mut self, host: &mut H) {
        let now = self.now;
        let Some(page) = &mut self.page else {
            return;
        };
        if !page.ready {
            return;
        }
        let elapsed = now.saturating_sub(page.out.last_ms) as f64 / 1000.0;
        page.out.tokens = (page.out.tokens + elapsed * PAGE_RATE_PER_S).min(PAGE_BURST);
        page.out.last_ms = now;
        // New state is only materialised once the queue has drained, so a busy bridge coalesces.
        // A second round sends what came after an `init` right away.
        for _ in 0..2 {
            if self.page.as_ref().is_some_and(|p| p.out.queue.is_empty()) {
                self.materialise(host);
            }
            let Some(page) = &mut self.page else {
                return;
            };
            while page.out.tokens >= 1.0 {
                let Some(msg) = page.out.queue.front() else {
                    break;
                };
                match host.webview_post(msg) {
                    Ok(()) => {
                        page.out.queue.pop_front();
                        page.out.tokens -= 1.0;
                    }
                    Err(Error::RateLimited) => {
                        page.out.tokens = 0.0;
                        break;
                    }
                    Err(Error::TooLarge) => {
                        host.log(Level::Warn, "a page message was too large and was dropped");
                        page.out.queue.pop_front();
                    }
                    Err(e) => {
                        host.log(Level::Debug, &format!("page is gone: {e:?}"));
                        self.page = None;
                        return;
                    }
                }
            }
            if !page.out.queue.is_empty() {
                return;
            }
        }
    }

    fn materialise<H: Host>(&mut self, host: &mut H) {
        let status = self
            .page
            .as_ref()
            .is_some_and(|p| p.out.status)
            .then(|| self.status_json(host));
        let peers = self
            .page
            .as_ref()
            .is_some_and(|p| p.out.peers)
            .then(|| self.peers_json());
        let Some(page) = &mut self.page else {
            return;
        };
        let max = page.max_bytes();
        let out = &mut page.out;
        if let Some(o) = &self.open
            && (out.reset || !out.ids.is_empty())
        {
            let (upsert, remove): (Vec<_>, Vec<_>) = if out.reset {
                (
                    o.s.visible_objects()
                        .into_iter()
                        .filter_map(bridge::page_obj)
                        .collect(),
                    Vec::new(),
                )
            } else {
                let mut up = Vec::new();
                let mut rm = Vec::new();
                for id in &out.ids {
                    match o.s.board().object(*id).filter(|_| o.s.is_visible(*id)) {
                        Some(obj) => up.extend(bridge::page_obj(obj)),
                        None => rm.push(*id),
                    }
                }
                (up, rm)
            };
            let (msgs, skipped) = bridge::objs_messages(out.reset, &upsert, &remove, max);
            if skipped > 0 {
                host.log(
                    Level::Debug,
                    &format!("{skipped} objects are too large for this view"),
                );
            }
            out.queue.extend(msgs);
            out.reset = false;
            out.ids.clear();
        }
        if let Some(s) = status {
            out.queue.push_back(ToPage::Status(Box::new(s)).to_json());
            out.status = false;
        }
        if let Some(list) = peers {
            out.queue.push_back(ToPage::Peers { list }.to_json());
            out.peers = false;
        }
        for (author, (x, y, st)) in std::mem::take(&mut out.cursors) {
            out.queue.push_back(
                ToPage::Cursor {
                    author,
                    x: bridge::px(x),
                    y: bridge::px(y),
                    status: bridge::status_word(st),
                }
                .to_json(),
            );
        }
        if let Some((author, x, y, zoom)) = out.view.take() {
            out.queue.push_back(
                ToPage::View {
                    author,
                    x: bridge::px(x),
                    y: bridge::px(y),
                    zoom: f64::from(zoom) / 1000.0,
                }
                .to_json(),
            );
        }
        for ((author, sid), (points, done)) in std::mem::take(&mut out.live) {
            out.queue.push_back(
                ToPage::Live {
                    author,
                    sid,
                    pts: bridge::flat_px(&points),
                    done,
                }
                .to_json(),
            );
        }
        for (author, points) in std::mem::take(&mut out.laser) {
            out.queue.push_back(
                ToPage::Laser {
                    author,
                    pts: bridge::flat_px(&points),
                }
                .to_json(),
            );
        }
    }

    // ---- dialogs --------------------------------------------------------------------------------

    fn menu<H: Host>(&mut self, host: &mut H, item: bridge::MenuItem) {
        use bridge::MenuItem;
        let ctx = self.dialog_ctx(host);
        let dialog = match item {
            MenuItem::Export if ctx.open => Dialog::export(&ctx),
            MenuItem::Post if ctx.open => Dialog::post(&ctx),
            MenuItem::Clear if ctx.open && ctx.host => Dialog::new(View::Clear),
            MenuItem::Settings => Dialog::new(View::Settings {
                confirm_remove: None,
            }),
            MenuItem::Boards => Dialog::new(View::Boards {
                confirm_delete: None,
            }),
            MenuItem::Describe if ctx.open => {
                let lines = self.describe();
                self.announcer.push(lines.join(" "));
                Dialog::new(View::Describe(lines))
            }
            _ => return,
        };
        self.open_dialog(host, dialog);
    }

    fn open_dialog<H: Host>(&mut self, host: &mut H, dialog: Dialog) {
        self.render = None;
        self.dialog = Some(dialog);
        self.redraw_dialog(host);
    }

    fn redraw_dialog<H: Host>(&mut self, host: &mut H) {
        let ctx = self.dialog_ctx(host);
        let Some(d) = &self.dialog else {
            return;
        };
        let title = d.title(&ctx);
        let tree = d.render(&ctx);
        if let Err(e) = host.open_dialog(&title) {
            host.log(Level::Warn, &format!("could not open the dialog: {e:?}"));
        }
        if let Err(e) = host.show(Slot::Dialog, tree) {
            host.log(Level::Warn, &format!("could not show the dialog: {e:?}"));
        }
    }

    fn dialog_note<H: Host>(&mut self, host: &mut H, tone: Tone, text: String) {
        if let Some(d) = &mut self.dialog {
            d.set_note(tone, text);
            self.redraw_dialog(host);
        }
    }

    fn dialog_ctx<H: Host>(&mut self, host: &mut H) -> Ctx {
        let now = host.wall_ms();
        let paper = self.paper(host);
        let open_id = self.open.as_ref().map(|o| o.id);
        let index = self.index(host).to_vec();
        let used = store::used_bytes(&index);
        let boards = index
            .iter()
            .map(|e| dialogs::BoardRow {
                id: e.id,
                title: e.title.clone(),
                ago: ago_text(store::saved_ago(now, e.saved_at_ms)),
                bytes: e.size,
                open: open_id == Some(e.id),
            })
            .collect();
        let mut ctx = Ctx {
            paper,
            boards,
            used,
            quota: store::QUOTA_BYTES,
            lines: self.has("chat.send"),
            ..Ctx::default()
        };
        let Some(o) = &self.open else {
            return ctx;
        };
        let board = o.s.board();
        let settings = board.settings();
        let status = o.s.status();
        let peers = o.s.peers();
        let allowed = |key: &Option<String>| {
            key.as_ref()
                .and_then(|k| settings.allowed.get(k))
                .is_some_and(|r| r.val)
        };
        let key_of = |session: SessionId| {
            self.presence
                .iter()
                .find(|p| p.session == session)
                .and_then(|p| p.key.clone())
        };
        let me = self.me.clone();
        let my_key = me.as_ref().and_then(|m| m.key.clone());
        let mut persons = vec![dialogs::Person {
            author: o.s.me(),
            name: me.map_or_else(String::new, |m| m.name),
            key: my_key.clone(),
            me: true,
            host: status.host,
            allowed: allowed(&my_key),
            hidden: false,
        }];
        persons.extend(peers.iter().map(|p| {
            let key = key_of(p.session);
            dialogs::Person {
                author: p.author,
                name: p.name.clone(),
                allowed: allowed(&key),
                key,
                me: false,
                host: p.host,
                hidden: p.hidden,
            }
        }));
        ctx.title = board_title(o);
        ctx.items = board.visible_objects().len();
        ctx.people = peers.len() + 1;
        ctx.selected = self
            .selection
            .iter()
            .filter(|id| o.s.is_visible(**id))
            .count();
        ctx.has_view = self.view.is_some();
        ctx.host = status.host;
        ctx.policy = Some(settings.policy.as_ref().map_or(Policy::Everyone, |r| r.val));
        ctx.laser = settings.laser_for_viewers.as_ref().is_some_and(|r| r.val);
        ctx.persons = persons;
        ctx.open = true;
        ctx
    }

    fn dialog_action<H: Host>(&mut self, host: &mut H, action: &str, value: &ActionValue) {
        let ctx = self.dialog_ctx(host);
        let Some(d) = &mut self.dialog else {
            return;
        };
        let effect = d.on_action(action, value, &ctx);
        self.apply(host, effect);
    }

    fn apply<H: Host>(&mut self, host: &mut H, effect: Effect) {
        let now = self.now;
        match effect {
            Effect::None => {}
            Effect::Redraw => self.redraw_dialog(host),
            Effect::Close => {
                host.close_dialog();
                self.dialog = None;
                self.render = None;
            }
            Effect::Export(req) => self.export(host, req),
            Effect::Post(area) => {
                self.request_render(host, area, 2, false, Purpose::Post { scale: 2 }, now)
            }
            Effect::Clear => {
                let Some(o) = &mut self.open else {
                    return;
                };
                match o.s.clear(now) {
                    Ok(out) => {
                        self.run(host, out, None);
                        self.clear_undo_until = Some(now + UNDO_CLEAR_MS);
                        let until = host.wall_ms() / 1000 + UNDO_CLEAR_MS / 1000;
                        self.open_dialog(host, Dialog::new(View::Cleared { until }));
                        self.announcer.push(t("announce.cleared"));
                    }
                    Err(e) => self.dialog_refused(host, e),
                }
            }
            Effect::UndoClear => {
                if self.clear_undo_until.is_none_or(|u| now >= u) {
                    self.dialog_note(host, Tone::Warning, t("dialog.cleared.tooLate"));
                    return;
                }
                let Some(o) = &mut self.open else {
                    return;
                };
                match o.s.undo(now) {
                    Ok(out) => {
                        self.run(host, out, None);
                        self.clear_undo_until = None;
                        host.close_dialog();
                        self.dialog = None;
                        self.announcer.push(t("announce.restored"));
                    }
                    Err(e) => self.dialog_refused(host, e),
                }
            }
            Effect::Policy(p) => self.board_settings(host, vec![BoardField::Policy(p)]),
            Effect::Allow { key, on } => {
                self.board_settings(host, vec![BoardField::Allowed(key, on)])
            }
            Effect::Laser(b) => self.board_settings(host, vec![BoardField::LaserForViewers(b)]),
            Effect::Title(title) => {
                if !title.is_empty() {
                    self.board_settings(host, vec![BoardField::Title(title)]);
                }
            }
            Effect::Hide { author, on } => {
                if let Some(o) = &mut self.open {
                    let out = o.s.set_hidden(author, on, now);
                    self.run(host, out, None);
                }
                self.redraw_dialog(host);
            }
            Effect::RemoveAuthor(a) => {
                let Some(o) = &mut self.open else {
                    return;
                };
                match o.s.remove_author(a, now) {
                    Ok(out) => {
                        self.run(host, out, None);
                        self.redraw_dialog(host);
                    }
                    Err(e) => self.dialog_refused(host, e),
                }
            }
            Effect::Handover(a) => {
                let Some(o) = &mut self.open else {
                    return;
                };
                match o.s.handover(a, now) {
                    Ok(out) => {
                        self.run(host, out, None);
                        self.redraw_dialog(host);
                    }
                    Err(e) => self.dialog_refused(host, e),
                }
            }
            Effect::Paper(p) => {
                self.set_paper(host, p);
                self.redraw_dialog(host);
            }
            Effect::Line(line) => self.post_line(host, line),
            Effect::ShowBoards => self.open_dialog(
                host,
                Dialog::new(View::Boards {
                    confirm_delete: None,
                }),
            ),
            Effect::OpenBoard(id) => {
                if self.open.is_some() {
                    return;
                }
                if self.activity.as_ref().is_some_and(|a| a.mine) {
                    host.close_dialog();
                    self.dialog = None;
                    self.open_saved(host, id);
                } else if self.activity.is_none() {
                    // The click is the gesture that starts the stage; the board opens once the
                    // page is ready.
                    host.close_dialog();
                    self.dialog = None;
                    self.pending_open = Some(id);
                    self.start(host);
                }
            }
            Effect::DeleteBoard(id) => {
                if self.open.as_ref().is_some_and(|o| o.id == id) {
                    return;
                }
                let result = store::delete_board(host, &id);
                self.index = None;
                match result {
                    Ok(()) => self.dialog_note(host, Tone::Success, t("dialog.boards.deleted")),
                    Err(e) => {
                        host.log(Level::Warn, &format!("could not delete a board: {e:?}"));
                        self.dialog_note(host, Tone::Danger, t("dialog.boards.deleteFailed"));
                    }
                }
                if self.open.is_none()
                    && let Some(page) = &self.page
                    && page.ready
                {
                    self.queue_boards(host);
                }
            }
        }
    }

    fn board_settings<H: Host>(&mut self, host: &mut H, fields: Vec<BoardField>) {
        let Some(o) = &mut self.open else {
            return;
        };
        match o.s.set_settings(fields, self.now) {
            Ok(out) => {
                let now = self.now;
                if let Some(o) = &mut self.open {
                    o.autosave.changed(now);
                }
                self.run(host, out, None);
                self.queue_init_settings();
                self.redraw_dialog(host);
            }
            Err(e) => self.dialog_refused(host, e),
        }
    }

    /// The page reads policy and laser from `init`/`status`; a status update carries them.
    fn queue_init_settings(&mut self) {
        if let Some(page) = &mut self.page {
            page.out.status = true;
        }
    }

    fn dialog_refused<H: Host>(&mut self, host: &mut H, e: LocalError) {
        host.log(Level::Debug, &format!("refused: {e:?}"));
        let text = match e {
            LocalError::NotAllowed => t("dialog.hostOnly"),
            LocalError::Model(_) => t("dialog.failed"),
        };
        self.dialog_note(host, Tone::Warning, text);
    }

    // ---- export, post to chat, chat lines -------------------------------------------------------

    fn export<H: Host>(&mut self, host: &mut H, req: ExportReq) {
        let Some(o) = &self.open else {
            return;
        };
        let title = board_title(o);
        let format = match req.format {
            FileFormat::Png => export::Format::Png,
            FileFormat::Svg => export::Format::Svg,
            FileFormat::Pdf => export::Format::Pdf,
            FileFormat::Board => export::Format::Board,
        };
        let name = export::file_name(&title, format);
        if req.format == FileFormat::Png {
            let now = self.now;
            self.request_render(
                host,
                req.area,
                req.scale,
                req.transparent,
                Purpose::Export { name },
                now,
            );
            return;
        }
        let scope = match req.area {
            Area::Board => Scope::Board,
            Area::View => match self.view {
                Some(v) => Scope::View(export::Rect {
                    x: v.x,
                    y: v.y,
                    w: v.w,
                    h: v.h,
                }),
                None => Scope::Board,
            },
            Area::Selection => Scope::Selection(self.selection.iter().copied().collect()),
        };
        let paper = self.paper(host);
        let opts = Options {
            paper: match paper {
                PaperSetting::Dark => Paper::Dark,
                PaperSetting::Light => Paper::Light,
                _ if self.dark => Paper::Dark,
                _ => Paper::Light,
            },
            transparent: req.transparent,
            with_names: req.names,
        };
        let Some(o) = &self.open else {
            return;
        };
        let bytes = match req.format {
            FileFormat::Svg => export::svg::export_svg(o.s.board(), &scope, &opts).into_bytes(),
            FileFormat::Pdf => export::pdf::export_pdf(o.s.board(), &scope, &opts),
            _ => {
                let file = BoardFile::from_board(o.id, o.s.board(), host.wall_ms());
                match store::encode_board(&file) {
                    Ok(b) => b,
                    Err(e) => {
                        host.log(Level::Warn, &format!("board file failed: {e:?}"));
                        self.dialog_note(host, Tone::Danger, t("dialog.export.failed"));
                        return;
                    }
                }
            }
        };
        self.save_file(host, &name, &bytes);
    }

    fn save_file<H: Host>(&mut self, host: &mut H, name: &str, bytes: &[u8]) {
        let (tone, text) = match host.save_file(name, bytes) {
            Ok(true) => (
                Tone::Success,
                t_with("dialog.export.saved", &[("name", name)]),
            ),
            Ok(false) => (Tone::Neutral, t("dialog.export.cancelled")),
            Err(Error::NotPermitted) => (Tone::Warning, t("dialog.export.notPermitted")),
            Err(Error::TooLarge) => (Tone::Warning, t("dialog.export.tooLarge")),
            Err(e) => (
                Tone::Danger,
                t_with("dialog.export.error", &[("error", &error_text(e))]),
            ),
        };
        self.dialog_note(host, tone, text);
    }

    fn request_render<H: Host>(
        &mut self,
        host: &mut H,
        area: Area,
        scale: u8,
        transparent: bool,
        purpose: Purpose,
        clicked: u64,
    ) {
        let ready = self.page.as_ref().is_some_and(|p| p.ready);
        if !ready {
            self.dialog_note(host, Tone::Warning, t("dialog.render.noPage"));
            return;
        }
        self.render_seq = self.render_seq.wrapping_add(1);
        let req = self.render_seq;
        self.render = Some(Render {
            req,
            purpose,
            area,
            transparent,
            parts: Vec::new(),
            clicked,
            started: self.now,
        });
        if let Some(page) = &mut self.page {
            page.out.queue.push_back(
                ToPage::Render {
                    req,
                    scope: area.name(),
                    scale,
                    transparent,
                }
                .to_json(),
            );
        }
        self.redraw_dialog(host);
    }

    fn png_part<H: Host>(&mut self, host: &mut H, req: u32, part: u16, of: u16, data: String) {
        let Some(r) = &mut self.render else {
            // Only a click in a dialog asks for a picture; anything else is ignored.
            host.log(Level::Debug, "ignored a picture nobody asked for");
            return;
        };
        if r.req != req {
            return;
        }
        if of == 0 {
            self.render = None;
            self.dialog_note(host, Tone::Danger, t("dialog.render.failed"));
            return;
        }
        if r.parts.is_empty() {
            r.parts = vec![None; usize::from(of)];
        }
        if r.parts.len() != usize::from(of) {
            self.render = None;
            self.dialog_note(host, Tone::Danger, t("dialog.render.failed"));
            return;
        }
        r.parts[usize::from(part)] = Some(data);
        let total: usize = r.parts.iter().flatten().map(String::len).sum();
        if total > bridge::MAX_PNG_BYTES / 3 * 4 + 4 {
            self.render = None;
            self.dialog_note(host, Tone::Warning, t("dialog.render.tooLarge"));
            return;
        }
        if r.parts.iter().any(Option::is_none) {
            return;
        }
        let Some(r) = self.render.take() else {
            return;
        };
        let joined: String = r.parts.into_iter().flatten().collect();
        let png = match bridge::base64_decode(&joined) {
            Some(b) if b.starts_with(b"\x89PNG\r\n\x1a\n") => b,
            _ => {
                self.dialog_note(host, Tone::Danger, t("dialog.render.failed"));
                return;
            }
        };
        if self.now.saturating_sub(r.clicked) > GESTURE_MS {
            self.dialog_note(host, Tone::Warning, t("dialog.render.slow"));
            return;
        }
        match r.purpose {
            Purpose::Export { name } => self.save_file(host, &name, &png),
            Purpose::Post { scale } => {
                let alt = self.post_alt();
                match host.post_image(&png, &alt) {
                    Ok(PostOutcome::Image) => {
                        self.dialog_note(host, Tone::Success, t("dialog.post.posted"));
                    }
                    Ok(PostOutcome::FileOffer) => {
                        self.dialog_note(host, Tone::Accent, t("dialog.post.fileOffer"));
                    }
                    Err(Error::TooLarge) if scale > 1 => {
                        // No image limit is known up front: try the smaller picture once.
                        self.request_render(
                            host,
                            r.area,
                            1,
                            r.transparent,
                            Purpose::Post { scale: 1 },
                            r.clicked,
                        );
                        self.dialog_note(host, Tone::Neutral, t("dialog.post.trySmaller"));
                        if let Some(d) = &mut self.dialog {
                            d.busy = true;
                        }
                    }
                    Err(Error::TooLarge) => {
                        self.dialog_note(host, Tone::Warning, t("dialog.post.tooLarge"));
                    }
                    Err(Error::NotPermitted) => {
                        self.dialog_note(host, Tone::Warning, t("dialog.post.notPermitted"));
                    }
                    Err(Error::NoRecipients) => {
                        self.dialog_note(host, Tone::Warning, t("dialog.post.notConnected"));
                    }
                    Err(e) => self.dialog_note(
                        host,
                        Tone::Danger,
                        t_with("dialog.post.error", &[("error", &error_text(e))]),
                    ),
                }
            }
        }
    }

    /// The posted picture's description (also the line official Mumble clients see).
    fn post_alt(&self) -> String {
        let title = self.open.as_ref().map(board_title).unwrap_or_default();
        // The host allows 200 bytes; keep well inside for any script.
        let alt = t_with("dialog.post.alt", &[("title", &title)]);
        let mut out = String::new();
        for c in alt.chars() {
            if out.len() + c.len_utf8() > 200 {
                break;
            }
            out.push(c);
        }
        out
    }

    fn post_line<H: Host>(&mut self, host: &mut H, line: Line) {
        let Some(o) = &self.open else {
            return;
        };
        let me = self
            .me
            .as_ref()
            .map_or_else(String::new, |m| m.name.clone());
        let text = match line {
            Line::Start => t_with("line.start", &[("name", &me)]),
            Line::Summary => {
                let people = tn("line.people", (o.s.peers().len() + 1) as u64, &[]);
                let items = tn(
                    "line.items",
                    o.s.board().visible_objects().len() as u64,
                    &[],
                );
                t_with(
                    "line.summary",
                    &[
                        ("title", &board_title(o)),
                        ("people", &people),
                        ("items", &items),
                        ("name", &me),
                    ],
                )
            }
        };
        let text = clip_to(&text, MAX_LINE_CHARS);
        let id = o.id;
        let hash = host.line_hash(&text);
        let (tone, note) = match host.chat_send(&text) {
            Ok(()) => {
                let mut payload = vec![FORMAT, u8::from(line == Line::Summary)];
                payload.extend_from_slice(&hash.to_le_bytes());
                payload.extend_from_slice(&id);
                // Lets other Mumcord clients show the invite card instead of the text line.
                if let Err(e) = host.peers_send(Target::Channel, KIND_LINE, &payload) {
                    host.log(Level::Debug, &format!("line notice not sent: {e:?}"));
                }
                (Tone::Success, t("dialog.lines.posted"))
            }
            Err(Error::NotPermitted) => (Tone::Warning, t("dialog.lines.notPermitted")),
            Err(e) => (
                Tone::Danger,
                t_with("dialog.lines.error", &[("error", &error_text(e))]),
            ),
        };
        self.dialog_note(host, tone, note);
    }

    /// "Describe board": a short spoken summary, in lines of at most 200 characters.
    fn describe(&self) -> Vec<String> {
        let Some(o) = &self.open else {
            return Vec::new();
        };
        let board = o.s.board();
        let objs = board.visible_objects();
        let people = o.s.peers().len() + 1;
        let items = tn("describe.items", objs.len() as u64, &[]);
        let people = tn("describe.people", people as u64, &[]);
        let mut lines = vec![t_with(
            "describe.summary",
            &[
                ("title", &board_title(o)),
                ("items", &items),
                ("people", &people),
            ],
        )];
        let mut counts: BTreeMap<u8, u64> = BTreeMap::new();
        for obj in &objs {
            if let Some(k) = obj.kind {
                *counts.entry(k.to_u8()).or_default() += 1;
            }
        }
        let parts: Vec<String> = counts
            .iter()
            .filter_map(|(k, n)| {
                let name = bridge::kind_name(Kind::from_u8(*k)?)?;
                Some(tn(&format!("describe.kind.{name}"), *n, &[]))
            })
            .collect();
        if !parts.is_empty() {
            lines.push(t_with("describe.contents", &[("list", &parts.join(", "))]));
        }
        if let Some(h) = o.s.effective_host() {
            lines.push(t_with("describe.host", &[("name", &self.name_of(h))]));
        }
        let status = o.s.status();
        lines.push(t(if status.can_draw {
            "describe.canDraw"
        } else {
            "describe.cannotDraw"
        }));
        let notes: Vec<String> = objs
            .iter()
            .filter(|obj| matches!(obj.kind, Some(Kind::Note | Kind::Text)))
            .filter_map(|obj| obj.text.as_ref().map(|r| r.val.trim().to_owned()))
            .filter(|s| !s.is_empty())
            .take(5)
            .map(|s| format!("“{}”", clip_to(&s, 40)))
            .collect();
        if !notes.is_empty() {
            lines.push(t_with("describe.notes", &[("list", &notes.join(", "))]));
        }
        lines.into_iter().map(|l| clip(&l)).collect()
    }

    fn refresh_texts<H: Host>(&mut self, host: &mut H) {
        self.queue_init(host);
        if self.dialog.is_some() {
            self.redraw_dialog(host);
        }
        for i in 0..self.invites.len() {
            self.show_invite(host, i);
        }
    }
}

fn board_title(o: &Open) -> String {
    o.s.board()
        .settings()
        .title
        .as_ref()
        .map(|r| r.val.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| t("board.untitled"))
}

fn top_z(o: &Open) -> Option<String> {
    match o.s.board().top_z() {
        Some(top) => z_after(top).or_else(|| Some(top.to_owned())),
        None => Some("n".to_owned()),
    }
}

fn known<'a>(o: &'a Open, ids: &'a [ObjId]) -> impl Iterator<Item = ObjId> + 'a {
    ids.iter().copied().filter(|id| o.s.is_visible(*id))
}

/// New z keys that put `ids` above (or below) everything else, keeping their order.
fn restack(o: &Open, ids: &[ObjId], front: bool) -> Vec<Edit> {
    let board = o.s.board();
    let mut objs: Vec<_> = known(o, ids).filter_map(|id| board.object(id)).collect();
    objs.sort_by(|a, b| a.z_key().cmp(b.z_key()).then(a.id.cmp(&b.id)));
    let all = board.visible_objects();
    let mut edits = Vec::new();
    if front {
        let mut z = all.last().map(|o| o.z_key().to_owned());
        for obj in objs {
            let next = match &z {
                Some(top) => z_after(top),
                None => Some("n".to_owned()),
            };
            let Some(next) = next else { break };
            edits.push(Edit::Set {
                id: obj.id,
                fields: Fields {
                    z: Some(next.clone()),
                    ..Fields::default()
                },
            });
            z = Some(next);
        }
    } else {
        let mut z = all.first().map(|o| o.z_key().to_owned());
        for obj in objs.into_iter().rev() {
            let next = match &z {
                Some(bottom) => z_before(bottom),
                None => Some("n".to_owned()),
            };
            let Some(next) = next else { break };
            edits.push(Edit::Set {
                id: obj.id,
                fields: Fields {
                    z: Some(next.clone()),
                    ..Fields::default()
                },
            });
            z = Some(next);
        }
    }
    edits
}

/// Copies of `ids`, a little down and to the right, on top of everything.
fn duplicates(o: &Open, ids: &[ObjId]) -> Vec<Edit> {
    let board = o.s.board();
    let mut z = board.top_z().map(str::to_owned);
    let mut edits = Vec::new();
    let mut objs: Vec<_> = known(o, ids).filter_map(|id| board.object(id)).collect();
    objs.sort_by(|a, b| a.z_key().cmp(b.z_key()).then(a.id.cmp(&b.id)));
    for obj in objs {
        let Some(kind) = obj.kind else { continue };
        let next = match &z {
            Some(top) => z_after(top),
            None => Some("n".to_owned()),
        };
        let g = obj.geom.as_ref().map(|r| r.val).unwrap_or_default();
        edits.push(Edit::Create {
            kind,
            base: obj.base.clone(),
            fields: Fields {
                geom: Some(Geom {
                    x: g.x.saturating_add(DUP_OFFSET),
                    y: g.y.saturating_add(DUP_OFFSET),
                    ..g
                }),
                ink: obj.ink.as_ref().map(|r| r.val),
                width: obj.width.as_ref().map(|r| r.val),
                fill: obj.fill.as_ref().map(|r| r.val),
                text: obj.text.as_ref().map(|r| r.val.clone()),
                z: next.clone(),
                alt: obj.alt.as_ref().map(|r| r.val.clone()),
                deleted: None,
            },
        });
        if next.is_some() {
            z = next;
        }
    }
    edits
}

fn ago_text(ago: Ago) -> String {
    match ago {
        Ago::JustNow => t("ago.now"),
        Ago::Minutes(n) => tn("ago.minutes", n, &[]),
        Ago::Hours(n) => tn("ago.hours", n, &[]),
        Ago::Days(n) => tn("ago.days", n, &[]),
    }
}
