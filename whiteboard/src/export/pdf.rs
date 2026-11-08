//! PDF export: a minimal PDF 1.4 writer, no crates besides deflate for the content stream.
//!
//! One page sized to the content (1 px = 0.75 pt, as CSS pixels at 96 dpi; very large boards
//! are scaled down to the 200 inch page limit). Objects are vector paths; text uses the
//! standard Helvetica font with WinAnsi encoding, so characters outside it become '?'. Objects
//! with alt text are tagged as figures with `/Alt`, which screen readers and PDF/UA checkers read.
//! The world is drawn with a y-down transform so coordinates match the page renderer.

use std::fmt::Write as _;

use miniz_oxide::deflate::compress_to_vec_zlib;

use super::{
    Item, NAME_FONT, NOTE_LINE, NOTE_PAD, NOTE_RADIUS, Options, Palette, Scope, Seg, num as n,
    palette, rgb, scene, stroke_path,
};
use crate::model::{Board, Kind};

const PX_TO_PT: f64 = 0.75;
/// Largest page side most viewers accept (200 inches).
const MAX_PAGE_PT: f64 = 14_400.0;
/// Smallest page side PDF allows.
const MIN_PAGE_PT: f64 = 3.0;
/// Helvetica is wider than the hand-written page font, so text is set smaller to keep the line
/// breaks close to the page.
const NOTE_FONT_PT: f64 = 19.0;
const TEXT_SCALE: f64 = 0.75;
const HELVETICA_CHAR_WIDTH: f64 = 0.5;
/// Circle approximation with cubic Béziers.
const KAPPA: f64 = 0.552_284_75;

// Object numbers; the structure tree for alt text follows when there is any.
const CATALOG: usize = 1;
const PAGES: usize = 2;
const PAGE: usize = 3;
const CONTENT: usize = 4;
const FONT: usize = 5;
const GS_HIGHLIGHT: usize = 6;
const GS_SHADOW: usize = 7;
const INFO: usize = 8;
const STRUCT_ROOT: usize = 9;
const PARENT_TREE: usize = 10;
const FIRST_ELEM: usize = 11;

