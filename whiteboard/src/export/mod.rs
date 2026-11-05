//! WB7: vector export of a board (SVG and PDF) plus export file names.
//!
//! Both writers draw the same [`Scene`]: the visible objects of a scope in world pixels, with
//! the look of the page renderer (`web/board/render.js`): ink and note colours per paper, round
//! caps, smoothed strokes, tilted notes, open arrow heads. There are no font files in an export;
//! SVG names the hand-written family with fallbacks, PDF uses the standard Helvetica font.
//! Author names appear only when [`Options::with_names`] is set.

pub mod pdf;
pub mod svg;

use std::collections::BTreeSet;

use crate::model::{Board, Kind, ObjId, Object};

/// A rectangle in world pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    /// Turns negative sizes into a positive rectangle.
    fn norm(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect {
            x: x.min(x + w),
            y: y.min(y + h),
            w: w.abs(),
            h: h.abs(),
        }
    }

    fn inflate(self, d: f64) -> Rect {
        Rect {
            x: self.x - d,
            y: self.y - d,
            w: self.w + 2.0 * d,
            h: self.h + 2.0 * d,
        }
    }

    fn union(self, o: Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect {
            x,
            y,
            w: (self.x + self.w).max(o.x + o.w) - x,
            h: (self.y + self.h).max(o.y + o.h) - y,
        }
    }

    fn intersects(&self, o: &Rect) -> bool {
        self.x <= o.x + o.w
            && o.x <= self.x + self.w
            && self.y <= o.y + o.h
            && o.y <= self.y + self.h
    }

    fn is_valid(&self) -> bool {
        [self.x, self.y, self.w, self.h]
            .iter()
            .all(|v| v.is_finite())
            && self.w > 0.0
            && self.h > 0.0
    }
}

/// What to export.
#[derive(Clone, Debug, PartialEq)]
pub enum Scope {
    /// Every visible object.
    Board,
    /// Exactly this world rectangle (the current view) and what touches it.
    View(Rect),
    /// Only these objects.
    Selection(BTreeSet<ObjId>),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Paper {
    #[default]
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub paper: Paper,
    /// Leave out the paper colour.
    pub transparent: bool,
    /// Include author names (off by default for privacy).
    pub with_names: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Svg,
    Pdf,
    /// The board file (`.mcwb`) from [`crate::store::encode_board`].
    Board,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Svg => "svg",
            Format::Pdf => "pdf",
            Format::Board => "mcwb",
        }
    }
}

const MAX_FILE_STEM_CHARS: usize = 64;
const DEFAULT_FILE_STEM: &str = "Whiteboard";

/// A file name from the board title that is safe on Windows, macOS and Linux.
pub fn file_name(title: &str, format: Format) -> String {
    let mut stem = String::new();
    for c in title.chars() {
        let c = if c.is_control() || "/\\:*?\"<>|".contains(c) {
            ' '
        } else {
            c
        };
        // Collapse runs of whitespace so stripped characters leave no gaps.
        if !(c.is_whitespace() && stem.ends_with(' ')) {
            stem.push(if c.is_whitespace() { ' ' } else { c });
        }
    }
    let mut stem: String = stem
        .trim_matches(|c: char| c == ' ' || c == '.')
        .chars()
        .take(MAX_FILE_STEM_CHARS)
        .collect();
    stem = stem.trim_end_matches([' ', '.']).to_string();
    if stem.is_empty() {
        stem = DEFAULT_FILE_STEM.to_string();
    }
    let upper = stem.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.len() == 4
            && upper.as_bytes()[3].is_ascii_digit());
    if reserved {
        stem.insert(0, '_');
    }
    format!("{stem}.{}", format.extension())
}

// ---- look (mirrors web/board/board.css and render.js) ---------------------------------------

