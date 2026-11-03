// Canvas drawing of board objects. Works on any 2D context, so the stage, the mini view and PNG
// renders share it. Colours come in as a palette read from the CSS tokens (ink 1…6, note colours,
// paper), which keeps one drawing readable on light, dark and high-contrast paper.

import { normRect, strokePoints, strokeWidthOf, TEXT_SIZES, NOTE_SIZE } from "./geom.js";
import { personColour } from "./objects.js";

export const HAND_FAMILY = '"Caveat", "Segoe Print", "Comic Sans MS", cursive';
const NOTE_FONT = 25;
const NOTE_LINE = 28;
const NOTE_PAD = 14;

/** 16×16 glyph paths for the four person shapes: circle, square, diamond, triangle. */
export const SHAPE_PATHS = [
  "M8 2a6 6 0 1 0 0.001 0z",
  "M2.5 2.5h11v11h-11z",
  "M8 1l7 7-7 7-7-7z",
  "M8 1.5l7 12.5H1z",
];

/**
 * Canvas text does not make the browser fetch a web font, so the hand-written face is loaded
 * explicitly; `onReady` redraws once it is there.
 */
export function loadHandFont(onReady) {
  if (!document.fonts) return;
  document.fonts.load(`700 25px ${HAND_FAMILY}`).then(onReady, (err) => console.warn("whiteboard: the hand-written font did not load", err.name));
}

/** Reads the drawing palette from CSS custom properties on `el`. */
export function readPalette(el) {
  const cs = getComputedStyle(el);
  const v = (name, fallback) => cs.getPropertyValue(name).trim() || fallback;
  return {
    paper: v("--wb-paper", "#16181d"),
    box: v("--wb-box", "#1d2027"),
    inks: [1, 2, 3, 4, 5, 6].map((i) => v(`--ink-${i}`, "#888")),
    hl: v("--hl", "#f4dc4f"),
    hlAlpha: Number.parseFloat(v("--wb-hl-alpha", "0.62")) || 0.62,
    notes: ["yellow", "pink", "mint", "sky", "lilac"].map((n) => v(`--note-${n}`, "#dcbc4a")),
    noteText: v("--wb-note-text", "#24211a"),
    accent: v("--wb-accent", "#a78bff"),
    multiply: v("--wb-hl-blend", "normal") === "multiply",
  };
}

/** Small, stable tilt per note (−2° … 2°) so a wall of notes looks hand-placed, the same for everyone. */
export function noteTilt(id) {
  let h = 2166136261;
  for (let i = 0; i < id.length; i++) h = Math.imul(h ^ id.charCodeAt(i), 16777619);
  return (((h >>> 0) % 9) - 4) * 0.5;
}

export function textSize(obj) {
  return TEXT_SIZES[obj.w] ?? TEXT_SIZES[1];
}

/** Breaks text into lines that fit `maxWidth`, keeping explicit line breaks. */
export function wrapText(ctx, text, maxWidth) {
  const lines = [];
  for (const para of text.split("\n")) {
    let line = "";
    for (const word of para.split(/(\s+)/)) {
      const next = line + word;
      if (line && ctx.measureText(next).width > maxWidth && word.trim()) {
        lines.push(line.trimEnd());
        line = word.trimStart();
      } else {
        line = next;
      }
    }
    lines.push(line);
  }
  return lines;
}

function tracePolyline(ctx, p) {
  ctx.beginPath();
  ctx.moveTo(p[0], p[1]);
  if (p.length === 2) {
    ctx.lineTo(p[0] + 0.01, p[1]);
    return;
  }
  // Quadratic curves through the midpoints keep hand-drawn strokes smooth without extra points.
  for (let i = 2; i + 3 < p.length; i += 2) {
    const mx = (p[i] + p[i + 2]) / 2;
    const my = (p[i + 1] + p[i + 3]) / 2;
    ctx.quadraticCurveTo(p[i], p[i + 1], mx, my);
  }
  ctx.lineTo(p[p.length - 2], p[p.length - 1]);
}

function inkOf(obj, pal) {
  if (obj.k === "hl" && obj.ink === 0) return pal.hl;
  return pal.inks[obj.ink] ?? pal.inks[0];
}

function drawArrowHead(ctx, x0, y0, x1, y1, size) {
  const a = Math.atan2(y1 - y0, x1 - x0);
  ctx.beginPath();
  ctx.moveTo(x1 - size * Math.cos(a - 0.45), y1 - size * Math.sin(a - 0.45));
  ctx.lineTo(x1, y1);
  ctx.lineTo(x1 - size * Math.cos(a + 0.45), y1 - size * Math.sin(a + 0.45));
  ctx.stroke();
}