/// The PDF document for `scope` of `board`.
pub fn export_pdf(board: &Board, scope: &Scope, opts: &Options) -> Vec<u8> {
    let scene = scene(board, scope, opts);
    let pal = palette(opts.paper);
    let f = scene.frame;
    let s = PX_TO_PT
        .max(MIN_PAGE_PT / f.w.min(f.h))
        .min(MAX_PAGE_PT / f.w.max(f.h));
    let (page_w, page_h) = (f.w * s, f.h * s);

    let mut c = String::new();
    let _ = writeln!(
        c,
        "{} 0 0 {} {} {} cm",
        n4(s),
        n4(-s),
        n4(-f.x * s),
        n4((f.y + f.h) * s)
    );
    c.push_str("1 J 1 j\n");
    if !opts.transparent {
        fill_colour(&mut c, pal.paper);
        let _ = writeln!(c, "{} {} {} {} re f", n(f.x), n(f.y), n(f.w), n(f.h));
    }
    let mut alts = Vec::new();
    for item in &scene.items {
        let tagged = !item.alt.is_empty();
        if tagged {
            let _ = writeln!(c, "/Figure << /MCID {} >> BDC", alts.len());
            alts.push(item.alt.as_str());
        }
        c.push_str("q\n");
        item_pdf(&mut c, item, &pal);
        c.push_str("Q\n");
        if tagged {
            c.push_str("EMC\n");
        }
    }

    let mut w = Writer::default();
    let tagged = !alts.is_empty();
    let elems: Vec<usize> = (0..alts.len()).map(|i| FIRST_ELEM + i).collect();
    let refs = |ids: &[usize]| {
        ids.iter()
            .map(|i| format!("{i} 0 R"))
            .collect::<Vec<_>>()
            .join(" ")
    };

    let mut catalog = format!("<< /Type /Catalog /Pages {PAGES} 0 R");
    if tagged {
        let _ = write!(
            catalog,
            " /MarkInfo << /Marked true >> /StructTreeRoot {STRUCT_ROOT} 0 R"
        );
    }
    if !scene.title.is_empty() {
        catalog.push_str(" /ViewerPreferences << /DisplayDocTitle true >>");
    }
    catalog.push_str(" >>");
    w.obj(CATALOG, catalog.as_bytes());
    w.obj(
        PAGES,
        format!("<< /Type /Pages /Kids [{PAGE} 0 R] /Count 1 >>").as_bytes(),
    );
    let mut page = format!(
        "<< /Type /Page /Parent {PAGES} 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 {FONT} 0 R >> /ExtGState << /GH {GS_HIGHLIGHT} 0 R /GS {GS_SHADOW} 0 R >> >> /Contents {CONTENT} 0 R",
        n(page_w),
        n(page_h)
    );
    if tagged {
        page.push_str(" /StructParents 0");
    }
    page.push_str(" >>");
    w.obj(PAGE, page.as_bytes());
    let data = compress_to_vec_zlib(c.as_bytes(), 6);
    let mut stream = format!(
        "<< /Length {} /Filter /FlateDecode >>\nstream\n",
        data.len()
    )
    .into_bytes();
    stream.extend_from_slice(&data);
    stream.extend_from_slice(b"\nendstream");
    w.obj(CONTENT, &stream);
    w.obj(
        FONT,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    );
    let blend = if pal.multiply { "Multiply" } else { "Normal" };
    w.obj(
        GS_HIGHLIGHT,
        format!(
            "<< /Type /ExtGState /CA {a} /ca {a} /BM /{blend} >>",
            a = n(pal.hl_alpha)
        )
        .as_bytes(),
    );
    w.obj(GS_SHADOW, b"<< /Type /ExtGState /ca 0.18 >>");
    let mut info = String::from("<< /Producer (Mumcord Whiteboard)");
    if !scene.title.is_empty() {
        let _ = write!(info, " /Title {}", utf16_text(&scene.title));
    }
    info.push_str(" >>");
    w.obj(INFO, info.as_bytes());
    if tagged {
        w.obj(
            STRUCT_ROOT,
            format!(
                "<< /Type /StructTreeRoot /K [{}] /ParentTree {PARENT_TREE} 0 R /ParentTreeNextKey 1 >>",
                refs(&elems)
            )
            .as_bytes(),
        );
        w.obj(
            PARENT_TREE,
            format!("<< /Nums [0 [{}]] >>", refs(&elems)).as_bytes(),
        );
        for (mcid, (id, alt)) in elems.iter().zip(&alts).enumerate() {
            w.obj(
                *id,
                format!(
                    "<< /Type /StructElem /S /Figure /P {STRUCT_ROOT} 0 R /Pg {PAGE} 0 R /K {mcid} /Alt {} >>",
                    utf16_text(alt)
                )
                .as_bytes(),
            );
        }
    }
    w.finish()
}