pub(crate) struct Palette {
    pub paper: &'static str,
    pub box_fill: &'static str,
    pub inks: [&'static str; 6],
    pub hl: &'static str,
    pub hl_alpha: f64,
    pub multiply: bool,
    pub notes: [&'static str; 5],
    pub note_text: &'static str,
}

pub(crate) fn palette(paper: Paper) -> Palette {
    match paper {
        Paper::Light => Palette {
            paper: "#fbfaf6",
            box_fill: "#ffffff",
            inks: [
                "#23232b", "#d03a2f", "#cf6a08", "#1f8a55", "#1f63c9", "#7b4fd0",
            ],
            hl: "#ffe94d",
            hl_alpha: 0.62,
            multiply: true,
            notes: ["#ffe27a", "#ffb3c8", "#a8ecc4", "#a9d6ff", "#d4c4ff"],
            note_text: "#24211a",
        },
        Paper::Dark => Palette {
            paper: "#16181d",
            box_fill: "#1d2027",
            inks: [
                "#ecebe6", "#ff7a70", "#ffa94d", "#5fd39a", "#6fb3ff", "#b69cff",
            ],
            hl: "#f4dc4f",
            hl_alpha: 0.62,
            multiply: false,
            notes: ["#dcbc4a", "#e58aa8", "#6fcf9f", "#6fb2e8", "#a995e8"],
            note_text: "#24211a",
        },
    }
}

impl Palette {
    pub fn ink(&self, item: &Item) -> &'static str {
        if item.kind == Kind::Highlight && item.ink == 0 {
            self.hl
        } else {
            self.inks[usize::from(item.ink).min(self.inks.len() - 1)]
        }
    }

    pub fn note(&self, item: &Item) -> &'static str {
        self.notes[usize::from(item.fill).min(self.notes.len() - 1)]
    }
}

/// `#rrggbb` as 0..1 components.
pub(crate) fn rgb(hex: &str) -> (f64, f64, f64) {
    let c = |i: usize| {
        hex.get(i..i + 2)
            .and_then(|s| u8::from_str_radix(s, 16).ok())
            .map_or(0.0, |v| f64::from(v) / 255.0)
    };
    (c(1), c(3), c(5))
}

pub(crate) const HAND_FAMILY: &str = "Caveat, 'Segoe Print', 'Comic Sans MS', cursive";
pub(crate) const NOTE_FONT: f64 = 25.0;
pub(crate) const NOTE_LINE: f64 = 28.0;
pub(crate) const NOTE_PAD: f64 = 14.0;
pub(crate) const NOTE_RADIUS: f64 = 3.0;
pub(crate) const NAME_FONT: f64 = 12.0;
const TEXT_SIZES: [f64; 3] = [22.0, 30.0, 40.0];
const WIDTHS: [f64; 3] = [2.0, 4.0, 7.0];
const HL_WIDTHS: [f64; 3] = [12.0, 22.0, 32.0];
/// Space around the content of a board or selection export.
const MARGIN: f64 = 24.0;
/// Page size of an empty export.
const EMPTY: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: 320.0,
    h: 240.0,
};
/// Average glyph width relative to the font size, for wrapping without font metrics.
pub(crate) const HAND_CHAR_WIDTH: f64 = 0.42;
/// Distance from the top of a text line to its baseline, relative to the font size.
pub(crate) const ASCENT: f64 = 0.8;

/// One visible object in world pixels.
pub(crate) struct Item {
    pub id: ObjId,
    pub kind: Kind,
    /// Raw geometry: lines and arrows run from (x, y) to (x + w, y + h); sizes may be negative.
    pub g: Rect,
    /// World points of strokes and highlights.
    pub points: Vec<(f64, f64)>,
    pub ink: u8,
    pub width: u8,
    pub fill: u8,
    pub text: String,
    pub alt: String,
    /// Author name, only with [`Options::with_names`].
    pub author: Option<String>,
}