/** Draws one object in world coordinates (the caller sets the camera transform). */
export function drawObject(ctx, obj, pal, peerOf) {
  const [x, y, w, h] = obj.g;
  const lw = strokeWidthOf(obj);
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  switch (obj.k) {
    case "stroke":
    case "hl": {
      ctx.save();
      ctx.strokeStyle = inkOf(obj, pal);
      ctx.lineWidth = lw;
      if (obj.k === "hl") {
        ctx.globalAlpha *= pal.hlAlpha;
        if (pal.multiply) ctx.globalCompositeOperation = "multiply";
      }
      tracePolyline(ctx, strokePoints(obj));
      ctx.stroke();
      ctx.restore();
      break;
    }
    case "rect":
    case "ellipse":
    case "diamond": {
      const r = normRect(x, y, w, h);
      ctx.beginPath();
      if (obj.k === "rect") ctx.roundRect(r.x, r.y, r.w, r.h, Math.min(12, r.w / 4, r.h / 4));
      else if (obj.k === "ellipse") ctx.ellipse(r.x + r.w / 2, r.y + r.h / 2, r.w / 2, r.h / 2, 0, 0, Math.PI * 2);
      else {
        ctx.moveTo(r.x + r.w / 2, r.y);
        ctx.lineTo(r.x + r.w, r.y + r.h / 2);
        ctx.lineTo(r.x + r.w / 2, r.y + r.h);
        ctx.lineTo(r.x, r.y + r.h / 2);
        ctx.closePath();
      }
      ctx.fillStyle = pal.box;
      ctx.fill();
      ctx.strokeStyle = inkOf(obj, pal);
      ctx.lineWidth = lw;
      ctx.stroke();
      break;
    }
    case "line":
    case "arrow": {
      ctx.strokeStyle = inkOf(obj, pal);
      ctx.lineWidth = lw;
      ctx.beginPath();
      ctx.moveTo(x, y);
      ctx.lineTo(x + w, y + h);
      ctx.stroke();
      if (obj.k === "arrow" && (w !== 0 || h !== 0)) drawArrowHead(ctx, x, y, x + w, y + h, 10 + lw * 2);
      break;
    }
    case "note":
      drawNote(ctx, obj, pal, peerOf);
      break;
    case "text": {
      const size = textSize(obj);
      ctx.fillStyle = inkOf(obj, pal);
      ctx.font = `700 ${size}px ${HAND_FAMILY}`;
      ctx.textBaseline = "top";
      const lines = obj.text.split("\n");
      lines.forEach((line, i) => ctx.fillText(line, x, y + i * size * 1.1));
      break;
    }
    default:
      break;
  }
}

function drawNote(ctx, obj, pal, peerOf) {
  const r = normRect(...obj.g);
  const cx = r.x + r.w / 2;
  const cy = r.y + r.h / 2;
  ctx.save();
  ctx.translate(cx, cy);
  ctx.rotate((noteTilt(obj.id) * Math.PI) / 180);
  ctx.translate(-cx, -cy);
  ctx.save();
  ctx.shadowColor = "rgb(0 0 0 / 28%)";
  ctx.shadowBlur = 5;
  ctx.shadowOffsetY = 2;
  ctx.fillStyle = pal.notes[obj.fill] ?? pal.notes[0];
  ctx.beginPath();
  ctx.roundRect(r.x, r.y, r.w, r.h, 3);
  ctx.fill();
  ctx.restore();
  ctx.fillStyle = pal.noteText;
  ctx.font = `700 ${NOTE_FONT}px ${HAND_FAMILY}`;
  ctx.textBaseline = "alphabetic";
  const lines = wrapText(ctx, obj.text, r.w - 2 * NOTE_PAD);
  const max = Math.max(1, Math.floor((r.h - 22) / NOTE_LINE));
  lines.slice(0, max).forEach((line, i) => ctx.fillText(line, r.x + NOTE_PAD, r.y + 36 + i * NOTE_LINE));
  // Author mark: colour and shape together, never colour alone.
  const peer = peerOf ? peerOf(obj.by) : null;
  if (peer) {
    ctx.translate(r.x + r.w - 24, r.y + r.h - 24);
    ctx.scale(0.95, 0.95);
    ctx.fillStyle = personColour(peer.color);
    ctx.fill(new Path2D(SHAPE_PATHS[peer.shape] ?? SHAPE_PATHS[0]));
  }
  ctx.restore();
}

/** Default size of a new note. */
export function defaultNoteGeom(x, y) {
  return [x, y, NOTE_SIZE.w, NOTE_SIZE.h];
}

/**
 * Draws all objects for camera `cam` on a context whose pixels are `dpr` times CSS px.
 * `opts.hidden` leaves out authors hidden locally, `opts.skip` leaves out ids (the object being
 * edited), `opts.faded` dims ids (while erasing), `opts.override` swaps geometry during a drag.
 */
export function drawScene(ctx, objs, cam, dpr, pal, opts = {}) {
  ctx.save();
  ctx.setTransform(dpr * cam.zoom, 0, 0, dpr * cam.zoom, -cam.x * cam.zoom * dpr, -cam.y * cam.zoom * dpr);
  for (const obj of objs) {
    if (opts.hidden && opts.hidden.has(obj.by)) continue;
    if (opts.skip && opts.skip.has(obj.id)) continue;
    const g = opts.override ? opts.override.get(obj.id) : null;
    const o = g ? { ...obj, g } : obj;
    ctx.globalAlpha = opts.faded && opts.faded.has(obj.id) ? 0.25 : 1;
    drawObject(ctx, o, pal, opts.peerOf);
  }
  ctx.restore();
}

/** Someone else's stroke while it is being drawn, in their person colour with a live dot at the tip. */
export function drawLive(ctx, flat, colour, width, pulse) {
  if (flat.length < 2) return;
  ctx.save();
  ctx.strokeStyle = colour;
  ctx.lineWidth = width;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  tracePolyline(ctx, flat);
  ctx.stroke();
  ctx.fillStyle = colour;
  ctx.globalAlpha = 1 - pulse * 0.4;
  ctx.beginPath();
  ctx.arc(flat[flat.length - 2], flat[flat.length - 1], 5 + pulse * 2, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
}