fn item_pdf(c: &mut String, item: &Item, pal: &Palette) {
    let ink = pal.ink(item);
    stroke_colour(c, ink);
    let _ = writeln!(c, "{} w", n(item.stroke_width()));
    let r = item.rect();
    match item.kind {
        Kind::Stroke | Kind::Highlight => {
            if item.kind == Kind::Highlight {
                c.push_str("/GH gs\n");
            }
            path(c, &stroke_path(&item.points));
            c.push_str("S\n");
        }
        Kind::Rect | Kind::Ellipse | Kind::Diamond => {
            fill_colour(c, pal.box_fill);
            match item.kind {
                Kind::Rect => {
                    rounded_rect(c, r.x, r.y, r.w, r.h, 12f64.min(r.w / 4.0).min(r.h / 4.0))
                }
                Kind::Ellipse => ellipse(c, r.x + r.w / 2.0, r.y + r.h / 2.0, r.w / 2.0, r.h / 2.0),
                _ => {
                    let _ = writeln!(
                        c,
                        "{} {} m {} {} l {} {} l {} {} l h",
                        n(r.x + r.w / 2.0),
                        n(r.y),
                        n(r.x + r.w),
                        n(r.y + r.h / 2.0),
                        n(r.x + r.w / 2.0),
                        n(r.y + r.h),
                        n(r.x),
                        n(r.y + r.h / 2.0)
                    );
                }
            }
            c.push_str("B\n");
        }
        Kind::Line | Kind::Arrow => {
            let (x1, y1) = (item.g.x + item.g.w, item.g.y + item.g.h);
            let _ = writeln!(
                c,
                "{} {} m {} {} l S",
                n(item.g.x),
                n(item.g.y),
                n(x1),
                n(y1)
            );
            if let Some([a, b]) = item.arrow_head() {
                let _ = writeln!(
                    c,
                    "{} {} m {} {} l {} {} l S",
                    n(a.0),
                    n(a.1),
                    n(x1),
                    n(y1),
                    n(b.0),
                    n(b.1)
                );
            }
        }
        Kind::Note => note_pdf(c, item, pal),
        Kind::Text => {
            let size = item.text_size() * TEXT_SCALE;
            fill_colour(c, ink);
            for (i, line) in item.text.split('\n').enumerate() {
                let y = item.g.y + i as f64 * item.text_size() * 1.1 + size * super::ASCENT;
                text(c, item.g.x, y, size, line);
            }
        }
        Kind::Image => {}
    }
}

fn note_pdf(c: &mut String, item: &Item, pal: &Palette) {
    let r = item.rect();
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let (sin, cos) = item.note_tilt_deg().to_radians().sin_cos();
    // Rotation about the note centre: translate(c) · rotate · translate(-c).
    let _ = writeln!(
        c,
        "{} {} {} {} {} {} cm",
        n4(cos),
        n4(sin),
        n4(-sin),
        n4(cos),
        n4(cx - cos * cx + sin * cy),
        n4(cy - sin * cx - cos * cy)
    );
    c.push_str("q /GS gs 0 0 0 rg\n");
    rounded_rect(c, r.x, r.y + 2.0, r.w, r.h, NOTE_RADIUS);
    c.push_str("f Q\n");
    fill_colour(c, pal.note(item));
    rounded_rect(c, r.x, r.y, r.w, r.h, NOTE_RADIUS);
    c.push_str("f\n");
    fill_colour(c, pal.note_text);
    for (i, line) in item
        .note_lines(NOTE_FONT_PT * HELVETICA_CHAR_WIDTH)
        .iter()
        .enumerate()
    {
        text(
            c,
            r.x + NOTE_PAD,
            r.y + 36.0 + i as f64 * NOTE_LINE,
            NOTE_FONT_PT,
            line,
        );
    }
    if let Some(name) = &item.author {
        text(c, r.x + NOTE_PAD, r.y + r.h - 8.0, NAME_FONT, name);
    }
}

/// One line of text with its baseline at world (x, y); the text matrix flips y back up.
fn text(c: &mut String, x: f64, y: f64, size: f64, s: &str) {
    let _ = writeln!(
        c,
        "BT /F1 {} Tf 1 0 0 -1 {} {} Tm {} Tj ET",
        n(size),
        n(x),
        n(y),
        win_ansi_string(s)
    );
}

fn path(c: &mut String, segs: &[Seg]) {
    let (mut px, mut py) = (0.0, 0.0);
    for s in segs {
        match *s {
            Seg::Move(x, y) => {
                let _ = writeln!(c, "{} {} m", n(x), n(y));
                (px, py) = (x, y);
            }
            Seg::Line(x, y) => {
                let _ = writeln!(c, "{} {} l", n(x), n(y));
                (px, py) = (x, y);
            }
            Seg::Quad(qx, qy, x, y) => {
                // A quadratic Bézier as the equivalent cubic.
                let _ = writeln!(
                    c,
                    "{} {} {} {} {} {} c",
                    n(px + 2.0 / 3.0 * (qx - px)),
                    n(py + 2.0 / 3.0 * (qy - py)),
                    n(x + 2.0 / 3.0 * (qx - x)),
                    n(y + 2.0 / 3.0 * (qy - y)),
                    n(x),
                    n(y)
                );
                (px, py) = (x, y);
            }
        }
    }
}

