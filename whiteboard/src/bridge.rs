//! The page bridge: JSON messages between the plugin and its web page (`web/board/`).
//!
//! The page holds no authority. Every message it sends is untrusted: [`parse`] checks the size,
//! the message type, every field's type and range, and turns it into a [`PageMsg`] in model
//! units (quarter pixels, object ids, board ids) or refuses it with a [`Refused`] reason.
//! Messages to the page are the serde types of [`ToPage`]; object lists are split by
//! [`objs_messages`] so each message fits the bridge of the place the page runs in.
//!
//! Units: the page speaks world pixels (floats are fine), the model quarter pixels.

use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::codec::BoardId;
use crate::model::{
    AuthorKey, FILL_COUNT, Geom, INK_COUNT, Kind, MAX_ALT_CHARS, MAX_COORD, MAX_TEXT_CHARS,
    MAX_TITLE_CHARS, ObjId, Object, Policy, WIDTH_COUNT, valid_z,
};
use crate::store::{hex_id, parse_hex_id};
use mumcord_plugin_sdk::webview::Place;

/// Which part of the board an export or post covers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Area {
    #[default]
    Board,
    View,
    Selection,
}

impl Area {
    /// The page's word in a `render` request and an `export` message.
    pub fn name(self) -> &'static str {
        match self {
            Area::Board => "board",
            Area::View => "view",
            Area::Selection => "selection",
        }
    }

    fn from_name(s: &str) -> Option<Area> {
        [Area::Board, Area::View, Area::Selection]
            .into_iter()
            .find(|a| a.name() == s)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FileFormat {
    #[default]
    Png,
    Svg,
    Pdf,
    Board,
}

impl FileFormat {
    fn from_name(s: &str) -> Option<FileFormat> {
        Some(match s {
            "png" => FileFormat::Png,
            "svg" => FileFormat::Svg,
            "pdf" => FileFormat::Pdf,
            "board" => FileFormat::Board,
            _ => return None,
        })
    }
}

/// What the user asked to export from the in-board Export dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportReq {
    pub format: FileFormat,
    pub area: Area,
    /// PNG only: 1, 2 or 4.
    pub scale: u8,
    pub transparent: bool,
    /// SVG and PDF only.
    pub names: bool,
    /// Post the PNG to chat instead of saving a file.
    pub to_chat: bool,
}

/// Bridge limit of the stage page.
pub const STAGE_MAX_BYTES: usize = 64 * 1024;
/// Bridge limit of the side panel (and every other place).
pub const PANEL_MAX_BYTES: usize = 16 * 1024;
/// Largest coordinate the page may send, in pixels: half the model's range, so a size computed
/// from two coordinates still fits.
const MAX_PX: f64 = (MAX_COORD / 8) as f64;
/// Raw pen points the page may send for one stroke; simplification brings them down to the
/// model's per-stroke limit.
const MAX_RAW_POINTS: usize = 8_000;
/// Points in one live, laser or cursor chunk.
const MAX_CHUNK_POINTS: usize = 2_000;
/// Ids in one selection message (front, back, delete, dup, menu selection).
pub const MAX_IDS: usize = 5_000;
/// Parts of one rendered PNG and the largest PNG accepted (the plugin has 32 MiB of memory).
pub const MAX_PNG_PARTS: u16 = 512;
pub const MAX_PNG_BYTES: usize = 8 << 20;
/// Zoom the page may report (10 % to 400 % plus some slack).
const ZOOM_RANGE: (f64, f64) = (0.05, 8.0);
/// Room for the envelope of an `objs` message around its objects.
const OBJS_ENVELOPE: usize = 256;

/// Why a page message was dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    TooLarge,
    NotJson,
    UnknownType,
    BadField(&'static str),
}

/// What the plugin opens from the page's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    /// The in-board dialogs (the page draws them, the plugin fills in the counted texts).
    Export,
    Post,
    Clear,
    Settings,
    Describe,
    Boards,
}

/// A paper choice of the local user.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaperSetting {
    #[default]
    Auto,
    Light,
    Dark,
    Dots,
    Grid,
    Plain,
}

impl PaperSetting {
    pub const ALL: [PaperSetting; 6] = [
        PaperSetting::Auto,
        PaperSetting::Light,
        PaperSetting::Dark,
        PaperSetting::Dots,
        PaperSetting::Grid,
        PaperSetting::Plain,
    ];

    pub fn name(self) -> &'static str {
        match self {
            PaperSetting::Auto => "auto",
            PaperSetting::Light => "light",
            PaperSetting::Dark => "dark",
            PaperSetting::Dots => "dots",
            PaperSetting::Grid => "grid",
            PaperSetting::Plain => "plain",
        }
    }

    pub fn from_name(s: &str) -> Option<PaperSetting> {
        Self::ALL.into_iter().find(|p| p.name() == s)
    }
}

pub fn policy_name(p: Policy) -> &'static str {
    match p {
        Policy::Everyone => "everyone",
        Policy::Selected => "selected",
        Policy::HostOnly => "host",
    }
}

pub fn policy_from_name(s: &str) -> Option<Policy> {
    [Policy::Everyone, Policy::Selected, Policy::HostOnly]
        .into_iter()
        .find(|p| policy_name(*p) == s)
}

/// A world rectangle in pixels (the page's current view).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PxRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A new object from the page, already in model units.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewObj {
    pub kind: Kind,
    pub geom: Geom,
    /// Base points of a stroke (quarter pixels, local to `geom`); empty for other kinds.
    pub base: Vec<(i32, i32)>,
    pub ink: Option<u8>,
    pub width: Option<u8>,
    pub fill: Option<u8>,
    pub text: Option<String>,
}

/// Field changes of one object from the page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SetFields {
    pub geom: Option<Geom>,
    pub ink: Option<u8>,
    pub width: Option<u8>,
    pub fill: Option<u8>,
    pub text: Option<String>,
    pub z: Option<String>,
    pub alt: Option<String>,
}

