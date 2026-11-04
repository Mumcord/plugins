//! Mumcord tree dialogs for the actions that need a real user gesture or a confirmation:
//! Export, Post to chat, Clear board, Board settings, Saved boards and Describe board.
//!
//! The web page asks for a dialog (`menu`), but a page message is not a user gesture: only the
//! buttons drawn here are. Saving a file, posting an image or a chat line therefore always starts
//! from a click in one of these dialogs. The module is deliberately self-contained: a dialog is
//! a small state machine that renders an [`El`] tree from a [`Ctx`] snapshot and turns clicks
//! into an [`Effect`] for the glue. Moving a dialog into the page later only means producing the
//! same effects from page buttons once the host accepts page clicks as gestures.

use mumcord_plugin_sdk::events::ActionValue;
use mumcord_plugin_sdk::ui::{self, Align, ButtonKind, Direction, El, Justify, Tone};

use crate::bridge::PaperSetting;
use crate::codec::BoardId;
use crate::model::{AuthorKey, MAX_TITLE_CHARS, Policy};
use crate::store::{hex_id, parse_hex_id};
use crate::text::{clip, clip_to, t, t_with, tn};

/// Which part of the board an export or post covers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Area {
    #[default]
    Board,
    View,
    Selection,
}

impl Area {
    /// The page's word in a `render` request.
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
    const ALL: [FileFormat; 4] = [
        FileFormat::Png,
        FileFormat::Svg,
        FileFormat::Pdf,
        FileFormat::Board,
    ];

    fn name(self) -> &'static str {
        match self {
            FileFormat::Png => "png",
            FileFormat::Svg => "svg",
            FileFormat::Pdf => "pdf",
            FileFormat::Board => "board",
        }
    }
}

/// What the user asked to export.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportReq {
    pub format: FileFormat,
    pub area: Area,
    /// PNG only: 1, 2 or 4.
    pub scale: u8,
    pub transparent: bool,
    /// SVG and PDF only.
    pub names: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    Start,
    Summary,
}

/// One person on the board, for the settings dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub author: AuthorKey,
    pub name: String,
    pub key: Option<String>,
    pub me: bool,
    pub host: bool,
    pub allowed: bool,
    pub hidden: bool,
}

/// One saved board, for the boards dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardRow {
    pub id: BoardId,
    pub title: String,
    pub ago: String,
    pub bytes: u32,
    /// The board that is open right now (it cannot be deleted while open).
    pub open: bool,
}

/// What a dialog shows, taken fresh from the glue for every render.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ctx {
    pub title: String,
    pub items: usize,
    /// People on the board, including the local user.
    pub people: usize,
    pub selected: usize,
    pub has_view: bool,
    pub host: bool,
    pub policy: Option<Policy>,
    pub laser: bool,
    pub paper: PaperSetting,
    pub persons: Vec<Person>,
    pub boards: Vec<BoardRow>,
    pub used: usize,
    pub quota: usize,
    /// `chat.send` is granted (plain-text lines).
    pub lines: bool,
    /// A board is open.
    pub open: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExportForm {
    pub format: FileFormat,
    pub area: Area,
    pub scale: u8,
    pub transparent: bool,
    pub names: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    Export(ExportForm),
    Post {
        area: Area,
    },
    Clear,
    /// The board was cleared; Undo works until `until` (Unix seconds).
    Cleared {
        until: u64,
    },
    Settings {
        confirm_remove: Option<AuthorKey>,
    },
    Boards {
        confirm_delete: Option<BoardId>,
    },
    Describe(Vec<String>),
    Notice {
        title: String,
        lines: Vec<String>,
    },
}

/// What the glue has to do after a click.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    Redraw,
    Close,
    Export(ExportReq),
    Post(Area),
    Clear,
    UndoClear,
    Policy(Policy),
    Allow { key: String, on: bool },
    Hide { author: AuthorKey, on: bool },
    RemoveAuthor(AuthorKey),
    Handover(AuthorKey),
    Laser(bool),
    Paper(PaperSetting),
    Title(String),
    Line(Line),
    OpenBoard(BoardId),
    DeleteBoard(BoardId),
    ShowBoards,
}

/// The open dialog: what it shows, a status note and whether an action is running.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dialog {
    pub view: View,
    pub note: Option<(Tone, String)>,
    pub busy: bool,
}