fn rounded_rect(c: &mut String, x: f64, y: f64, w: f64, h: f64, r: f64) {
    if r <= 0.0 {
        let _ = writeln!(c, "{} {} {} {} re", n(x), n(y), n(w), n(h));
        return;
    }
    let k = KAPPA * r;
    let (x1, y1) = (x + w, y + h);
    let _ = writeln!(c, "{} {} m {} {} l", n(x + r), n(y), n(x1 - r), n(y));
    let _ = writeln!(
        c,
        "{} {} {} {} {} {} c {} {} l",
        n(x1 - r + k),
        n(y),
        n(x1),
        n(y + r - k),
        n(x1),
        n(y + r),
        n(x1),
        n(y1 - r)
    );
    let _ = writeln!(
        c,
        "{} {} {} {} {} {} c {} {} l",
        n(x1),
        n(y1 - r + k),
        n(x1 - r + k),
        n(y1),
        n(x1 - r),
        n(y1),
        n(x + r),
        n(y1)
    );
    let _ = writeln!(
        c,
        "{} {} {} {} {} {} c {} {} l",
        n(x + r - k),
        n(y1),
        n(x),
        n(y1 - r + k),
        n(x),
        n(y1 - r),
        n(x),
        n(y + r)
    );
    let _ = writeln!(
        c,
        "{} {} {} {} {} {} c h",
        n(x),
        n(y + r - k),
        n(x + r - k),
        n(y),
        n(x + r),
        n(y)
    );
}

fn ellipse(c: &mut String, cx: f64, cy: f64, rx: f64, ry: f64) {
    let (kx, ky) = (KAPPA * rx, KAPPA * ry);
    let _ = writeln!(c, "{} {} m", n(cx + rx), n(cy));
    for [a, b, e] in [
        [(cx + rx, cy + ky), (cx + kx, cy + ry), (cx, cy + ry)],
        [(cx - kx, cy + ry), (cx - rx, cy + ky), (cx - rx, cy)],
        [(cx - rx, cy - ky), (cx - kx, cy - ry), (cx, cy - ry)],
        [(cx + kx, cy - ry), (cx + rx, cy - ky), (cx + rx, cy)],
    ] {
        let _ = writeln!(
            c,
            "{} {} {} {} {} {} c",
            n(a.0),
            n(a.1),
            n(b.0),
            n(b.1),
            n(e.0),
            n(e.1)
        );
    }
    c.push_str("h\n");
}

fn fill_colour(c: &mut String, hex: &str) {
    let (r, g, b) = rgb(hex);
    let _ = writeln!(c, "{} {} {} rg", n4(r), n4(g), n4(b));
}

fn stroke_colour(c: &mut String, hex: &str) {
    let (r, g, b) = rgb(hex);
    let _ = writeln!(c, "{} {} {} RG", n4(r), n4(g), n4(b));
}