/// One validated message from the page.
#[derive(Clone, Debug, PartialEq)]
pub enum PageMsg {
    Ready,
    /// The side panel's "Open on stage".
    OpenStage,
    Create(NewObj),
    Set {
        id: ObjId,
        fields: SetFields,
    },
    Front(Vec<ObjId>),
    Back(Vec<ObjId>),
    Delete(Vec<ObjId>),
    Dup(Vec<ObjId>),
    Undo,
    Redo,
    /// Pointer over the board (quarter pixels, status 0 to 3), or `None` when it left.
    Cursor(Option<(i32, i32, u8)>),
    Live {
        sid: u16,
        points: Vec<(i32, i32)>,
        done: bool,
    },
    Laser(Vec<(i32, i32)>),
    /// Centre of the view in quarter pixels and zoom in per mille.
    View {
        x: i32,
        y: i32,
        zoom_permille: u32,
    },
    Present(bool),
    Follow(bool),
    Menu {
        item: MenuItem,
        view: Option<PxRect>,
        selection: Vec<ObjId>,
    },
    /// The Export dialog's Save or Post button.
    Export {
        req: ExportReq,
        view: Option<PxRect>,
        selection: Vec<ObjId>,
    },
    /// The Clear confirmation's "Clear board" button.
    Clear,
    Settings {
        policy: Option<Policy>,
        allow: Option<(String, bool)>,
        laser: Option<bool>,
        paper: Option<PaperSetting>,
    },
    Hide {
        author: AuthorKey,
        on: bool,
    },
    RemoveAuthor(AuthorKey),
    Handover(AuthorKey),
    Open(BoardId),
    DeleteBoard(BoardId),
    NewBoard(String),
    Png(PngPart),
    /// "Undo" in the page's cleared notice.
    UndoClear,
}

/// One part of a rendered PNG; `of == 0` means the render failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PngPart {
    pub req: u32,
    pub part: u16,
    pub of: u16,
    pub data: String,
}

/// Validates one message from the page and returns the place the page says it runs in.
///
/// Host messages carry no place and a plugin may show up to three web views, so every page tags
/// its messages with `p` (`stage` or `side`); the bridge limit of that place applies.
pub fn parse(raw: &str) -> Result<(Place, PageMsg), Refused> {
    if raw.len() > STAGE_MAX_BYTES {
        return Err(Refused::TooLarge);
    }
    let value: Value = serde_json::from_str(raw).map_err(|_| Refused::NotJson)?;
    let m = value.as_object().ok_or(Refused::NotJson)?;
    let place = match m.get("p").and_then(Value::as_str) {
        Some("stage") => Place::Stage,
        Some("side") => Place::SidePanel,
        _ => return Err(Refused::BadField("p")),
    };
    if raw.len() > max_bytes(place) {
        return Err(Refused::TooLarge);
    }
    Ok((place, parse_msg(m)?))
}

/// The bridge limit of a place.
pub fn max_bytes(place: Place) -> usize {
    if place == Place::Stage {
        STAGE_MAX_BYTES
    } else {
        PANEL_MAX_BYTES
    }
}

/// The page tag of a place, as in `p` and `to`.
pub fn place_tag(place: Place) -> &'static str {
    if place == Place::Stage {
        "stage"
    } else {
        "side"
    }
}

/// Addresses a message for the page in `place`: the host may deliver a post to every open web
/// view of the plugin, and each page keeps only messages whose `to` is its own tag.
pub fn addressed(json: &str, place: Place) -> String {
    match json.strip_prefix('{') {
        Some(rest) => format!("{{\"to\":\"{}\",{rest}", place_tag(place)),
        None => json.to_owned(),
    }
}

fn parse_msg(m: &Map<String, Value>) -> Result<PageMsg, Refused> {
    let t = m
        .get("t")
        .and_then(Value::as_str)
        .ok_or(Refused::UnknownType)?;
    Ok(match t {
        "ready" => PageMsg::Ready,
        "open-stage" => PageMsg::OpenStage,
        "undo" => PageMsg::Undo,
        "redo" => PageMsg::Redo,
        "create" => PageMsg::Create(new_obj(m.get("obj").ok_or(Refused::BadField("obj"))?)?),
        "set" => PageMsg::Set {
            id: obj_id_field(m, "id")?,
            fields: set_fields(m.get("fields").ok_or(Refused::BadField("fields"))?)?,
        },
        "front" => PageMsg::Front(ids(m, "ids")?),
        "back" => PageMsg::Back(ids(m, "ids")?),
        "delete" => PageMsg::Delete(ids(m, "ids")?),
        "dup" => PageMsg::Dup(ids(m, "ids")?),
        "cursor" => {
            if m.get("off") == Some(&Value::Bool(true)) {
                PageMsg::Cursor(None)
            } else {
                let status = match m.get("status") {
                    None => 0,
                    Some(Value::String(s)) => status_code(s),
                    Some(_) => return Err(Refused::BadField("status")),
                };
                PageMsg::Cursor(Some((
                    quarter(coord(m, "x")?),
                    quarter(coord(m, "y")?),
                    status,
                )))
            }
        }
        "live" => {
            let sid = m
                .get("sid")
                .and_then(Value::as_u64)
                .ok_or(Refused::BadField("sid"))?;
            PageMsg::Live {
                // The page counts strokes up; only the low bits travel.
                sid: (sid & 0xffff) as u16,
                points: flat_points(m.get("pts"), MAX_CHUNK_POINTS)?,
                done: flag(m, "done")?.unwrap_or(false),
            }
        }
        "laser" => PageMsg::Laser(flat_points(m.get("pts"), MAX_CHUNK_POINTS)?),
        "view" => {
            let zoom = number(m, "zoom")?;
            if !(ZOOM_RANGE.0..=ZOOM_RANGE.1).contains(&zoom) {
                return Err(Refused::BadField("zoom"));
            }
            PageMsg::View {
                x: quarter(coord(m, "x")?),
                y: quarter(coord(m, "y")?),
                zoom_permille: (zoom * 1000.0).round() as u32,
            }
        }
        "present" => PageMsg::Present(flag(m, "on")?.ok_or(Refused::BadField("on"))?),
        "follow" => PageMsg::Follow(flag(m, "on")?.ok_or(Refused::BadField("on"))?),
        "menu" => menu(m)?,
        "export" => export_msg(m)?,
        "clear" => PageMsg::Clear,
        "undo-clear" => PageMsg::UndoClear,
        "settings" => settings(m)?,
        "hide" => PageMsg::Hide {
            author: author(m)?,
            on: flag(m, "on")?.ok_or(Refused::BadField("on"))?,
        },
        "remove-author" => PageMsg::RemoveAuthor(author(m)?),
        "handover" => PageMsg::Handover(author(m)?),
        "open" => PageMsg::Open(board_id(m)?),
        "delete-board" => PageMsg::DeleteBoard(board_id(m)?),
        "new-board" => PageMsg::NewBoard(match m.get("title") {
            None => String::new(),
            Some(v) => text(v, MAX_TITLE_CHARS, "title")?,
        }),
        "png" => png(m)?,
        _ => return Err(Refused::UnknownType),
    })
}

