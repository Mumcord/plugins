// Small DOM helpers shared by the stage and the side panel. Everything from the server (names,
// note text) goes in through textContent, never as HTML.

import { SHAPE_PATHS } from "./render.js";
import { personColour } from "./objects.js";

const SVG_NS = "http://www.w3.org/2000/svg";

export function el(tag, attrs = {}, children = []) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === null || v === undefined || v === false) continue;
    if (k === "text") node.textContent = v;
    else if (k === "class") node.className = v;
    else if (k === "style") node.style.cssText = v;
    else node.setAttribute(k, v === true ? "" : String(v));
  }
  for (const c of children) if (c) node.append(c);
  return node;
}

/** Line icon in the mockups' style: 24×24, stroked with currentColor. */
export function icon(paths, size = 18) {
  const svg = document.createElementNS(SVG_NS, "svg");
  for (const [k, v] of Object.entries({
    viewBox: "0 0 24 24",
    width: size,
    height: size,
    "aria-hidden": "true",
    fill: "none",
    stroke: "currentColor",
    "stroke-width": 2,
    "stroke-linecap": "round",
    "stroke-linejoin": "round",
  })) {
    svg.setAttribute(k, String(v));
  }
  for (const d of paths) {
    const p = document.createElementNS(SVG_NS, "path");
    p.setAttribute("d", d);
    svg.append(p);
  }
  return svg;
}

/** A person's shape glyph (circle, square, diamond, triangle) in `fill`. */
export function shapeGlyph(shape, size, fill) {
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", "0 0 16 16");
  svg.setAttribute("width", String(size));
  svg.setAttribute("height", String(size));
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("fill", fill);
  const p = document.createElementNS(SVG_NS, "path");
  p.setAttribute("d", SHAPE_PATHS[shape] ?? SHAPE_PATHS[0]);
  svg.append(p);
  return svg;
}

const ARROW = "M2 2l15 8-6.5 2L7 18.5z";

/** A remote cursor element in the mockup's structure: arrow, then a label with shape and name. */
export function cursorEl(mini) {
  const arrow = document.createElementNS(SVG_NS, "svg");
  arrow.setAttribute("class", "wb-cursor__arrow");
  arrow.setAttribute("viewBox", "0 0 20 22");
  arrow.setAttribute("width", "20");
  arrow.setAttribute("height", "22");
  arrow.setAttribute("aria-hidden", "true");
  const p = document.createElementNS(SVG_NS, "path");
  p.setAttribute("d", ARROW);
  p.setAttribute("fill", "var(--c)");
  p.setAttribute("stroke", "#fff");
  p.setAttribute("stroke-width", "1.4");
  p.setAttribute("stroke-linejoin", "round");
  arrow.append(p);
  const name = el("span");
  const tag = el("span", { class: "wb-cursor__tag" });
  const label = el("span", { class: "wb-cursor__label" }, [name, tag]);
  const root = el("div", { class: mini ? "wb-cursor wb-cursor--mini" : "wb-cursor", "aria-hidden": "true" }, [arrow, label]);
  return { root, arrow, label, name, tag, glyph: null, key: "" };
}

/** Updates a cursor element for a peer; `tagText` is the status (empty = none). */
export function updateCursor(c, peer, sx, sy, tagText) {
  const key = `${peer.shape}/${peer.color}`;
  if (c.key !== key) {
    c.key = key;
    c.root.style.setProperty("--c", personColour(peer.color));
    if (c.glyph) c.glyph.remove();
    c.glyph = shapeGlyph(peer.shape, 11, "#fff");
    c.label.prepend(c.glyph);
  }
  if (c.name.textContent !== peer.name) c.name.textContent = peer.name;
  if (c.tag.textContent !== tagText) c.tag.textContent = tagText;
  c.tag.hidden = !tagText;
  c.root.style.left = `${sx}px`;
  c.root.style.top = `${sy}px`;
}

/**
 * Roving focus for a toolbar or radio group: one tab stop, arrow keys move between the items.
 * `items()` returns the current buttons; `vertical` picks Up/Down instead of Left/Right.
 */
export function roving(container, items, vertical, onMove) {
  container.addEventListener("keydown", (e) => {
    const prev = vertical ? "ArrowUp" : "ArrowLeft";
    const next = vertical ? "ArrowDown" : "ArrowRight";
    if (![prev, next, "Home", "End"].includes(e.key)) return;
    const list = items().filter((b) => !b.disabled && !b.hidden);
    const i = list.indexOf(document.activeElement);
    if (i < 0) return;
    e.preventDefault();
    e.stopPropagation();
    let j = i;
    if (e.key === prev) j = (i - 1 + list.length) % list.length;
    else if (e.key === next) j = (i + 1) % list.length;
    else if (e.key === "Home") j = 0;
    else j = list.length - 1;
    for (const b of list) b.tabIndex = -1;
    list[j].tabIndex = 0;
    list[j].focus();
    if (onMove) onMove(list[j]);
  });
}

/** Makes `active` the single tab stop of a roving group. */
export function setTabStop(buttons, active) {
  for (const b of buttons) b.tabIndex = b === active ? 0 : -1;
}