impl Item {
    fn from_object(o: &Object, board: &Board, with_names: bool) -> Option<Item> {
        let kind = o.kind?;
        let q = |v: i32| f64::from(v) / 4.0;
        // Base points are local to the original bounding box; geometry scales them like the page.
        let bw = o.base.iter().map(|p| p.0).max().unwrap_or(0);
        let bh = o.base.iter().map(|p| p.1).max().unwrap_or(0);
        let geom = o.geom.as_ref().map_or(
            crate::model::Geom {
                x: 0,
                y: 0,
                w: bw,
                h: bh,
            },
            |r| r.val,
        );
        let sx = if bw > 0 {
            f64::from(geom.w) / f64::from(bw)
        } else {
            1.0
        };
        let sy = if bh > 0 {
            f64::from(geom.h) / f64::from(bh)
        } else {
            1.0
        };
        let points = o
            .base
            .iter()
            .map(|&(x, y)| (q(geom.x) + q(x) * sx, q(geom.y) + q(y) * sy))
            .collect();
        let author = with_names
            .then(|| board.author(o.id.author).map(|a| a.name.clone()))
            .flatten()
            .filter(|n| !n.is_empty());
        Some(Item {
            id: o.id,
            kind,
            g: Rect {
                x: q(geom.x),
                y: q(geom.y),
                w: q(geom.w),
                h: q(geom.h),
            },
            points,
            ink: o.ink.as_ref().map_or(0, |r| r.val),
            width: o.width.as_ref().map_or(0, |r| r.val),
            fill: o.fill.as_ref().map_or(0, |r| r.val),
            text: o.text.as_ref().map_or_else(String::new, |r| r.val.clone()),
            alt: o.alt.as_ref().map_or_else(String::new, |r| r.val.clone()),
            author,
        })
    }

    pub fn stroke_width(&self) -> f64 {
        let i = usize::from(self.width).min(2);
        if self.kind == Kind::Highlight {
            HL_WIDTHS[i]
        } else {
            WIDTHS[i]
        }
    }

    pub fn rect(&self) -> Rect {
        Rect::norm(self.g.x, self.g.y, self.g.w, self.g.h)
    }

    pub fn text_size(&self) -> f64 {
        TEXT_SIZES[usize::from(self.width).min(2)]
    }

    pub fn arrow_head_size(&self) -> f64 {
        10.0 + self.stroke_width() * 2.0
    }

    /// The two outer points of an open arrow head at the line's end.
    pub fn arrow_head(&self) -> Option<[(f64, f64); 2]> {
        let (x0, y0, x1, y1) = (self.g.x, self.g.y, self.g.x + self.g.w, self.g.y + self.g.h);
        if self.kind != Kind::Arrow || (self.g.w == 0.0 && self.g.h == 0.0) {
            return None;
        }
        let a = (y1 - y0).atan2(x1 - x0);
        let s = self.arrow_head_size();
        Some([
            (x1 - s * (a - 0.45).cos(), y1 - s * (a - 0.45).sin()),
            (x1 - s * (a + 0.45).cos(), y1 - s * (a + 0.45).sin()),
        ])
    }

    /// Same small tilt as the page (`noteTilt` in render.js): FNV-1a over "<author>.<counter>".
    pub fn note_tilt_deg(&self) -> f64 {
        let key = format!("{}.{}", self.id.author, self.id.counter);
        let h = key.bytes().fold(2_166_136_261u32, |h, b| {
            (h ^ u32::from(b)).wrapping_mul(16_777_619)
        });
        (f64::from(h % 9) - 4.0) * 0.5
    }

    /// Note text lines that fit the note, like the page; an author name takes the last line.
    pub fn note_lines(&self, char_width: f64) -> Vec<String> {
        let r = self.rect();
        let name_room = if self.author.is_some() {
            NOTE_LINE
        } else {
            0.0
        };
        let max = (((r.h - 22.0 - name_room) / NOTE_LINE).floor() as usize).max(1);
        let mut lines = wrap_text(&self.text, r.w - 2.0 * NOTE_PAD, char_width);
        lines.truncate(max);
        lines
    }