impl Dialog {
    pub fn new(view: View) -> Dialog {
        Dialog {
            view,
            note: None,
            busy: false,
        }
    }

    pub fn export(ctx: &Ctx) -> Dialog {
        Dialog::new(View::Export(ExportForm {
            area: default_area(ctx),
            scale: 2,
            ..ExportForm::default()
        }))
    }

    pub fn post(ctx: &Ctx) -> Dialog {
        Dialog::new(View::Post {
            area: default_area(ctx),
        })
    }

    pub fn notice(title: &str, lines: Vec<String>) -> Dialog {
        Dialog::new(View::Notice {
            title: title.to_owned(),
            lines,
        })
    }

    /// Sets the status line under the dialog and ends the busy state.
    pub fn set_note(&mut self, tone: Tone, text: String) {
        self.note = Some((tone, text));
        self.busy = false;
    }

    /// The dialog title (Mumcord draws it in the header, at most 64 characters).
    pub fn title(&self, ctx: &Ctx) -> String {
        let s = match &self.view {
            View::Export(_) => t_with("dialog.export.title", &[("title", &ctx.title)]),
            View::Post { .. } => t("dialog.post.title"),
            View::Clear => t("dialog.clear.title"),
            View::Cleared { .. } => t("dialog.cleared.title"),
            View::Settings { .. } => t("dialog.settings.title"),
            View::Boards { .. } => t("dialog.boards.title"),
            View::Describe(_) => t("dialog.describe.title"),
            View::Notice { title, .. } => title.clone(),
        };
        clip_to(&s, 64)
    }

    pub fn render(&self, ctx: &Ctx) -> El {
        let body = match &self.view {
            View::Export(f) => export_view(f, ctx, self.busy),
            View::Post { area } => post_view(*area, ctx, self.busy),
            View::Clear => clear_view(ctx),
            View::Cleared { until } => cleared_view(*until),
            View::Settings { confirm_remove } => settings_view(ctx, *confirm_remove),
            View::Boards { confirm_delete } => boards_view(ctx, *confirm_delete),
            View::Describe(lines) | View::Notice { lines, .. } => text_view(lines),
        };
        match &self.note {
            Some((tone, text)) => body.child(ui::chip(&clip(text)).tone(*tone)),
            None => body,
        }
    }

    /// A click or change in the dialog.
    pub fn on_action(&mut self, action: &str, value: &ActionValue, ctx: &Ctx) -> Effect {
        let text = match value {
            ActionValue::Text(s) => Some(s.as_str()),
            _ => None,
        };
        let on = match value {
            ActionValue::Boolean(b) => Some(*b),
            _ => None,
        };
        if matches!(action, "close" | "done") {
            return Effect::Close;
        }
        match &mut self.view {
            View::Export(f) => match (action, text, on) {
                ("format", Some(v), _) => {
                    if let Some(fmt) = FileFormat::ALL.into_iter().find(|x| x.name() == v) {
                        f.format = fmt;
                    }
                    Effect::Redraw
                }
                ("area", Some(v), _) => {
                    if let Some(a) = Area::from_name(v) {
                        f.area = a;
                    }
                    Effect::Redraw
                }
                ("scale", Some(v), _) => {
                    if let Some(s) = ["1", "2", "4"].iter().position(|x| *x == v) {
                        f.scale = [1, 2, 4][s];
                    }
                    Effect::Redraw
                }
                ("transparent", _, Some(b)) => {
                    f.transparent = b;
                    Effect::Redraw
                }
                ("names", _, Some(b)) => {
                    f.names = b;
                    Effect::Redraw
                }
                ("save", ..) if !self.busy => match area_problem(f.area, ctx) {
                    Some(problem) => {
                        self.note = Some((Tone::Warning, problem));
                        Effect::Redraw
                    }
                    None => {
                        self.busy = true;
                        self.note = None;
                        Effect::Export(ExportReq {
                            format: f.format,
                            area: f.area,
                            scale: f.scale,
                            transparent: f.transparent,
                            names: f.names,
                        })
                    }
                },
                _ => Effect::None,
            },
            View::Post { area } => match (action, text) {
                ("area", Some(v)) => {
                    if let Some(a) = Area::from_name(v) {
                        *area = a;
                    }
                    Effect::Redraw
                }
                ("post", _) if !self.busy => match area_problem(*area, ctx) {
                    Some(problem) => {
                        self.note = Some((Tone::Warning, problem));
                        Effect::Redraw
                    }
                    None => {
                        self.busy = true;
                        self.note = None;
                        Effect::Post(*area)
                    }
                },
                _ => Effect::None,
            },
            View::Clear => match action {
                "clear" => Effect::Clear,
                _ => Effect::None,
            },
            View::Cleared { .. } => match action {
                "undo" => Effect::UndoClear,
                _ => Effect::None,
            },
            View::Settings { confirm_remove } => {
                settings_action(action, text, on, confirm_remove, ctx)
            }
            View::Boards { confirm_delete } => match action.split_once(':') {
                Some(("open", hex)) => parse_hex_id(hex).map_or(Effect::None, Effect::OpenBoard),
                Some(("delete", hex)) => {
                    *confirm_delete = parse_hex_id(hex);
                    Effect::Redraw
                }
                _ if action == "delete-confirm" => match confirm_delete.take() {
                    Some(id) => Effect::DeleteBoard(id),
                    None => Effect::None,
                },
                _ if action == "delete-cancel" => {
                    *confirm_delete = None;
                    Effect::Redraw
                }
                _ => Effect::None,
            },
            View::Describe(_) | View::Notice { .. } => Effect::None,
        }
    }
}