// ---- field helpers -----------------------------------------------------------------------------

fn number(m: &Map<String, Value>, key: &'static str) -> Result<f64, Refused> {
    m.get(key)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .ok_or(Refused::BadField(key))
}

fn coord(m: &Map<String, Value>, key: &'static str) -> Result<f64, Refused> {
    let v = number(m, key)?;
    if v.abs() <= MAX_PX {
        Ok(v)
    } else {
        Err(Refused::BadField(key))
    }
}

fn flag(m: &Map<String, Value>, key: &'static str) -> Result<Option<bool>, Refused> {
    match m.get(key) {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(Refused::BadField(key)),
    }
}

fn small(v: Option<&Value>, count: u8, key: &'static str) -> Result<Option<u8>, Refused> {
    match v {
        None => Ok(None),
        Some(v) => v
            .as_u64()
            .filter(|n| *n < u64::from(count))
            .map(|n| Some(n as u8))
            .ok_or(Refused::BadField(key)),
    }
}

fn text(v: &Value, max_chars: usize, key: &'static str) -> Result<String, Refused> {
    let s = v.as_str().ok_or(Refused::BadField(key))?;
    if s.chars().count() > max_chars {
        return Err(Refused::BadField(key));
    }
    Ok(s.to_owned())
}

/// Pixels to quarter pixels (the caller has checked the range).
pub fn quarter(px: f64) -> i32 {
    (px * 4.0).round() as i32
}

/// Quarter pixels to pixels.
pub fn px(q: i32) -> f64 {
    f64::from(q) / 4.0
}

fn status_code(s: &str) -> u8 {
    match s {
        "drawing" => 1,
        "typing" => 2,
        "highlighting" => 3,
        _ => 0,
    }
}

/// The page's word for a cursor status code.
pub fn status_word(code: u8) -> &'static str {
    match code {
        1 => "drawing",
        2 => "typing",
        3 => "highlighting",
        _ => "",
    }
}

fn kind_from_name(s: &str) -> Option<Kind> {
    Some(match s {
        "stroke" => Kind::Stroke,
        "hl" => Kind::Highlight,
        "rect" => Kind::Rect,
        "ellipse" => Kind::Ellipse,
        "diamond" => Kind::Diamond,
        "line" => Kind::Line,
        "arrow" => Kind::Arrow,
        "note" => Kind::Note,
        "text" => Kind::Text,
        _ => return None,
    })
}

/// The page's name of a kind; `None` for kinds the page cannot draw (images).
pub fn kind_name(k: Kind) -> Option<&'static str> {
    Some(match k {
        Kind::Stroke => "stroke",
        Kind::Highlight => "hl",
        Kind::Rect => "rect",
        Kind::Ellipse => "ellipse",
        Kind::Diamond => "diamond",
        Kind::Line => "line",
        Kind::Arrow => "arrow",
        Kind::Note => "note",
        Kind::Text => "text",
        Kind::Image => return None,
    })
}

/// `"<author>.<counter>"`, both plain decimal.
pub fn id_string(id: ObjId) -> String {
    format!("{}.{}", id.author, id.counter)
}

pub fn parse_id(s: &str) -> Option<ObjId> {
    let (a, c) = s.split_once('.')?;
    let num = |p: &str| {
        if p.is_empty() || p.len() > 10 || !p.bytes().all(|b| b.is_ascii_digit()) {
            None
        } else {
            p.parse::<u32>().ok()
        }
    };
    Some(ObjId {
        author: num(a)?,
        counter: num(c)?,
    })
}

fn obj_id_field(m: &Map<String, Value>, key: &'static str) -> Result<ObjId, Refused> {
    m.get(key)
        .and_then(Value::as_str)
        .and_then(parse_id)
        .ok_or(Refused::BadField(key))
}

