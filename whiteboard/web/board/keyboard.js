// Pure keyboard logic: tool shortcuts, the keyboard cursor and global shortcuts.

/** Tool ids in toolbar order with their single-letter shortcuts. */
export const TOOLS = [
  { id: "select", key: "V" },
  { id: "pen", key: "P" },
  { id: "hl", key: "H" },
  { id: "eraser", key: "E" },
  { id: "shapes", key: "S" },
  { id: "arrow", key: "A" },
  { id: "note", key: "N" },
  { id: "text", key: "T" },
  { id: "laser", key: "L" },
];

/** The shape kinds the S tool cycles through. */
export const SHAPES = ["rect", "ellipse", "diamond", "line"];

export const KBD_STEP = 8;
export const KBD_STEP_BIG = 64;

/** Tool for a plain letter key, or null. Modifier keys are handled by the caller. */
export function toolForKey(key) {
  if (typeof key !== "string" || key.length !== 1) return null;
  const k = key.toUpperCase();
  const t = TOOLS.find((tool) => tool.key === k);
  return t ? t.id : null;
}

/** Pressing S again while the shape tool is active picks the next shape. */
export function nextShape(current) {
  const i = SHAPES.indexOf(current);
  return SHAPES[(i + 1) % SHAPES.length];
}

const ARROWS = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };

export function isArrowKey(key) {
  return Object.hasOwn(ARROWS, key);
}

/**
 * Moves the keyboard cursor (world coordinates). Steps are 8 or 64 screen px, so they feel the
 * same at every zoom level.
 */
export function moveKbCursor(pos, key, shift, zoom) {
  const d = ARROWS[key];
  if (!d) return pos;
  const step = (shift ? KBD_STEP_BIG : KBD_STEP) / (zoom > 0 ? zoom : 1);
  return { x: pos.x + d[0] * step, y: pos.y + d[1] * step };
}

/**
 * Keeps the keyboard cursor inside the visible world rectangle so it never gets lost off screen;
 * returns the camera shift (world px) needed to follow it instead when it would leave the view.
 */
export function followShift(pos, view, margin) {
  let dx = 0;
  let dy = 0;
  if (pos.x < view.x + margin) dx = pos.x - (view.x + margin);
  else if (pos.x > view.x + view.w - margin) dx = pos.x - (view.x + view.w - margin);
  if (pos.y < view.y + margin) dy = pos.y - (view.y + margin);
  else if (pos.y > view.y + view.h - margin) dy = pos.y - (view.y + view.h - margin);
  return { dx, dy };
}

/**
 * Board-wide shortcuts with Ctrl/Cmd, returned as an action name. `mod` is Ctrl or Cmd.
 */
export function commandFor(key, mod, shift) {
  if (!mod) return null;
  const k = key.length === 1 ? key.toLowerCase() : key;
  if (k === "z") return shift ? "redo" : "undo";
  if (k === "y") return "redo";
  if (k === "d") return "dup";
  if (k === "a") return "selectAll";
  if (k === "]") return "front";
  if (k === "[") return "back";
  if (k === "0") return "fit";
  if (k === "=" || k === "+") return "zoomIn";
  if (k === "-") return "zoomOut";
  return null;
}

/** Shortcuts shown in the ?/F1 help, as [keys, string key of the description]. */
export const HELP = [
  ["V", "tool.select"],
  ["P", "tool.pen"],
  ["H", "tool.hl"],
  ["E", "tool.eraser"],
  ["S", "help.shapes"],
  ["A", "tool.arrow"],
  ["N", "tool.note"],
  ["T", "tool.text"],
  ["L", "tool.laser"],
  ["Space", "help.hand"],
  ["Ctrl+Z", "help.undo"],
  ["Ctrl+Shift+Z", "help.redo"],
  ["Delete", "help.delete"],
  ["Ctrl+D", "help.dup"],
  ["Ctrl+A", "help.selectAll"],
  ["Ctrl+] / Ctrl+[", "help.order"],
  ["+ / -", "help.zoom"],
  ["Ctrl+0", "help.fit"],
  ["← ↑ → ↓", "help.kbdMove"],
  ["Space", "help.kbdPen"],
  ["Enter", "help.kbdPlace"],
  ["Tab", "help.outline"],
  ["Ctrl+Enter", "help.editDone"],
  ["? / F1", "help.help"],
  ["Esc", "help.esc"],
];

/** Should board shortcuts stay quiet because the user is typing into a field? */
export function isTypingTarget(el) {
  if (!el || typeof el.tagName !== "string") return false;
  const tag = el.tagName.toUpperCase();
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || el.isContentEditable === true;
}