    /// Area the object paints, for scoping and the page size.
    fn bounds(&self) -> Rect {
        let half = self.stroke_width() / 2.0;
        match self.kind {
            Kind::Stroke | Kind::Highlight => {
                let mut it = self.points.iter();
                let Some(&(x, y)) = it.next() else {
                    return self.rect().inflate(half);
                };
                it.fold(
                    Rect {
                        x,
                        y,
                        w: 0.0,
                        h: 0.0,
                    },
                    |r, &(x, y)| {
                        r.union(Rect {
                            x,
                            y,
                            w: 0.0,
                            h: 0.0,
                        })
                    },
                )
                .inflate(half)
            }
            Kind::Line => self.rect().inflate(half),
            Kind::Arrow => self.rect().inflate(half + self.arrow_head_size()),
            // Tilt and shadow reach a little outside the note.
            Kind::Note => self.rect().inflate(8.0),
            Kind::Text => {
                let size = self.text_size();
                let lines = self.text.split('\n').collect::<Vec<_>>();
                let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
                Rect {
                    x: self.g.x,
                    y: self.g.y,
                    w: (longest as f64 * size * HAND_CHAR_WIDTH).max(self.g.w.abs()),
                    h: (lines.len() as f64 * size * 1.1).max(self.g.h.abs()),
                }
            }
            _ => self.rect().inflate(half),
        }
    }
}

/// Breaks text into lines no wider than `max_width` (estimated per character), keeping explicit
/// line breaks; words longer than a line stay whole, as on the page.
pub(crate) fn wrap_text(text: &str, max_width: f64, char_width: f64) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        let mut tokens: Vec<String> = Vec::new();
        for c in para.chars() {
            match tokens.last_mut() {
                Some(t) if t.chars().all(char::is_whitespace) == c.is_whitespace() => t.push(c),
                _ => tokens.push(c.to_string()),
            }
        }
        for word in tokens {
            let next_len = line.chars().count() + word.chars().count();
            let is_word = !word.trim().is_empty();
            if !line.is_empty() && is_word && next_len as f64 * char_width > max_width {
                lines.push(line.trim_end().to_string());
                line = word.trim_start().to_string();
            } else {
                line.push_str(&word);
            }
        }
        lines.push(line);
    }
    lines
}