fn default_area(ctx: &Ctx) -> Area {
    if ctx.selected > 0 {
        Area::Selection
    } else {
        Area::Board
    }
}

fn area_problem(area: Area, ctx: &Ctx) -> Option<String> {
    match area {
        Area::Selection if ctx.selected == 0 => Some(t("dialog.area.nothingSelected")),
        Area::View if !ctx.has_view => Some(t("dialog.area.noView")),
        _ => None,
    }
}

fn buttons(children: Vec<El>) -> El {
    ui::stack(Direction::Row, 8, Align::Center)
        .justify(Justify::End)
        .children(children)
}

fn area_select(area: Area, ctx: &Ctx) -> El {
    let board = tn("dialog.area.board", ctx.items as u64, &[]);
    let view = t("dialog.area.view");
    let selection = if ctx.selected == 0 {
        t("dialog.area.selectionNone")
    } else {
        tn("dialog.area.selection", ctx.selected as u64, &[])
    };
    ui::select(
        "area",
        &t("dialog.area.label"),
        &[
            ("board", &board),
            ("view", &view),
            ("selection", &selection),
        ],
        Some(area.name()),
    )
}

fn export_view(f: &ExportForm, ctx: &Ctx, busy: bool) -> El {
    let png = t("dialog.export.png");
    let svg = t("dialog.export.svg");
    let pdf = t("dialog.export.pdf");
    let board = t("dialog.export.board");
    let mut col = ui::column()
        .child(ui::select(
            "format",
            &t("dialog.export.format"),
            &[
                ("png", &png),
                ("svg", &svg),
                ("pdf", &pdf),
                ("board", &board),
            ],
            Some(f.format.name()),
        ))
        .child(ui::caption(&t(match f.format {
            FileFormat::Png => "dialog.export.pngHint",
            FileFormat::Svg | FileFormat::Pdf => "dialog.export.vectorHint",
            FileFormat::Board => "dialog.export.boardHint",
        })));
    // A board file always holds the whole board.
    if f.format != FileFormat::Board {
        col = col.child(area_select(f.area, ctx));
    }
    if f.format == FileFormat::Png {
        let one = t("dialog.export.scale1");
        let two = t("dialog.export.scale2");
        let four = t("dialog.export.scale4");
        col = col.child(ui::select(
            "scale",
            &t("dialog.export.scale"),
            &[("1", &one), ("2", &two), ("4", &four)],
            Some(&f.scale.to_string()),
        ));
    }
    if f.format != FileFormat::Board {
        col = col.child(ui::toggle(
            "transparent",
            &t("dialog.export.transparent"),
            f.transparent,
        ));
    }
    if matches!(f.format, FileFormat::Svg | FileFormat::Pdf) {
        col = col
            .child(ui::toggle("names", &t("dialog.export.names"), f.names))
            .child(ui::caption(&t("dialog.export.namesHint")));
    }
    col.child(ui::caption(&t("dialog.export.privacy")))
        .child(buttons(vec![
            ui::button("close", &t("dialog.cancel")).kind(ButtonKind::Secondary),
            ui::button("save", &t("dialog.export.save"))
                .kind(ButtonKind::Primary)
                .disabled(busy),
        ]))
}

