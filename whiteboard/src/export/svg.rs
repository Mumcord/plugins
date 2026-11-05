//! SVG export: a standalone document with no external references (no links, images, fonts or
//! scripts). Every text from the board is escaped. Alt text becomes the `<title>` of the
//! object's group, the author (only with names on) its `<desc>`.

use std::fmt::Write as _;

use super::{
    ASCENT, HAND_CHAR_WIDTH, HAND_FAMILY, Item, NAME_FONT, NOTE_FONT, NOTE_LINE, NOTE_PAD,
    NOTE_RADIUS, Options, Palette, Scope, Seg, num as n, palette, scene, stroke_path,
};
use crate::model::{Board, Kind};

/// The SVG document for `scope` of `board`.
pub fn export_svg(board: &Board, scope: &Scope, opts: &Options) -> String {
    let scene = scene(board, scope, opts);
    let pal = palette(opts.paper);
    let f = scene.frame;
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="{} {} {} {}" role="img">"#,
        n(f.w),
        n(f.h),
        n(f.x),
        n(f.y),
        n(f.w),
        n(f.h)
    );
    if !scene.title.is_empty() {
        let _ = write!(out, "<title>{}</title>", esc(&scene.title));
    }
    if !opts.transparent {
        let _ = write!(
            out,
            r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}"/>"#,
            n(f.x),
            n(f.y),
            n(f.w),
            n(f.h),
            pal.paper
        );
    }
    out.push_str(r#"<g fill="none" stroke-linecap="round" stroke-linejoin="round">"#);
    for item in &scene.items {
        item_svg(&mut out, item, &pal);
    }
    out.push_str("</g></svg>");
    out
}