/// A compact number for SVG and PDF: at most two decimals, no trailing zeros, never "-0" and
/// never an exponent (PDF has none).
pub(crate) fn num(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    let r = if r == 0.0 || !r.is_finite() { 0.0 } else { r };
    let s = format!("{r:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// One piece of a smoothed stroke path.
pub(crate) enum Seg {
    Move(f64, f64),
    Line(f64, f64),
    /// Quadratic curve: control point, end point.
    Quad(f64, f64, f64, f64),
}

/// Quadratic curves through the midpoints, exactly like `tracePolyline` in render.js. A single
/// point becomes a tiny line so round caps draw a dot.
pub(crate) fn stroke_path(p: &[(f64, f64)]) -> Vec<Seg> {
    let Some(&(x0, y0)) = p.first() else {
        return Vec::new();
    };
    let mut out = vec![Seg::Move(x0, y0)];
    if p.len() == 1 {
        out.push(Seg::Line(x0 + 0.01, y0));
        return out;
    }
    for i in 1..p.len().saturating_sub(1) {
        let (cx, cy) = p[i];
        let (nx, ny) = p[i + 1];
        out.push(Seg::Quad(cx, cy, (cx + nx) / 2.0, (cy + ny) / 2.0));
    }
    let (lx, ly) = p[p.len() - 1];
    out.push(Seg::Line(lx, ly));
    out
}

/// What an export draws: the frame (page area in world pixels) and its objects back to front.
pub(crate) struct Scene {
    pub frame: Rect,
    pub items: Vec<Item>,
    pub title: String,
}

pub(crate) fn scene(board: &Board, scope: &Scope, opts: &Options) -> Scene {
    let all = board
        .visible_objects()
        .into_iter()
        .filter(|o| match scope {
            Scope::Selection(ids) => ids.contains(&o.id),
            _ => true,
        })
        .filter_map(|o| Item::from_object(o, board, opts.with_names));
    let (frame, items) = match scope {
        Scope::View(view) if view.is_valid() => {
            (*view, all.filter(|i| i.bounds().intersects(view)).collect())
        }
        _ => {
            let items: Vec<Item> = all.collect();
            let content = items.iter().map(Item::bounds).reduce(Rect::union);
            (content.map_or(EMPTY, |r| r.inflate(MARGIN)), items)
        }
    };
    let title = board
        .settings()
        .title
        .as_ref()
        .map_or_else(String::new, |r| r.val.clone());
    Scene {
        frame,
        items,
        title,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::{AuthorInfo, BoardField, Edit, Fields, Geom};

    fn create(kind: Kind, base: Vec<(i32, i32)>, g: (i32, i32, i32, i32), f: Fields) -> Edit {
        Edit::Create {
            kind,
            base,
            fields: Fields {
                geom: Some(Geom {
                    x: g.0 * 4,
                    y: g.1 * 4,
                    w: g.2 * 4,
                    h: g.3 * 4,
                }),
                ..f
            },
        }
    }

    /// A small scene with one of each kind; returns the board and the ids in creation order.
    pub(crate) fn sample() -> (Board, Vec<ObjId>) {
        let mut b = Board::new(7);
        b.local_author(AuthorInfo {
            name: "Maya <admin>".into(),
            cert_key: None,
            shape: 1,
            color: 2,
        })
        .unwrap();
        b.local_board(vec![BoardField::Title("Launch & <plan>".into())])
            .unwrap();
        let z = |s: &str| Fields {
            z: Some(s.into()),
            ..Fields::default()
        };
        let edits = vec![
            create(
                Kind::Stroke,
                vec![(0, 0), (40, 40), (80, 0)],
                (10, 10, 20, 10),
                Fields {
                    ink: Some(1),
                    ..z("b")
                },
            ),
            create(
                Kind::Highlight,
                vec![(0, 0), (200, 0)],
                (0, 60, 50, 0),
                z("c"),
            ),
            create(
                Kind::Rect,
                vec![],
                (100, 10, 60, 40),
                Fields {
                    ink: Some(4),
                    ..z("d")
                },
            ),
            create(Kind::Ellipse, vec![], (180, 10, 40, 40), z("e")),
            create(Kind::Diamond, vec![], (240, 10, 40, 40), z("f")),
            create(Kind::Arrow, vec![], (10, 100, 80, 0), z("g")),
            create(Kind::Line, vec![], (10, 130, 80, 20), z("h")),
            create(
                Kind::Note,
                vec![],
                (300, 100, 156, 112),
                Fields {
                    fill: Some(2),
                    text: Some("Ship <script>alert(1)</script> & party".into()),
                    alt: Some("Note about \"shipping\"".into()),
                    ..z("i")
                },
            ),
            create(
                Kind::Text,
                vec![],
                (10, 200, 0, 0),
                Fields {
                    text: Some("Launch!\nWeek 2 €".into()),
                    width: Some(2),
                    ..z("j")
                },
            ),
        ];
        let mut ids = Vec::new();
        for e in edits {
            let ops = b.local(vec![e]).unwrap();
            ids.push(ObjId {
                author: ops[0].author,
                counter: ops[0].seq,
            });
        }
        (b, ids)
    }

    #[test]
    fn file_names_are_sanitised() {
        assert_eq!(
            file_name("Launch week plan", Format::Svg),
            "Launch week plan.svg"
        );
        assert_eq!(file_name("a/b\\c:d*?\"<>|e", Format::Pdf), "a b c d e.pdf");
        assert_eq!(file_name("  ..hidden.. ", Format::Board), "hidden.mcwb");
        assert_eq!(file_name("", Format::Png), "Whiteboard.png");
        assert_eq!(file_name("///", Format::Png), "Whiteboard.png");
        assert_eq!(file_name("con", Format::Pdf), "_con.pdf");
        assert_eq!(file_name("COM1", Format::Pdf), "_COM1.pdf");
        assert_eq!(
            file_name("line\nbreak\ttab", Format::Svg),
            "line break tab.svg"
        );
        assert_eq!(file_name(&"x".repeat(200), Format::Svg).len(), 64 + 4);
    }

    #[test]
    fn scopes_filter_objects() {
        let (b, ids) = sample();
        let opts = Options::default();
        let all = scene(&b, &Scope::Board, &opts);
        assert_eq!(all.items.len(), ids.len());
        assert!(all.frame.x < 0.0 && all.frame.w > 450.0);

        let sel: BTreeSet<ObjId> = [ids[2], ids[7]].into();
        let s = scene(&b, &Scope::Selection(sel), &opts);
        assert_eq!(
            s.items.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![ids[2], ids[7]]
        );
        assert!(s.frame.x > 50.0, "frame fits the selection");

        let view = Rect {
            x: 95.0,
            y: 0.0,
            w: 100.0,
            h: 60.0,
        };
        let v = scene(&b, &Scope::View(view), &opts);
        assert_eq!(v.frame, view);
        let got: Vec<ObjId> = v.items.iter().map(|i| i.id).collect();
        assert_eq!(got, vec![ids[2], ids[3]]);

        let bad = Rect {
            w: f64::NAN,
            ..view
        };
        assert_eq!(scene(&b, &Scope::View(bad), &opts).items.len(), ids.len());
        let empty = scene(&b, &Scope::Selection(BTreeSet::new()), &opts);
        assert!(empty.items.is_empty());
        assert_eq!(empty.frame, EMPTY);
    }

    #[test]
    fn deleted_objects_are_not_exported() {
        let (mut b, ids) = sample();
        b.local(vec![Edit::Set {
            id: ids[0],
            fields: Fields {
                deleted: Some(true),
                ..Fields::default()
            },
        }])
        .unwrap();
        let s = scene(&b, &Scope::Board, &Options::default());
        assert!(s.items.iter().all(|i| i.id != ids[0]));
    }

    #[test]
    fn names_only_when_asked() {
        let (b, _) = sample();
        let off = scene(&b, &Scope::Board, &Options::default());
        assert!(off.items.iter().all(|i| i.author.is_none()));
        let on = scene(
            &b,
            &Scope::Board,
            &Options {
                with_names: true,
                ..Options::default()
            },
        );
        assert!(
            on.items
                .iter()
                .all(|i| i.author.as_deref() == Some("Maya <admin>"))
        );
    }

    #[test]
    fn strokes_scale_with_their_geometry() {
        let (b, ids) = sample();
        let s = scene(&b, &Scope::Board, &Options::default());
        let stroke = s.items.iter().find(|i| i.id == ids[0]).unwrap();
        // Base 80×40 quarter px scaled into 20×10 px at (10, 10).
        assert_eq!(
            stroke.points,
            vec![(10.0, 10.0), (20.0, 20.0), (30.0, 10.0)]
        );
    }

    #[test]
    fn wrapping_matches_the_page_rules() {
        assert_eq!(wrap_text("aa bb cc", 5.0, 1.0), vec!["aa bb", "cc"]);
        assert_eq!(wrap_text("one\n\ntwo", 100.0, 1.0), vec!["one", "", "two"]);
        assert_eq!(
            wrap_text("verylongword x", 3.0, 1.0),
            vec!["verylongword", "x"]
        );
        assert_eq!(wrap_text("", 3.0, 1.0), vec![""]);
    }

    #[test]
    fn colours_parse() {
        assert_eq!(rgb("#ff0000"), (1.0, 0.0, 0.0));
        assert_eq!(rgb("#zz"), (0.0, 0.0, 0.0));
    }
}