fn post_view(area: Area, ctx: &Ctx, busy: bool) -> El {
    ui::column()
        .child(ui::text(&t("dialog.post.intro")))
        .child(area_select(area, ctx))
        .child(ui::caption(&t("dialog.post.hint")))
        .child(buttons(vec![
            ui::button("close", &t("dialog.cancel")).kind(ButtonKind::Secondary),
            ui::button("post", &t("dialog.post.button"))
                .kind(ButtonKind::Primary)
                .disabled(busy),
        ]))
}

fn clear_view(ctx: &Ctx) -> El {
    let items = tn("dialog.clear.items", ctx.items as u64, &[]);
    let people = tn("dialog.clear.people", ctx.people as u64, &[]);
    ui::column()
        .child(ui::text(&clip(&t_with(
            "dialog.clear.body",
            &[("items", &items), ("people", &people)],
        ))))
        .child(ui::text(&t("dialog.clear.undoHint")))
        .child(buttons(vec![
            ui::button("close", &t("dialog.clear.keep")).kind(ButtonKind::Secondary),
            ui::button("clear", &t("dialog.clear.button")).kind(ButtonKind::Danger),
        ]))
}

fn cleared_view(until: u64) -> El {
    ui::column()
        .child(ui::text(&t("dialog.cleared.body")))
        .child(ui::countdown(&t("dialog.cleared.countdown"), until))
        .child(buttons(vec![
            ui::button("close", &t("dialog.done")).kind(ButtonKind::Secondary),
            ui::button("undo", &t("dialog.cleared.undo")).kind(ButtonKind::Primary),
        ]))
}

fn settings_action(
    action: &str,
    text: Option<&str>,
    on: Option<bool>,
    confirm_remove: &mut Option<AuthorKey>,
    ctx: &Ctx,
) -> Effect {
    let person = |s: &str| {
        s.parse::<AuthorKey>()
            .ok()
            .and_then(|a| ctx.persons.iter().find(|p| p.author == a))
    };
    match (action.split_once(':'), text, on) {
        (Some(("allow", a)), _, Some(on)) => match person(a).and_then(|p| p.key.clone()) {
            Some(key) => Effect::Allow { key, on },
            None => Effect::None,
        },
        (Some(("hide", a)), _, Some(on)) => person(a).map_or(Effect::None, |p| Effect::Hide {
            author: p.author,
            on,
        }),
        (Some(("remove", a)), ..) => {
            *confirm_remove = person(a).map(|p| p.author);
            Effect::Redraw
        }
        (Some(("handover", a)), ..) => {
            person(a).map_or(Effect::None, |p| Effect::Handover(p.author))
        }
        _ => match (action, text, on) {
            ("policy", Some(v), _) => {
                crate::bridge::policy_from_name(v).map_or(Effect::None, Effect::Policy)
            }
            ("laser", _, Some(b)) => Effect::Laser(b),
            ("paper", Some(v), _) => PaperSetting::from_name(v).map_or(Effect::None, Effect::Paper),
            ("title", Some(v), _) => {
                Effect::Title(v.trim().chars().take(MAX_TITLE_CHARS).collect())
            }
            ("remove-confirm", ..) => match confirm_remove.take() {
                Some(a) => Effect::RemoveAuthor(a),
                None => Effect::None,
            },
            ("remove-cancel", ..) => {
                *confirm_remove = None;
                Effect::Redraw
            }
            ("clear", ..) => Effect::Clear,
            ("line-start", ..) => Effect::Line(Line::Start),
            ("line-summary", ..) => Effect::Line(Line::Summary),
            ("boards", ..) => Effect::ShowBoards,
            _ => Effect::None,
        },
    }
}