fn id_list(v: Option<&Value>, key: &'static str) -> Result<Vec<ObjId>, Refused> {
    let list = v.and_then(Value::as_array).ok_or(Refused::BadField(key))?;
    if list.len() > MAX_IDS {
        return Err(Refused::BadField(key));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(list.len());
    for v in list {
        let id = v
            .as_str()
            .and_then(parse_id)
            .ok_or(Refused::BadField(key))?;
        if seen.insert(id) {
            out.push(id);
        }
    }
    Ok(out)
}

fn ids(m: &Map<String, Value>, key: &'static str) -> Result<Vec<ObjId>, Refused> {
    id_list(m.get(key), key)
}

fn author(m: &Map<String, Value>) -> Result<AuthorKey, Refused> {
    m.get("author")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(Refused::BadField("author"))
}

fn board_id(m: &Map<String, Value>) -> Result<BoardId, Refused> {
    m.get("board")
        .and_then(Value::as_str)
        .and_then(parse_hex_id)
        .ok_or(Refused::BadField("board"))
}

/// A flat `[x0, y0, x1, y1, …]` list in pixels, to quarter pixels.
fn flat_points(v: Option<&Value>, max_points: usize) -> Result<Vec<(i32, i32)>, Refused> {
    let list = v
        .and_then(Value::as_array)
        .ok_or(Refused::BadField("pts"))?;
    if list.len() % 2 != 0 || list.len() > 2 * max_points {
        return Err(Refused::BadField("pts"));
    }
    list.as_chunks::<2>()
        .0
        .iter()
        .map(|p| {
            let x = p[0].as_f64().filter(|v| v.abs() <= MAX_PX);
            let y = p[1].as_f64().filter(|v| v.abs() <= MAX_PX);
            match (x, y) {
                (Some(x), Some(y)) => Ok((quarter(x), quarter(y))),
                _ => Err(Refused::BadField("pts")),
            }
        })
        .collect()
}

fn geom_px(v: Option<&Value>) -> Result<[f64; 4], Refused> {
    let list = v.and_then(Value::as_array).ok_or(Refused::BadField("g"))?;
    if list.len() != 4 {
        return Err(Refused::BadField("g"));
    }
    let mut g = [0.0; 4];
    for (slot, v) in g.iter_mut().zip(list) {
        *slot = v
            .as_f64()
            .filter(|v| v.abs() <= MAX_PX)
            .ok_or(Refused::BadField("g"))?;
    }
    Ok(g)
}

fn geom(v: Option<&Value>) -> Result<Geom, Refused> {
    let [x, y, w, h] = geom_px(v)?;
    Ok(Geom {
        x: quarter(x),
        y: quarter(y),
        w: quarter(w),
        h: quarter(h),
    })
}

fn new_obj(v: &Value) -> Result<NewObj, Refused> {
    let m = v.as_object().ok_or(Refused::BadField("obj"))?;
    let kind = m
        .get("k")
        .and_then(Value::as_str)
        .and_then(kind_from_name)
        .ok_or(Refused::BadField("k"))?;
    let ink = small(m.get("ink"), INK_COUNT, "ink")?;
    let width = small(m.get("w"), WIDTH_COUNT, "w")?;
    let fill = small(m.get("fill"), FILL_COUNT, "fill")?;
    let text = match m.get("text") {
        None => None,
        Some(v) => Some(text(v, MAX_TEXT_CHARS, "text")?),
    };
    let (geom, base) = if kind.has_points() {
        // Stroke points are local to the box origin; the model wants the pen path itself.
        let [ox, oy, _, _] = geom_px(m.get("g"))?;
        let local = m
            .get("pts")
            .and_then(Value::as_array)
            .ok_or(Refused::BadField("pts"))?;
        if local.len() % 2 != 0 || local.len() < 2 || local.len() > 2 * MAX_RAW_POINTS {
            return Err(Refused::BadField("pts"));
        }
        let mut path = Vec::with_capacity(local.len() / 2);
        for p in local.as_chunks::<2>().0 {
            match (p[0].as_f64(), p[1].as_f64()) {
                (Some(x), Some(y)) if (ox + x).abs() <= MAX_PX && (oy + y).abs() <= MAX_PX => {
                    path.push((ox + x, oy + y));
                }
                _ => return Err(Refused::BadField("pts")),
            }
        }
        crate::model::prepare_stroke(&path).map_err(|_| Refused::BadField("pts"))?
    } else {
        (geom(m.get("g"))?, Vec::new())
    };
    Ok(NewObj {
        kind,
        geom,
        base,
        ink,
        width,
        fill,
        text,
    })
}

fn set_fields(v: &Value) -> Result<SetFields, Refused> {
    let m = v.as_object().ok_or(Refused::BadField("fields"))?;
    let f = SetFields {
        geom: match m.get("g") {
            None => None,
            Some(_) => Some(geom(m.get("g"))?),
        },
        ink: small(m.get("ink"), INK_COUNT, "ink")?,
        width: small(m.get("w"), WIDTH_COUNT, "w")?,
        fill: small(m.get("fill"), FILL_COUNT, "fill")?,
        text: match m.get("text") {
            None => None,
            Some(v) => Some(text(v, MAX_TEXT_CHARS, "text")?),
        },
        z: match m.get("z") {
            None => None,
            Some(v) => {
                let z = v
                    .as_str()
                    .filter(|z| valid_z(z))
                    .ok_or(Refused::BadField("z"))?;
                Some(z.to_owned())
            }
        },
        alt: match m.get("alt") {
            None => None,
            Some(v) => Some(text(v, MAX_ALT_CHARS, "alt")?),
        },
    };
    if f == SetFields::default() {
        return Err(Refused::BadField("fields"));
    }
    Ok(f)
}

fn menu(m: &Map<String, Value>) -> Result<PageMsg, Refused> {
    let item = match m.get("item").and_then(Value::as_str) {
        Some("export") => MenuItem::Export,
        Some("post") => MenuItem::Post,
        Some("clear") => MenuItem::Clear,
        Some("settings") => MenuItem::Settings,
        Some("describe") => MenuItem::Describe,
        Some("boards") => MenuItem::Boards,
        _ => return Err(Refused::BadField("item")),
    };
    let (view, selection) = view_and_selection(m)?;
    Ok(PageMsg::Menu {
        item,
        view,
        selection,
    })
}

/// What the page knows and the plugin needs for an export: the view ("what I see now") and the
/// selection.
fn view_and_selection(m: &Map<String, Value>) -> Result<(Option<PxRect>, Vec<ObjId>), Refused> {
    let view = match m.get("view") {
        None => None,
        Some(v) => {
            let [x, y, w, h] = geom_px(Some(v)).map_err(|_| Refused::BadField("view"))?;
            if w <= 0.0 || h <= 0.0 {
                return Err(Refused::BadField("view"));
            }
            Some(PxRect { x, y, w, h })
        }
    };
    let selection = match m.get("sel") {
        None => Vec::new(),
        Some(v) => id_list(Some(v), "sel")?,
    };
    Ok((view, selection))
}

/// The in-board Export dialog's button (mockup 5): save a file or post the PNG to chat.
fn export_msg(m: &Map<String, Value>) -> Result<PageMsg, Refused> {
    let name = |key: &'static str| {
        m.get(key)
            .and_then(Value::as_str)
            .ok_or(Refused::BadField(key))
    };
    let area = Area::from_name(name("area")?).ok_or(Refused::BadField("area"))?;
    let to_chat = match name("dest")? {
        "file" => false,
        "chat" => true,
        _ => return Err(Refused::BadField("dest")),
    };
    let format = FileFormat::from_name(name("format")?).ok_or(Refused::BadField("format"))?;
    // Only a PNG goes to chat (mockup 5: the other formats are files only).
    if to_chat && format != FileFormat::Png {
        return Err(Refused::BadField("format"));
    }
    let scale = match m.get("scale").and_then(Value::as_u64) {
        Some(s @ (1 | 2 | 4)) => s as u8,
        _ => return Err(Refused::BadField("scale")),
    };
    let (view, selection) = view_and_selection(m)?;
    Ok(PageMsg::Export {
        req: ExportReq {
            format,
            area,
            scale,
            transparent: flag(m, "transparent")?.unwrap_or(false),
            names: flag(m, "names")?.unwrap_or(false),
            to_chat,
        },
        view,
        selection,
    })
}