fn item_svg(out: &mut String, item: &Item, pal: &Palette) {
    out.push_str("<g>");
    if !item.alt.is_empty() {
        let _ = write!(out, "<title>{}</title>", esc(&item.alt));
    }
    if let Some(name) = &item.author {
        let _ = write!(out, "<desc>By {}</desc>", esc(name));
    }
    let ink = pal.ink(item);
    let lw = n(item.stroke_width());
    let r = item.rect();
    match item.kind {
        Kind::Stroke | Kind::Highlight => {
            let _ = write!(
                out,
                r#"<path d="{}" stroke="{ink}" stroke-width="{lw}""#,
                path_data(&stroke_path(&item.points))
            );
            if item.kind == Kind::Highlight {
                let _ = write!(out, r#" stroke-opacity="{}""#, n(pal.hl_alpha));
                if pal.multiply {
                    out.push_str(r#" style="mix-blend-mode:multiply""#);
                }
            }
            out.push_str("/>");
        }
        Kind::Rect => {
            let radius = 12f64.min(r.w / 4.0).min(r.h / 4.0);
            let _ = write!(
                out,
                r#"<rect x="{}" y="{}" width="{}" height="{}" rx="{}" fill="{}" stroke="{ink}" stroke-width="{lw}"/>"#,
                n(r.x),
                n(r.y),
                n(r.w),
                n(r.h),
                n(radius),
                pal.box_fill
            );
        }
        Kind::Ellipse => {
            let _ = write!(
                out,
                r#"<ellipse cx="{}" cy="{}" rx="{}" ry="{}" fill="{}" stroke="{ink}" stroke-width="{lw}"/>"#,
                n(r.x + r.w / 2.0),
                n(r.y + r.h / 2.0),
                n(r.w / 2.0),
                n(r.h / 2.0),
                pal.box_fill
            );
        }
        Kind::Diamond => {
            let _ = write!(
                out,
                r#"<path d="M{} {}L{} {}L{} {}L{} {}Z" fill="{}" stroke="{ink}" stroke-width="{lw}"/>"#,
                n(r.x + r.w / 2.0),
                n(r.y),
                n(r.x + r.w),
                n(r.y + r.h / 2.0),
                n(r.x + r.w / 2.0),
                n(r.y + r.h),
                n(r.x),
                n(r.y + r.h / 2.0),
                pal.box_fill
            );
        }
        Kind::Line | Kind::Arrow => {
            let (x1, y1) = (item.g.x + item.g.w, item.g.y + item.g.h);
            let mut d = format!("M{} {}L{} {}", n(item.g.x), n(item.g.y), n(x1), n(y1));
            if let Some([a, b]) = item.arrow_head() {
                let _ = write!(
                    d,
                    "M{} {}L{} {}L{} {}",
                    n(a.0),
                    n(a.1),
                    n(x1),
                    n(y1),
                    n(b.0),
                    n(b.1)
                );
            }
            let _ = write!(out, r#"<path d="{d}" stroke="{ink}" stroke-width="{lw}"/>"#);
        }
        Kind::Note => note_svg(out, item, pal),
        Kind::Text => {
            let size = item.text_size();
            let _ = write!(
                out,
                r#"<text fill="{ink}" font-family="{HAND_FAMILY}" font-weight="700" font-size="{}" xml:space="preserve">"#,
                n(size)
            );
            for (i, line) in item.text.split('\n').enumerate() {
                let y = item.g.y + i as f64 * size * 1.1 + size * ASCENT;
                let _ = write!(
                    out,
                    r#"<tspan x="{}" y="{}">{}</tspan>"#,
                    n(item.g.x),
                    n(y),
                    esc(line)
                );
            }
            out.push_str("</text>");
        }
        Kind::Image => {}
    }
    out.push_str("</g>");
}

fn note_svg(out: &mut String, item: &Item, pal: &Palette) {
    let r = item.rect();
    let _ = write!(
        out,
        r#"<g transform="rotate({} {} {})" stroke="none">"#,
        n(item.note_tilt_deg()),
        n(r.x + r.w / 2.0),
        n(r.y + r.h / 2.0)
    );
    // A flat shadow instead of a blur filter keeps the file simple and renders everywhere.
    let _ = write!(
        out,
        r##"<rect x="{}" y="{}" width="{}" height="{}" rx="{NOTE_RADIUS}" fill="#000" fill-opacity="0.18"/>"##,
        n(r.x),
        n(r.y + 2.0),
        n(r.w),
        n(r.h)
    );
    let _ = write!(
        out,
        r#"<rect x="{}" y="{}" width="{}" height="{}" rx="{NOTE_RADIUS}" fill="{}"/>"#,
        n(r.x),
        n(r.y),
        n(r.w),
        n(r.h),
        pal.note(item)
    );
    let _ = write!(
        out,
        r#"<text fill="{}" font-family="{HAND_FAMILY}" font-weight="700" font-size="{NOTE_FONT}" xml:space="preserve">"#,
        pal.note_text
    );
    for (i, line) in item
        .note_lines(NOTE_FONT * HAND_CHAR_WIDTH)
        .iter()
        .enumerate()
    {
        let _ = write!(
            out,
            r#"<tspan x="{}" y="{}">{}</tspan>"#,
            n(r.x + NOTE_PAD),
            n(r.y + 36.0 + i as f64 * NOTE_LINE),
            esc(line)
        );
    }
    out.push_str("</text>");
    if let Some(name) = &item.author {
        let _ = write!(
            out,
            r#"<text x="{}" y="{}" fill="{}" font-family="sans-serif" font-size="{NAME_FONT}">{}</text>"#,
            n(r.x + NOTE_PAD),
            n(r.y + r.h - 8.0),
            pal.note_text,
            esc(name)
        );
    }
    out.push_str("</g>");
}

fn path_data(segs: &[Seg]) -> String {
    let mut d = String::new();
    for s in segs {
        let _ = match *s {
            Seg::Move(x, y) => write!(d, "M{} {}", n(x), n(y)),
            Seg::Line(x, y) => write!(d, "L{} {}", n(x), n(y)),
            Seg::Quad(cx, cy, x, y) => write!(d, "Q{} {} {} {}", n(cx), n(cy), n(x), n(y)),
        };
    }
    d
}

/// Escapes text for element content and attribute values, dropping characters XML forbids.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(' '),
            c if (c as u32) < 0x20 || c == '\u{fffe}' || c == '\u{ffff}' => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{Paper, tests::sample};

    #[test]
    fn small_scene_snapshot() {
        let (b, _) = sample();
        let svg = export_svg(&b, &Scope::Board, &Options::default());
        assert!(svg.starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg" "#));
        assert!(svg.ends_with("</g></svg>"));
        assert!(svg.contains("<title>Launch &amp; &lt;plan&gt;</title>"));
        // Paper, the light ink 2 stroke and its smoothed path.
        assert!(svg.contains(r##"fill="#fbfaf6""##));
        assert!(svg.contains(
            r##"<path d="M10 10Q20 20 25 15L30 10" stroke="#d03a2f" stroke-width="2"/>"##
        ));
        // Highlighter: yellow, translucent, multiplied on light paper.
        assert!(svg.contains(r##"stroke="#ffe94d" stroke-width="12" stroke-opacity="0.62" style="mix-blend-mode:multiply""##));
        assert!(svg.contains(r##"<rect x="100" y="10" width="60" height="40" rx="10" fill="#ffffff" stroke="#1f63c9""##));
        assert!(svg.contains("<ellipse cx=\"200\" cy=\"30\" rx=\"20\" ry=\"20\""));
        assert!(svg.contains(r#"<path d="M260 10L280 30L260 50L240 30Z""#));
        // Arrow line plus an open head ending at the tip.
        assert!(svg.contains(r#"<path d="M10 100L90 100M"#));
        assert!(svg.contains("L90 100L"));
        // Note: mint fill, tilt, escaped text, alt text as title.
        assert!(svg.contains(r##"fill="#a8ecc4""##));
        assert!(svg.contains(r#"<g transform="rotate("#));
        assert!(svg.contains("<title>Note about &quot;shipping&quot;</title>"));
        assert!(svg.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(!svg.contains("<script"));
        // Text object: large size, two lines.
        assert!(svg.contains(r#"font-size="40""#));
        assert!(svg.contains(">Launch!</tspan>"));
        assert!(svg.contains(">Week 2 €</tspan>"));
        // No names by default, no external references.
        assert!(!svg.contains("Maya"));
        assert!(!svg.contains("<desc>"));
        assert!(!svg.contains("href"));
        assert!(!svg.contains("url("));
        assert_eq!(svg.matches("http").count(), 1, "only the namespace");
    }

    #[test]
    fn names_dark_paper_and_transparency() {
        let (b, _) = sample();
        let svg = export_svg(
            &b,
            &Scope::Board,
            &Options {
                paper: Paper::Dark,
                transparent: true,
                with_names: true,
            },
        );
        assert!(svg.contains("<desc>By Maya &lt;admin&gt;</desc>"));
        assert!(!svg.contains("#16181d"), "no paper when transparent");
        assert!(svg.contains(r##"stroke="#ff7a70""##), "dark paper ink");
        assert!(!svg.contains("mix-blend-mode"));
        assert!(!svg.contains("Maya <"));
    }

    #[test]
    fn empty_board_is_a_valid_page() {
        let svg = export_svg(&Board::new(1), &Scope::Board, &Options::default());
        assert!(svg.contains(r#"viewBox="0 0 320 240""#));
    }

    #[test]
    fn helpers() {
        assert_eq!(n(-0.001), "0");
        assert_eq!(n(1.5), "1.5");
        assert_eq!(n(2.0), "2");
        assert_eq!(n(f64::NAN), "0");
        assert_eq!(esc("a\u{1}<'\">&\n"), "a&lt;&apos;&quot;&gt;&amp; ");
    }
}