fn settings_view(ctx: &Ctx, confirm_remove: Option<AuthorKey>) -> El {
    let mut col = ui::column();
    if !ctx.open {
        return col
            .child(ui::text(&t("dialog.settings.noBoard")))
            .child(buttons(vec![
                ui::button("boards", &t("dialog.settings.boards")).kind(ButtonKind::Secondary),
                ui::button("done", &t("dialog.done")).kind(ButtonKind::Primary),
            ]));
    }
    col = col.child(ui::caption(&t(if ctx.host {
        "dialog.settings.introHost"
    } else {
        "dialog.settings.intro"
    })));
    if ctx.host {
        col = col.child(
            ui::text_input("title", &t("dialog.settings.boardTitle"), &ctx.title, 64)
                .help(&t("dialog.settings.boardTitleHint")),
        );
        let everyone = t("dialog.settings.policy.everyone");
        let selected = t("dialog.settings.policy.selected");
        let host = t("dialog.settings.policy.host");
        col = col
            .child(ui::select(
                "policy",
                &t("dialog.settings.policy"),
                &[
                    ("everyone", &everyone),
                    ("selected", &selected),
                    ("host", &host),
                ],
                ctx.policy.map(crate::bridge::policy_name),
            ))
            .child(ui::caption(&t(match ctx.policy {
                Some(Policy::Selected) => "dialog.settings.policy.selectedHint",
                Some(Policy::HostOnly) => "dialog.settings.policy.hostHint",
                _ => "dialog.settings.policy.everyoneHint",
            })))
            .child(ui::toggle("laser", &t("dialog.settings.laser"), ctx.laser))
            .child(ui::caption(&t("dialog.settings.laserHint")));
    } else {
        col = col.child(ui::text(&t(match ctx.policy {
            Some(Policy::Selected) => "dialog.settings.policy.selectedHint",
            Some(Policy::HostOnly) => "dialog.settings.policy.hostHint",
            _ => "dialog.settings.policy.everyoneHint",
        })));
    }
    let mut list = ui::list(&t("dialog.settings.people"));
    for p in ctx.persons.iter().take(48) {
        let mut row = ui::stack(Direction::Row, 8, Align::Center).justify(Justify::FillFirst);
        let mut name = clip_to(&p.name, 40);
        if p.host {
            name = t_with("dialog.settings.hostName", &[("name", &name)]);
        }
        if p.me {
            name = t_with("dialog.settings.you", &[("name", &name)]);
        }
        row = row.child(ui::text(&clip(&name)));
        if p.hidden {
            row = row.child(ui::chip(&t("dialog.settings.hiddenChip")).tone(Tone::Neutral));
        }
        if ctx.host && ctx.policy == Some(Policy::Selected) && !p.host {
            row = row.child(
                ui::toggle(
                    &format!("allow:{}", p.author),
                    &t_with("dialog.settings.allow", &[("name", &clip_to(&p.name, 40))]),
                    p.allowed,
                )
                .disabled(p.key.is_none()),
            );
        }
        if !p.me {
            row = row.child(ui::toggle(
                &format!("hide:{}", p.author),
                &t_with("dialog.settings.hide", &[("name", &clip_to(&p.name, 40))]),
                p.hidden,
            ));
        }
        if ctx.host && !p.me {
            row = row
                .child(
                    ui::button(
                        &format!("remove:{}", p.author),
                        &t("dialog.settings.remove"),
                    )
                    .kind(ButtonKind::Secondary),
                )
                .child(
                    ui::button(
                        &format!("handover:{}", p.author),
                        &t("dialog.settings.handover"),
                    )
                    .kind(ButtonKind::Secondary),
                );
        }
        list = list.child(row);
    }
    col = col.child(list);
    if let Some(a) = confirm_remove {
        let name = ctx
            .persons
            .iter()
            .find(|p| p.author == a)
            .map_or_else(String::new, |p| clip_to(&p.name, 40));
        col = col
            .child(ui::text(&clip(&t_with(
                "dialog.settings.removeConfirm",
                &[("name", &name)],
            ))))
            .child(buttons(vec![
                ui::button("remove-cancel", &t("dialog.cancel")).kind(ButtonKind::Secondary),
                ui::button("remove-confirm", &t("dialog.settings.removeButton"))
                    .kind(ButtonKind::Danger),
            ]));
    }
    let auto = t("dialog.settings.paper.auto");
    let light = t("dialog.settings.paper.light");
    let dark = t("dialog.settings.paper.dark");
    let dots = t("dialog.settings.paper.dots");
    let grid = t("dialog.settings.paper.grid");
    let plain = t("dialog.settings.paper.plain");
    col = col.child(ui::select(
        "paper",
        &t("dialog.settings.paper"),
        &[
            ("auto", &auto),
            ("light", &light),
            ("dark", &dark),
            ("dots", &dots),
            ("grid", &grid),
            ("plain", &plain),
        ],
        Some(ctx.paper.name()),
    ));
    if ctx.host {
        col = col
            .child(ui::divider())
            .child(ui::text(&t("dialog.settings.clearTitle")))
            .child(ui::caption(&t("dialog.settings.clearHint")))
            .child(ui::button("clear", &t("dialog.settings.clearButton")).kind(ButtonKind::Danger));
        if ctx.lines {
            col = col
                .child(ui::divider())
                .child(ui::text(&t("dialog.settings.linesTitle")))
                .child(ui::caption(&t("dialog.settings.linesHint")))
                .child(
                    ui::row()
                        .child(
                            ui::button("line-start", &t("dialog.settings.lineStart"))
                                .kind(ButtonKind::Secondary),
                        )
                        .child(
                            ui::button("line-summary", &t("dialog.settings.lineSummary"))
                                .kind(ButtonKind::Secondary),
                        ),
                );
        }
    }
    col.child(ui::divider()).child(buttons(vec![
        ui::button("boards", &t("dialog.settings.boards")).kind(ButtonKind::Secondary),
        ui::button("done", &t("dialog.done")).kind(ButtonKind::Primary),
    ]))
}