/// Four decimals for colours and transforms, where two would visibly shift things.
fn n4(v: f64) -> String {
    let r = (v * 10_000.0).round() / 10_000.0;
    let r = if r == 0.0 || !r.is_finite() { 0.0 } else { r };
    let s = format!("{r:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The WinAnsi (Windows-1252) byte for `c`, or '?' when the font cannot show it.
fn win_ansi(c: char) -> u8 {
    let code = u32::from(c);
    if (0x20..=0x7e).contains(&code) || (0xa0..=0xff).contains(&code) {
        return code as u8;
    }
    match c {
        '€' => 0x80,
        '‚' => 0x82,
        'ƒ' => 0x83,
        '„' => 0x84,
        '…' => 0x85,
        '†' => 0x86,
        '‡' => 0x87,
        'ˆ' => 0x88,
        '‰' => 0x89,
        'Š' => 0x8a,
        '‹' => 0x8b,
        'Œ' => 0x8c,
        'Ž' => 0x8e,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        '˜' => 0x98,
        '™' => 0x99,
        'š' => 0x9a,
        '›' => 0x9b,
        'œ' => 0x9c,
        'ž' => 0x9e,
        'Ÿ' => 0x9f,
        _ => b'?',
    }
}

/// A PDF literal string in WinAnsi; the content stays ASCII through octal escapes.
fn win_ansi_string(s: &str) -> String {
    let mut out = String::from("(");
    for b in s.chars().map(win_ansi) {
        match b {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(char::from(b));
            }
            0x20..=0x7e => out.push(char::from(b)),
            _ => {
                let _ = write!(out, "\\{b:03o}");
            }
        }
    }
    out.push(')');
    out
}

/// A PDF text string for metadata and alt text: UTF-16BE with a byte order mark, as hex.
fn utf16_text(s: &str) -> String {
    let mut out = String::from("<FEFF");
    for u in s.encode_utf16() {
        let _ = write!(out, "{u:04X}");
    }
    out.push('>');
    out
}

/// Collects numbered objects in order and writes the cross-reference table and trailer.
#[derive(Default)]
struct Writer {
    out: Vec<u8>,
    offsets: Vec<usize>,
}

impl Writer {
    /// Objects must be added in order 1, 2, 3, …
    fn obj(&mut self, num: usize, body: &[u8]) {
        if self.out.is_empty() {
            // The binary comment tells transfer tools the file is not text.
            self.out.extend_from_slice(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n");
        }
        debug_assert_eq!(num, self.offsets.len() + 1);
        self.offsets.push(self.out.len());
        self.out
            .extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        self.out.extend_from_slice(body);
        self.out.extend_from_slice(b"\nendobj\n");
    }

    fn finish(mut self) -> Vec<u8> {
        let xref = self.out.len();
        let mut t = format!("xref\n0 {}\n0000000000 65535 f \n", self.offsets.len() + 1);
        for off in &self.offsets {
            let _ = writeln!(t, "{off:010} 00000 n ");
        }
        let _ = write!(
            t,
            "trailer\n<< /Size {} /Root {CATALOG} 0 R /Info {INFO} 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            self.offsets.len() + 1
        );
        self.out.extend_from_slice(t.as_bytes());
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{Paper, Rect, tests::sample};
    use miniz_oxide::inflate::decompress_to_vec_zlib;

    fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
        hay.windows(needle.len()).position(|w| w == needle)
    }

    fn text_of(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    /// Checks header, xref offsets, trailer and startxref; returns the object count.
    fn check_structure(pdf: &[u8]) -> usize {
        assert!(pdf.starts_with(b"%PDF-1.4\n"));
        assert!(pdf.ends_with(b"%%EOF\n"));
        let tail = text_of(&pdf[pdf.len() - 40..]);
        let start: usize = tail
            .split("startxref\n")
            .nth(1)
            .and_then(|s| s.lines().next())
            .and_then(|s| s.parse().ok())
            .expect("startxref");
        assert!(pdf[start..].starts_with(b"xref\n0 "));
        let xref = text_of(&pdf[start..]);
        let mut lines = xref.lines().skip(1);
        let count: usize = lines.next().unwrap()[2..].parse().unwrap();
        assert_eq!(lines.next().unwrap(), "0000000000 65535 f ");
        for num in 1..count {
            let entry = lines.next().unwrap();
            assert_eq!(entry.len(), 19, "20-byte entries with the newline");
            assert!(entry.ends_with(" 00000 n "));
            let off: usize = entry[..10].parse().unwrap();
            assert!(
                pdf[off..].starts_with(format!("{num} 0 obj\n").as_bytes()),
                "object {num} at {off}"
            );
        }
        assert_eq!(lines.next().unwrap(), "trailer");
        let trailer = lines.next().unwrap();
        assert!(trailer.contains(&format!("/Size {count} ")));
        assert!(trailer.contains("/Root 1 0 R"));
        count - 1
    }

    fn content(pdf: &[u8]) -> String {
        let at = find(pdf, b"4 0 obj\n").unwrap();
        let head = text_of(&pdf[at..at + 80]);
        let len: usize = head
            .split("/Length ")
            .nth(1)
            .and_then(|s| s.split(' ').next())
            .and_then(|s| s.parse().ok())
            .unwrap();
        let start = at + find(&pdf[at..], b"stream\n").unwrap() + 7;
        assert!(
            pdf[start + len..].starts_with(b"\nendstream"),
            "stream length"
        );
        text_of(&decompress_to_vec_zlib(&pdf[start..start + len]).unwrap())
    }

    #[test]
    fn structure_and_xref_offsets_are_valid() {
        let (b, _) = sample();
        let pdf = export_pdf(&b, &Scope::Board, &Options::default());
        // Catalog … info, plus the structure root, parent tree and one tagged note.
        assert_eq!(check_structure(&pdf), 11);
        let all = text_of(&pdf);
        assert!(all.contains("/BaseFont /Helvetica /Encoding /WinAnsiEncoding"));
        assert!(all.contains("/StructTreeRoot 9 0 R"));
        assert!(all.contains("/S /Figure"));
        assert!(all.contains(&format!("/Alt {}", utf16_text("Note about \"shipping\""))));
        assert!(all.contains(&format!("/Title {}", utf16_text("Launch & <plan>"))));
        assert!(all.contains("/BM /Multiply"));
    }

    #[test]
    fn content_draws_the_scene() {
        let (b, _) = sample();
        let pdf = export_pdf(&b, &Scope::Board, &Options::default());
        let c = content(&pdf);
        assert!(c.starts_with("0.75 0 0 -0.75 "));
        assert!(c.contains(" re f\n"), "paper");
        assert!(c.contains("/GH gs"), "highlighter");
        assert!(c.contains(" c\n"), "curves");
        assert!(c.contains("B\n"), "filled shapes");
        assert!(c.contains("/Figure << /MCID 0 >> BDC"));
        assert!(c.contains("(Week 2 \\200) Tj"), "euro in WinAnsi");
        assert!(c.contains("alert\\(1\\)"), "parentheses escaped");
        assert!(!c.contains("Maya"), "no names by default");
    }

    #[test]
    fn names_transparency_and_untagged() {
        let (b, ids) = sample();
        let opts = Options {
            paper: Paper::Dark,
            transparent: true,
            with_names: true,
        };
        let pdf = export_pdf(&b, &Scope::Board, &opts);
        let c = content(&pdf);
        assert!(c.contains("(Maya <admin>) Tj"));
        assert!(!c.contains(" re f\n"), "no paper");
        assert!(text_of(&pdf).contains("/BM /Normal"));

        // Without alt text there is no structure tree.
        let sel = Scope::Selection([ids[0]].into());
        let pdf = export_pdf(&b, &sel, &Options::default());
        assert_eq!(check_structure(&pdf), 8);
        assert!(!text_of(&pdf).contains("StructTreeRoot"));
    }

    #[test]
    fn page_size_follows_the_content() {
        let (b, _) = sample();
        let view = Rect {
            x: 0.0,
            y: 0.0,
            w: 400.0,
            h: 200.0,
        };
        let pdf = export_pdf(&b, &Scope::View(view), &Options::default());
        assert!(text_of(&pdf).contains("/MediaBox [0 0 300 150]"));
        let huge = Rect {
            x: 0.0,
            y: 0.0,
            w: 100_000.0,
            h: 50_000.0,
        };
        let pdf = export_pdf(&b, &Scope::View(huge), &Options::default());
        assert!(text_of(&pdf).contains("/MediaBox [0 0 14400 7200]"));
        let pdf = export_pdf(&Board::new(1), &Scope::Board, &Options::default());
        check_structure(&pdf);
    }

    #[test]
    fn text_encoding() {
        assert_eq!(win_ansi_string("a(b)\\"), "(a\\(b\\)\\\\)");
        assert_eq!(win_ansi_string("é€日\n"), "(\\351\\200??)");
        assert_eq!(utf16_text("A日"), "<FEFF004165E5>");
        assert_eq!(utf16_text("😀"), "<FEFFD83DDE00>");
        assert_eq!(n4(-0.00001), "0");
        assert_eq!(n4(0.123456), "0.1235");
    }
}
