// Pure board state on the page: a copy of what the plugin sent. The page never decides what is on
// the board; it only mirrors `objs` messages and draws them.

import { objectBounds, readingOrder, regionOf, boundsOf } from "./geom.js";

export const KINDS = ["stroke", "hl", "rect", "ellipse", "diamond", "line", "arrow", "note", "text"];
const MAX_POINTS = 4000; // 2,000 points as x/y pairs
const MAX_TEXT = 500;
const MAX_ALT = 200;
const MAX_COORD = 1e7;

function num(v, fallback) {
  return typeof v === "number" && Number.isFinite(v) && Math.abs(v) <= MAX_COORD ? v : fallback;
}

function int(v, lo, hi) {
  return Number.isInteger(v) && v >= lo && v <= hi ? v : lo;
}

function str(v, max) {
  return typeof v === "string" ? v.slice(0, max) : "";
}

/**
 * Validates one object from the plugin. Messages come from our own plugin, but its data comes
 * from other people, so anything malformed is dropped instead of drawn.
 */
export function normalizeObj(raw) {
  if (!raw || typeof raw !== "object") return null;
  if (typeof raw.id !== "string" || raw.id.length === 0 || raw.id.length > 40) return null;
  if (!KINDS.includes(raw.k)) return null;
  if (!Array.isArray(raw.g) || raw.g.length !== 4) return null;
  const g = raw.g.map((v) => num(v, NaN));
  if (g.some((v) => Number.isNaN(v))) return null;
  const obj = {
    id: raw.id,
    k: raw.k,
    g,
    ink: int(raw.ink, 0, 5),
    w: int(raw.w, 0, 2),
    fill: int(raw.fill, 0, 4),
    text: str(raw.text, MAX_TEXT),
    z: str(raw.z, 64),
    alt: str(raw.alt, MAX_ALT),
    by: Number.isInteger(raw.by) && raw.by >= 0 ? raw.by : 0,
  };
  if (obj.k === "stroke" || obj.k === "hl") {
    if (!Array.isArray(raw.pts) || raw.pts.length < 2 || raw.pts.length > MAX_POINTS) return null;
    const pts = raw.pts.slice(0, raw.pts.length - (raw.pts.length % 2)).map((v) => num(v, NaN));
    if (pts.some((v) => Number.isNaN(v))) return null;
    obj.pts = pts;
    obj.bw = num(raw.bw, 0);
    obj.bh = num(raw.bh, 0);
  }
  return obj;
}

/** Paint order: fractional z index, then id so equal z values still sort the same everywhere. */
export function compareZ(a, b) {
  if (a.z !== b.z) return a.z < b.z ? -1 : 1;
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
}

export class ObjectStore {
  constructor() {
    this.map = new Map();
    this.sorted = null;
  }

  /** Applies an `objs` message; returns the ids that were added (for "x added" focus handling). */
  apply(msg) {
    if (msg.reset === true) this.map.clear();
    const added = [];
    if (Array.isArray(msg.remove)) {
      for (const id of msg.remove) if (typeof id === "string") this.map.delete(id);
    }
    if (Array.isArray(msg.upsert)) {
      for (const raw of msg.upsert) {
        const obj = normalizeObj(raw);
        if (!obj) continue;
        if (!this.map.has(obj.id)) added.push(obj.id);
        this.map.set(obj.id, obj);
      }
    }
    this.sorted = null;
    return added;
  }

  get(id) {
    return this.map.get(id) || null;
  }

  get size() {
    return this.map.size;
  }

  /** All objects in paint order (back to front). */
  list() {
    if (!this.sorted) this.sorted = [...this.map.values()].sort(compareZ);
    return this.sorted;
  }
}

/** One spoken outline line, e.g. "Sticky note by Maya, yellow, 'Docs: update', top left". */
export function describeObj(obj, t, nameOf, board) {
  const parts = [t("outline.item", { kind: t(`kind.${obj.k}`), name: nameOf(obj.by) })];
  if (obj.k === "note") parts.push(t(`note.${obj.fill}`).toLowerCase());
  else if (obj.k === "hl" && obj.ink === 0) parts.push(t("hl.0").toLowerCase());
  else parts.push(t(`ink.${obj.ink}`).toLowerCase());
  const text = obj.text.trim().replace(/\s+/g, " ");
  if (text) parts.push(`'${text.length > 80 ? `${text.slice(0, 79)}…` : text}'`);
  if (obj.alt.trim()) parts.push(obj.alt.trim());
  parts.push(t(`pos.${regionOf(objectBounds(obj), board)}`));
  return parts.join(", ");
}

/** Outline entries in reading order, optionally only one author's objects. */
export function outlineEntries(objs, t, nameOf, author = null) {
  const board = boundsOf(objs);
  const list = author === null ? objs : objs.filter((o) => o.by === author);
  return readingOrder(list).map((o) => ({ id: o.id, label: describeObj(o, t, nameOf, board) }));
}

export const PERSON_COLOURS = ["#0b6fb8", "#1a7f5a", "#6a4fc4", "#c25a00", "#c0392b", "#b4348a"];

/** Person colour for a peer's `color` index; unknown people get a neutral grey. */
export function personColour(index) {
  return PERSON_COLOURS[index] ?? "#6b6b78";
}

/** Validates the `peers` list from the plugin. */
export function normalizePeers(list) {
  if (!Array.isArray(list)) return [];
  const out = [];
  for (const p of list.slice(0, 64)) {
    if (!p || typeof p !== "object" || !Number.isInteger(p.author)) continue;
    const cursor = Array.isArray(p.cursor) && p.cursor.length === 2 && p.cursor.every((v) => Number.isFinite(v)) ? p.cursor : null;
    out.push({
      author: p.author,
      name: str(p.name, 64) || "?",
      shape: int(p.shape, 0, 3),
      color: int(p.color, 0, 5),
      cursor,
      ago: Number.isFinite(p.ago) && p.ago >= 0 ? p.ago : null,
      status: str(p.status, 20),
      speaking: p.speaking === true,
      host: p.host === true,
      canDraw: p.canDraw !== false,
      hidden: p.hidden === true,
    });
  }
  return out;
}

/** The side panel's wording for a peer status ("drawing", "typing", …); idle people are watching. */
export function peerStateKey(status) {
  return ["pointing", "drawing", "typing", "highlighting"].includes(status) ? status : "watching";
}