fn kib(bytes: usize) -> String {
    bytes.div_ceil(1024).to_string()
}

fn boards_view(ctx: &Ctx, confirm_delete: Option<BoardId>) -> El {
    let quota = ctx.quota.max(1);
    let mut col = ui::column()
        .child(ui::meter(
            &t_with(
                "dialog.boards.storage",
                &[("used", &kib(ctx.used)), ("quota", &kib(ctx.quota))],
            ),
            (ctx.used as f32 / quota as f32).clamp(0.0, 1.0),
        ))
        .child(ui::caption(&t("dialog.boards.privacy")));
    if ctx.boards.is_empty() {
        col = col.child(ui::text(&t("dialog.boards.none")));
    } else {
        let mut list = ui::list(&t("dialog.boards.title"));
        for b in ctx.boards.iter().take(50) {
            let hex = hex_id(&b.id);
            let title = if b.title.is_empty() {
                t("board.untitled")
            } else {
                b.title.clone()
            };
            let detail = t_with(
                "dialog.boards.detail",
                &[("ago", &b.ago), ("size", &kib(b.bytes as usize))],
            );
            let mut row = ui::stack(Direction::Row, 8, Align::Center)
                .justify(Justify::FillFirst)
                .child(
                    ui::button(&format!("open:{hex}"), &clip(&title))
                        .detail(&clip(&detail))
                        .kind(ButtonKind::Secondary)
                        .disabled(b.open || ctx.open),
                );
            row = row.child(
                ui::button(&format!("delete:{hex}"), &t("dialog.boards.delete"))
                    .kind(ButtonKind::Secondary)
                    .disabled(b.open),
            );
            list = list.child(row);
        }
        col = col.child(list);
    }
    if let Some(id) = confirm_delete {
        let title = ctx
            .boards
            .iter()
            .find(|b| b.id == id)
            .map_or_else(String::new, |b| b.title.clone());
        col = col
            .child(ui::text(&clip(&t_with(
                "dialog.boards.deleteConfirm",
                &[("title", &title)],
            ))))
            .child(buttons(vec![
                ui::button("delete-cancel", &t("dialog.cancel")).kind(ButtonKind::Secondary),
                ui::button("delete-confirm", &t("dialog.boards.deleteButton"))
                    .kind(ButtonKind::Danger),
            ]));
    }
    col.child(buttons(vec![
        ui::button("done", &t("dialog.done")).kind(ButtonKind::Primary),
    ]))
}