fn settings(m: &Map<String, Value>) -> Result<PageMsg, Refused> {
    let policy = match m.get("policy") {
        None => None,
        Some(v) => Some(
            v.as_str()
                .and_then(policy_from_name)
                .ok_or(Refused::BadField("policy"))?,
        ),
    };
    let allow = match m.get("allow") {
        None => None,
        Some(v) => {
            let a = v.as_object().ok_or(Refused::BadField("allow"))?;
            let key = a
                .get("key")
                .and_then(Value::as_str)
                .filter(|k| !k.is_empty() && k.chars().count() <= crate::model::MAX_KEY_CHARS)
                .ok_or(Refused::BadField("allow"))?;
            let on = flag(a, "on")?.ok_or(Refused::BadField("allow"))?;
            Some((key.to_owned(), on))
        }
    };
    let paper = match m.get("paper") {
        None => None,
        Some(v) => Some(
            v.as_str()
                .and_then(PaperSetting::from_name)
                .ok_or(Refused::BadField("paper"))?,
        ),
    };
    let laser = flag(m, "laser")?;
    if policy.is_none() && allow.is_none() && laser.is_none() && paper.is_none() {
        return Err(Refused::BadField("settings"));
    }
    Ok(PageMsg::Settings {
        policy,
        allow,
        laser,
        paper,
    })
}

fn png(m: &Map<String, Value>) -> Result<PageMsg, Refused> {
    let req = m
        .get("req")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(Refused::BadField("req"))?;
    let int16 = |key: &'static str| {
        m.get(key)
            .and_then(Value::as_u64)
            .and_then(|v| u16::try_from(v).ok())
            .ok_or(Refused::BadField(key))
    };
    let part = int16("part")?;
    let of = int16("of")?;
    if of > MAX_PNG_PARTS || (of > 0 && part >= of) || (of == 0 && part != 0) {
        return Err(Refused::BadField("part"));
    }
    let data = m
        .get("data")
        .and_then(Value::as_str)
        .filter(|d| {
            d.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
        })
        .ok_or(Refused::BadField("data"))?;
    Ok(PageMsg::Png(PngPart {
        req,
        part,
        of,
        data: data.to_owned(),
    }))
}

/// Decodes standard base64 with padding; `None` for anything else.
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let bytes = s.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let quads = bytes.len() / 4;
    for (i, q) in bytes.as_chunks::<4>().0.iter().enumerate() {
        let last = i + 1 == quads;
        let pad = q.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return None;
        }
        let mut n = 0u32;
        for &c in &q[..4 - pad] {
            n = (n << 6) | val(c)?;
        }
        n <<= 6 * pad as u32;
        let b = n.to_be_bytes();
        out.extend_from_slice(&b[1..4 - pad]);
    }
    Some(out)
}

// ---- messages to the page ----------------------------------------------------------------------

/// One object as the page draws it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PageObj {
    pub id: String,
    pub k: &'static str,
    pub g: [f64; 4],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pts: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bw: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bh: Option<f64>,
    pub ink: u8,
    pub w: u8,
    pub fill: u8,
    pub text: String,
    pub z: String,
    pub alt: String,
    pub by: AuthorKey,
}