fn text_view(lines: &[String]) -> El {
    ui::column()
        .children(lines.iter().map(|l| ui::text(&clip(l))))
        .child(buttons(vec![
            ui::button("done", &t("dialog.done")).kind(ButtonKind::Primary),
        ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mumcord_plugin_sdk::ui::Component;

    fn ctx() -> Ctx {
        Ctx {
            title: "Plan".into(),
            items: 143,
            people: 5,
            selected: 0,
            has_view: true,
            host: true,
            policy: Some(Policy::Everyone),
            open: true,
            lines: true,
            quota: 1024 * 1024,
            persons: vec![
                Person {
                    author: 1,
                    name: "Maya".into(),
                    key: Some("k1".into()),
                    me: true,
                    host: true,
                    allowed: true,
                    hidden: false,
                },
                Person {
                    author: 2,
                    name: "Ben".into(),
                    key: None,
                    me: false,
                    host: false,
                    allowed: false,
                    hidden: false,
                },
            ],
            ..Ctx::default()
        }
    }

    fn texts(el: El) -> String {
        el.into_nodes()
            .into_iter()
            .map(|n| match n.component {
                Component::Text(t) => t.text,
                Component::Button(b) => b.label,
                Component::Chip(c) => c.text,
                Component::Toggle(t) => t.label,
                Component::Select(s) => s
                    .options
                    .into_iter()
                    .map(|o| o.label)
                    .collect::<Vec<_>>()
                    .join("|"),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn export_defaults_keep_names_off_and_save_is_the_gesture() {
        let c = ctx();
        let mut d = Dialog::export(&c);
        let View::Export(f) = &d.view else {
            panic!("not export")
        };
        assert!(!f.names);
        assert_eq!(f.scale, 2);
        assert_eq!(
            d.on_action("format", &ActionValue::Text("pdf".into()), &c),
            Effect::Redraw
        );
        assert!(texts(d.render(&c)).contains("Add names of who drew what"));
        assert_eq!(
            d.on_action("save", &ActionValue::None, &c),
            Effect::Export(ExportReq {
                format: FileFormat::Pdf,
                area: Area::Board,
                scale: 2,
                transparent: false,
                names: false
            })
        );
        // A second click while it runs does nothing.
        assert_eq!(d.on_action("save", &ActionValue::None, &c), Effect::None);
    }

    #[test]
    fn an_empty_selection_cannot_be_posted() {
        let c = ctx();
        let mut d = Dialog::post(&c);
        d.on_action("area", &ActionValue::Text("selection".into()), &c);
        assert_eq!(d.on_action("post", &ActionValue::None, &c), Effect::Redraw);
        assert!(d.note.is_some());
        d.on_action("area", &ActionValue::Text("board".into()), &c);
        assert_eq!(
            d.on_action("post", &ActionValue::None, &c),
            Effect::Post(Area::Board)
        );
    }

    #[test]
    fn clear_confirm_names_items_and_people() {
        let c = ctx();
        let d = Dialog::new(View::Clear);
        let s = texts(d.render(&c));
        assert!(
            s.contains("This removes 143 items for all 5 people on the board."),
            "{s}"
        );
        assert!(s.contains("Keep the board"));
        assert!(s.contains("Clear board"));
    }

    #[test]
    fn settings_checks_people_and_confirms_removal() {
        let c = ctx();
        let mut d = Dialog::new(View::Settings {
            confirm_remove: None,
        });
        assert_eq!(
            d.on_action("hide:2", &ActionValue::Boolean(true), &c),
            Effect::Hide {
                author: 2,
                on: true
            }
        );
        assert_eq!(
            d.on_action("hide:9", &ActionValue::Boolean(true), &c),
            Effect::None
        );
        // Ben has no certificate, so he cannot be put on the allow list.
        assert_eq!(
            d.on_action("allow:2", &ActionValue::Boolean(true), &c),
            Effect::None
        );
        assert_eq!(
            d.on_action("remove:2", &ActionValue::None, &c),
            Effect::Redraw
        );
        assert_eq!(
            d.on_action("remove-confirm", &ActionValue::None, &c),
            Effect::RemoveAuthor(2)
        );
        assert_eq!(
            d.on_action("policy", &ActionValue::Text("host".into()), &c),
            Effect::Policy(Policy::HostOnly)
        );
        assert!(d.render(&c).into_nodes().len() < 500);
    }

    #[test]
    fn boards_delete_needs_a_confirmation() {
        let mut c = ctx();
        c.open = false;
        c.boards = vec![BoardRow {
            id: [7; 16],
            title: "Old".into(),
            ago: "2 days ago".into(),
            bytes: 2048,
            open: false,
        }];
        let mut d = Dialog::new(View::Boards {
            confirm_delete: None,
        });
        let hex = hex_id(&[7; 16]);
        assert_eq!(
            d.on_action(&format!("delete:{hex}"), &ActionValue::None, &c),
            Effect::Redraw
        );
        assert!(texts(d.render(&c)).contains("Delete \"Old\" from this device?"));
        assert_eq!(
            d.on_action("delete-confirm", &ActionValue::None, &c),
            Effect::DeleteBoard([7; 16])
        );
        assert_eq!(
            d.on_action(&format!("open:{hex}"), &ActionValue::None, &c),
            Effect::OpenBoard([7; 16])
        );
    }
}