/// The page's view of a model object; `None` for objects it cannot draw.
pub fn page_obj(o: &Object) -> Option<PageObj> {
    let k = kind_name(o.kind?)?;
    let g = o.geom.as_ref().map(|r| r.val).unwrap_or_default();
    let (pts, bw, bh) = if o.kind.is_some_and(Kind::has_points) {
        // Base points are local to the stroke's original box, whose size is their extent.
        let bw = o.base.iter().map(|p| p.0).max().unwrap_or(0);
        let bh = o.base.iter().map(|p| p.1).max().unwrap_or(0);
        let flat = o.base.iter().flat_map(|&(x, y)| [px(x), px(y)]).collect();
        (Some(flat), Some(px(bw)), Some(px(bh)))
    } else {
        (None, None, None)
    };
    Some(PageObj {
        id: id_string(o.id),
        k,
        g: [px(g.x), px(g.y), px(g.w), px(g.h)],
        pts,
        bw,
        bh,
        ink: o.ink.as_ref().map_or(0, |r| r.val),
        w: o.width.as_ref().map_or(1, |r| r.val),
        fill: o.fill.as_ref().map_or(0, |r| r.val),
        text: o.text.as_ref().map_or_else(String::new, |r| r.val.clone()),
        z: o.z_key().to_owned(),
        alt: o.alt.as_ref().map_or_else(String::new, |r| r.val.clone()),
        by: o.id.author,
    })
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Me {
    pub author: AuthorKey,
    pub name: String,
    pub shape: u8,
    pub color: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BoardRef {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardSettings {
    pub policy: &'static str,
    pub laser: bool,
    /// Author key of the host, -1 while unknown.
    pub host: i64,
    pub can_draw: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PeerJson {
    pub author: AuthorKey,
    pub name: String,
    pub shape: u8,
    pub color: u8,
    pub cursor: Option<[f64; 2]>,
    pub ago: Option<u64>,
    pub status: &'static str,
    pub speaking: bool,
    pub host: bool,
    #[serde(rename = "canDraw")]
    pub can_draw: bool,
    pub hidden: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Storage {
    pub used: usize,
    pub quota: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusJson {
    pub relay: bool,
    pub queue: usize,
    pub saved: String,
    pub host: bool,
    pub presenting: Option<AuthorKey>,
    pub following: bool,
    pub can_draw: bool,
    pub loading: Option<u8>,
    pub undo: bool,
    pub redo: bool,
    pub storage: Storage,
    pub laser: bool,
    pub paper: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardListEntry {
    pub id: String,
    pub title: String,
    pub saved_ago: String,
    pub bytes: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Init {
    pub me: Me,
    pub board: BoardRef,
    pub strings: std::collections::BTreeMap<String, String>,
    pub settings: BoardSettings,
    pub relay: bool,
    pub paper: &'static str,
    pub theme: &'static str,
    pub reduced_motion: bool,
    pub lang: String,
}

/// Everything the plugin sends to the page.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum ToPage {
    Init(Box<Init>),
    Peers {
        list: Vec<PeerJson>,
    },
    Cursor {
        author: AuthorKey,
        x: f64,
        y: f64,
        status: &'static str,
    },
    Live {
        author: AuthorKey,
        sid: u16,
        pts: Vec<f64>,
        done: bool,
    },
    Laser {
        author: AuthorKey,
        pts: Vec<f64>,
    },
    View {
        author: AuthorKey,
        x: f64,
        y: f64,
        zoom: f64,
    },
    Status(Box<StatusJson>),
    Announce {
        text: String,
    },
    Render {
        req: u32,
        scope: &'static str,
        scale: u8,
        transparent: bool,
    },
    Boards {
        list: Vec<BoardListEntry>,
    },
    /// The outcome of an in-board dialog button, shown in that dialog. `tone` is `success`,
    /// `neutral`, `accent`, `warning` or `danger`; while `busy` the dialog's buttons stay off.
    Note {
        tone: &'static str,
        text: String,
        busy: bool,
    },
    /// Opens an in-board dialog: `which` is `export`, `post` (the export dialog set to post to
    /// chat) or `clear`. The texts that count things come translated from the plugin.
    Dialog(Box<DialogJson>),
    /// The board was cleared: the page offers Undo until `until` (Unix seconds).
    Cleared {
        until: u64,
    },
}

impl ToPage {
    pub fn to_json(&self) -> String {
        // Serialising plain structs and strings cannot fail.
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// What an in-board dialog shows besides the page's own texts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DialogJson {
    pub which: &'static str,
    pub title: String,
    /// "143 items" for the whole board.
    pub board: String,
    /// "2 items", or that nothing is selected.
    pub selection: String,
    pub has_selection: bool,
    /// The Clear confirmation's sentence.
    pub body: String,
}

/// Flattens points to the page's `[x0, y0, …]` in pixels.
pub fn flat_px(points: &[(i32, i32)]) -> Vec<f64> {
    points.iter().flat_map(|&(x, y)| [px(x), px(y)]).collect()
}

pub fn board_ref(id: &BoardId, title: &str) -> BoardRef {
    BoardRef {
        id: hex_id(id),
        title: title.to_owned(),
    }
}

/// `objs` messages for these upserts and removals, each at most `max_bytes`. With `reset` the
/// first message replaces everything the page holds. An object too large for the place on its
/// own is left out (and counted in the second return value).
pub fn objs_messages(
    reset: bool,
    upsert: &[PageObj],
    remove: &[ObjId],
    max_bytes: usize,
) -> (Vec<String>, usize) {
    let budget = max_bytes.saturating_sub(OBJS_ENVELOPE);
    let mut out = Vec::new();
    let mut skipped = 0;
    let mut ups: Vec<String> = Vec::new();
    let mut rms: Vec<String> = Vec::new();
    let mut size = 0;
    let flush = |ups: &mut Vec<String>, rms: &mut Vec<String>, out: &mut Vec<String>| {
        let reset_part = if reset && out.is_empty() {
            ",\"reset\":true"
        } else {
            ""
        };
        out.push(format!(
            "{{\"t\":\"objs\"{reset_part},\"upsert\":[{}],\"remove\":[{}]}}",
            ups.join(","),
            rms.join(",")
        ));
        ups.clear();
        rms.clear();
    };
    let items = upsert
        .iter()
        .map(|o| (true, serde_json::to_string(o).unwrap_or_default()))
        .chain(
            remove
                .iter()
                .map(|id| (false, format!("\"{}\"", id_string(*id)))),
        );
    for (is_upsert, json) in items {
        if json.len() + 1 > budget {
            skipped += 1;
            continue;
        }
        if size + json.len() + 1 > budget {
            flush(&mut ups, &mut rms, &mut out);
            size = 0;
        }
        size += json.len() + 1;
        if is_upsert {
            ups.push(json);
        } else {
            rms.push(json);
        }
    }
    if !ups.is_empty() || !rms.is_empty() || out.is_empty() {
        flush(&mut ups, &mut rms, &mut out);
    }
    (out, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tags a test message as coming from the stage page.
    fn staged(s: &str) -> String {
        format!("{{\"p\":\"stage\",{}", &s[1..])
    }

    fn ok(s: &str) -> PageMsg {
        let (place, msg) = parse(&staged(s)).unwrap_or_else(|e| panic!("{s}: {e:?}"));
        assert_eq!(place, Place::Stage);
        msg
    }

    fn bad(s: &str) -> Refused {
        parse(&staged(s)).expect_err(s)
    }

    #[test]
    fn pages_tag_their_place() {
        assert_eq!(
            parse(r#"{"p":"side","t":"ready"}"#),
            Ok((Place::SidePanel, PageMsg::Ready))
        );
        assert_eq!(parse(r#"{"t":"ready"}"#), Err(Refused::BadField("p")));
        assert_eq!(
            parse(r#"{"p":"dialog","t":"ready"}"#),
            Err(Refused::BadField("p"))
        );
        // The side panel's bridge is smaller than the stage's.
        let big = format!(
            r#"{{"p":"side","t":"ready","pad":"{}"}}"#,
            "x".repeat(PANEL_MAX_BYTES)
        );
        assert_eq!(parse(&big), Err(Refused::TooLarge));
        assert_eq!(
            addressed(r#"{"t":"announce","text":"hi"}"#, Place::SidePanel),
            r#"{"to":"side","t":"announce","text":"hi"}"#
        );
    }

    #[test]
    fn in_board_buttons_are_checked() {
        assert_eq!(
            ok(
                r#"{"t":"export","dest":"chat","format":"png","area":"selection","scale":2,"sel":["1.2"]}"#
            ),
            PageMsg::Export {
                req: ExportReq {
                    format: FileFormat::Png,
                    area: Area::Selection,
                    scale: 2,
                    transparent: false,
                    names: false,
                    to_chat: true,
                },
                view: None,
                selection: vec![ObjId {
                    author: 1,
                    counter: 2
                }],
            }
        );
        match ok(
            r#"{"t":"export","dest":"file","format":"pdf","area":"board","scale":1,"names":true}"#,
        ) {
            PageMsg::Export { req, .. } => assert!(req.names && !req.to_chat),
            other => panic!("{other:?}"),
        }
        // Only a PNG goes to chat; scales and areas are checked.
        for m in [
            r#"{"t":"export","dest":"chat","format":"svg","area":"board","scale":1}"#,
            r#"{"t":"export","dest":"file","format":"png","area":"board","scale":3}"#,
            r#"{"t":"export","dest":"file","format":"png","area":"all","scale":1}"#,
            r#"{"t":"export","dest":"mail","format":"png","area":"board","scale":1}"#,
        ] {
            assert!(matches!(bad(m), Refused::BadField(_)), "{m}");
        }
        assert_eq!(ok(r#"{"t":"clear"}"#), PageMsg::Clear);
    }

    #[test]
    fn simple_messages_parse() {
        assert_eq!(ok(r#"{"t":"ready"}"#), PageMsg::Ready);
        assert_eq!(ok(r#"{"t":"undo"}"#), PageMsg::Undo);
        assert_eq!(ok(r#"{"t":"present","on":true}"#), PageMsg::Present(true));
        assert_eq!(ok(r#"{"t":"cursor","off":true}"#), PageMsg::Cursor(None));
        assert_eq!(
            ok(r#"{"t":"cursor","x":1.5,"y":-2,"status":"typing"}"#),
            PageMsg::Cursor(Some((6, -8, 2)))
        );
        assert_eq!(
            ok(r#"{"t":"view","x":10,"y":20,"zoom":1.25}"#),
            PageMsg::View {
                x: 40,
                y: 80,
                zoom_permille: 1250
            }
        );
    }

    #[test]
    fn garbage_is_refused() {
        assert_eq!(bad("not json"), Refused::NotJson);
        assert_eq!(bad("[1,2]"), Refused::NotJson);
        assert_eq!(bad(r#"{"t":"nope"}"#), Refused::UnknownType);
        assert_eq!(bad(r#"{"x":1}"#), Refused::UnknownType);
        assert_eq!(bad(r#"{"t":"present"}"#), Refused::BadField("on"));
        assert_eq!(
            bad(r#"{"t":"cursor","x":1e300,"y":0}"#),
            Refused::BadField("x")
        );
        assert_eq!(
            bad(r#"{"t":"view","x":0,"y":0,"zoom":0}"#),
            Refused::BadField("zoom")
        );
        assert_eq!(
            bad(r#"{"t":"delete","ids":["1.x"]}"#),
            Refused::BadField("ids")
        );
        assert_eq!(
            bad(r#"{"t":"delete","ids":["+1.2"]}"#),
            Refused::BadField("ids")
        );
        assert_eq!(
            bad(r#"{"t":"hide","author":-1,"on":true}"#),
            Refused::BadField("author")
        );
        assert_eq!(
            bad(r#"{"t":"open","board":"zz"}"#),
            Refused::BadField("board")
        );
        assert_eq!(bad(r#"{"t":"settings"}"#), Refused::BadField("settings"));
        assert_eq!(
            bad(r#"{"t":"menu","item":"rm -rf"}"#),
            Refused::BadField("item")
        );
        let huge = format!(r#"{{"t":"ready","pad":"{}"}}"#, "x".repeat(STAGE_MAX_BYTES));
        assert_eq!(bad(&huge), Refused::TooLarge);
    }

    #[test]
    fn creates_are_checked_and_converted() {
        match ok(r#"{"t":"create","obj":{"k":"note","g":[1,2,156,112],"fill":4,"text":"hi"}}"#) {
            PageMsg::Create(o) => {
                assert_eq!(o.kind, Kind::Note);
                assert_eq!(
                    o.geom,
                    Geom {
                        x: 4,
                        y: 8,
                        w: 624,
                        h: 448
                    }
                );
                assert_eq!(o.fill, Some(4));
                assert_eq!(o.text.as_deref(), Some("hi"));
            }
            other => panic!("{other:?}"),
        }
        match ok(
            r#"{"t":"create","obj":{"k":"stroke","g":[10,10,20,0],"pts":[0,0,10,0,20,0],"bw":20,"bh":0,"ink":2,"w":1}}"#,
        ) {
            PageMsg::Create(o) => {
                assert_eq!(o.kind, Kind::Stroke);
                assert_eq!((o.geom.x, o.geom.y), (40, 40));
                // The straight middle point is simplified away.
                assert_eq!(o.base.len(), 2);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            bad(r#"{"t":"create","obj":{"k":"note","g":[1,2,3,4],"fill":5}}"#),
            Refused::BadField("fill")
        );
        assert_eq!(
            bad(r#"{"t":"create","obj":{"k":"image","g":[1,2,3,4]}}"#),
            Refused::BadField("k")
        );
        let long = "x".repeat(MAX_TEXT_CHARS + 1);
        assert_eq!(
            bad(&format!(
                r#"{{"t":"create","obj":{{"k":"text","g":[1,2,3,4],"text":"{long}"}}}}"#
            )),
            Refused::BadField("text")
        );
        assert_eq!(
            bad(r#"{"t":"create","obj":{"k":"stroke","g":[0,0,1,1],"pts":[0,0,1]}}"#),
            Refused::BadField("pts")
        );
    }

    #[test]
    fn set_needs_a_known_field() {
        assert_eq!(
            bad(r#"{"t":"set","id":"1.2","fields":{}}"#),
            Refused::BadField("fields")
        );
        assert_eq!(
            bad(r#"{"t":"set","id":"1.2","fields":{"z":"A"}}"#),
            Refused::BadField("z")
        );
        match ok(r#"{"t":"set","id":"7.3","fields":{"alt":"a box","ink":5}}"#) {
            PageMsg::Set { id, fields } => {
                assert_eq!(
                    id,
                    ObjId {
                        author: 7,
                        counter: 3
                    }
                );
                assert_eq!(fields.alt.as_deref(), Some("a box"));
                assert_eq!(fields.ink, Some(5));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn png_parts_are_checked() {
        assert!(matches!(
            ok(r#"{"t":"png","req":3,"part":0,"of":2,"data":"iVBO"}"#),
            PageMsg::Png(PngPart { req: 3, of: 2, .. })
        ));
        assert_eq!(
            bad(r#"{"t":"png","req":3,"part":2,"of":2,"data":""}"#),
            Refused::BadField("part")
        );
        assert_eq!(
            bad(r#"{"t":"png","req":3,"part":0,"of":1,"data":"<script>"}"#),
            Refused::BadField("data")
        );
    }

    #[test]
    fn base64_round_trips() {
        assert_eq!(base64_decode("").as_deref(), Some(&b""[..]));
        assert_eq!(base64_decode("TWFu").as_deref(), Some(&b"Man"[..]));
        assert_eq!(base64_decode("TWE=").as_deref(), Some(&b"Ma"[..]));
        assert_eq!(base64_decode("TQ==").as_deref(), Some(&b"M"[..]));
        assert_eq!(base64_decode("TQ=="), Some(vec![b'M']));
        assert_eq!(base64_decode("TQ="), None);
        assert_eq!(base64_decode("T==="), None);
        assert_eq!(base64_decode("TQ==TWFu"), None);
        assert_eq!(base64_decode("TW!u"), None);
    }

    #[test]
    fn ids_round_trip() {
        let id = ObjId {
            author: u32::MAX,
            counter: 12,
        };
        assert_eq!(parse_id(&id_string(id)), Some(id));
        assert_eq!(parse_id("1"), None);
        assert_eq!(parse_id("1.2.3"), None);
        assert_eq!(parse_id("4294967296.1"), None);
    }

    #[test]
    fn objs_split_by_size() {
        let obj = |n: u32| PageObj {
            id: id_string(ObjId {
                author: 1,
                counter: n,
            }),
            k: "note",
            g: [0.0, 0.0, 10.0, 10.0],
            pts: None,
            bw: None,
            bh: None,
            ink: 0,
            w: 1,
            fill: 0,
            text: "x".repeat(1000),
            z: "n".into(),
            alt: String::new(),
            by: 1,
        };
        let list: Vec<PageObj> = (1..=40).map(obj).collect();
        let (msgs, skipped) = objs_messages(true, &list, &[], PANEL_MAX_BYTES);
        assert_eq!(skipped, 0);
        assert!(msgs.len() >= 3);
        assert!(msgs.iter().all(|m| m.len() <= PANEL_MAX_BYTES));
        assert!(msgs[0].contains("\"reset\":true"));
        assert!(!msgs[1].contains("reset"));
        let total: usize = msgs
            .iter()
            .map(|m| m.matches("\"k\":\"note\"").count())
            .sum();
        assert_eq!(total, 40);
        // An empty reset still goes out, so the page clears its board.
        let (msgs, _) = objs_messages(true, &[], &[], PANEL_MAX_BYTES);
        assert_eq!(msgs.len(), 1);
        let (msgs, _) = objs_messages(false, &[], &[], PANEL_MAX_BYTES);
        assert_eq!(msgs.len(), 1);
        let removes: Vec<ObjId> = (1..=3000)
            .map(|n| ObjId {
                author: 4_000_000_000,
                counter: n,
            })
            .collect();
        let (msgs, _) = objs_messages(false, &[], &removes, PANEL_MAX_BYTES);
        assert!(msgs.len() >= 2);
        assert!(msgs.iter().all(|m| m.len() <= PANEL_MAX_BYTES));
    }

    #[test]
    fn page_messages_serialise_with_their_type() {
        let s = ToPage::Announce { text: "hi".into() }.to_json();
        assert_eq!(s, r#"{"t":"announce","text":"hi"}"#);
        let s = ToPage::Render {
            req: 1,
            scope: "view",
            scale: 2,
            transparent: false,
        }
        .to_json();
        assert!(s.starts_with(r#"{"t":"render""#));
    }
}
