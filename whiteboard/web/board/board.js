// The stage page of the whiteboard: canvas renderer, tools, camera, selection, live presence,
// presenter mode and keyboard access. It holds no authority: every edit goes to the plugin as a
// bridge message and only becomes part of the board when it comes back in `objs`.

import { connect } from "./bridge.js";
import { makeT } from "./strings.js";
import { createTheme } from "./theme.js";
import { createChrome, toolAllowed } from "./chrome.js";
import { el, cursorEl, updateCursor } from "./dom.js";
import { ObjectStore, normalizePeers, personColour } from "./objects.js";
import { readPalette, drawScene, drawLive, textSize, HAND_FAMILY, noteTilt, loadHandFont } from "./render.js";
import { answerRender, drawPreview } from "./pngrender.js";
import { createDialogs } from "./dialogs.js";
import { addTrail, pruneTrail, trailAlpha, TRAIL_MS } from "./trail.js";
import { nextShape, toolForKey, commandFor, isArrowKey, moveKbCursor, followShift, isTypingTarget } from "./keyboard.js";
import {
  screenToWorld, worldToScreen, zoomAt, panBy, viewRect, viewCenter, cameraFromCenter, fitRect, clampZoom,
  rectFromPoints, inflate, topHit, objectsInRect, boundsOf, objectBounds, translateGeom, resizeGeom,
  handlesOf, appendPoint, strokeObject, pointInRect, q, NOTE_SIZE,
} from "./geom.js";

const $ = (id) => document.getElementById(id);
const wb = $("wb");
const scene = $("scene");
const overlay = $("overlay");
const sctx = scene.getContext("2d");
const octx = overlay.getContext("2d");
const editor = $("editor");
const kbEl = $("kbcursor");

const MAX_STROKE_POINTS = 2000;
const SEND_EVERY_MS = 66; // cursor, live stroke and laser updates: about 15 per second each
const PENDING_MS = 800;
const LIVE_KEEP_MS = 1500;
const HIT_PX = 6;

const app = {
  t: makeT({}),
  me: { author: -1, name: "", shape: 0, color: 0 },
  board: { id: "", title: "" },
  settings: { policy: "everyone", laser: false, host: -1, canDraw: true },
  status: { relay: true, queue: 0, saved: "", host: false, presenting: null, following: false, canDraw: true, loading: null, undo: false, redo: false },
  store: new ObjectStore(),
  peers: new Map(),
  cam: { x: -80, y: -60, zoom: 1 },
  vw: 0,
  vh: 0,
  dpr: 1,
  tool: "pen",
  shape: "rect",
  ink: 0,
  hlInk: 0,
  fill: 0,
  widths: { pen: 1, hl: 1, shapes: 1, arrow: 1, text: 1 },
  size: { shapes: [160, 100], arrow: [160, 0], note: [NOTE_SIZE.w, NOTE_SIZE.h] },
  selected: new Set(),
  hidden: new Set(),
};

let palette = null;
let gesture = null;
let pending = null; // what a just-finished gesture looks like until `objs` confirms it
let editing = null; // { id | null, kind, g, ink, w, fill }
let sceneDirty = true;
let overlayDirty = true;
let frameQueued = false;
let spaceHeld = false;
let strokeSid = 0;
const kb = { on: false, x: 0, y: 0, down: false };
const live = new Map(); // author -> { sid, pts, doneAt }
const lasers = new Map(); // author -> trail (own laser under app.me.author)
const laserEls = new Map();
const cursorEls = new Map();
const pointers = new Map();
const lastSent = { cursor: 0, view: 0 };
let viewTimer = 0;
let pointerInside = false;
let lastPointer = null;

const bridge = connect(onMessage, 64 * 1024, "stage");
const send = (msg, ephemeral = false) => bridge.send(msg, ephemeral);
const exportScope = () => {
  const v = viewRect(app.cam, app.vw, app.vh);
  return { view: [v.x, v.y, v.w, v.h].map(q), sel: [...app.selected].slice(0, 2000) };
};
const dialogs = createDialogs((key, vars) => app.t(key, vars), send, exportScope, (canvas, area, transparent) =>
  drawPreview(canvas, area, transparent, {
    objs: app.store.list(),
    selected: app.selected,
    view: viewRect(app.cam, app.vw, app.vh),
    palette,
    peerOf,
    hidden: app.hidden,
  }),
);
const theme = createTheme(() => {
  palette = readPalette(document.documentElement);
  sceneDirty = true;
  requestFrame();
});

// --- Actions (bridge messages) ---------------------------------------------------------------------

const act = {
  chooseTool,
  setInk(i) {
    if (app.tool === "hl") app.hlInk = i;
    else app.ink = i;
    chrome.renderProps();
  },
  setWidth(i) {
    if (Object.hasOwn(app.widths, app.tool)) app.widths[app.tool] = i;
    chrome.renderProps();
  },
  setFill(i) {
    app.fill = i;
    chrome.renderProps();
  },
  setShape(s) {
    app.shape = s;
    chrome.renderProps();
  },
  setSize(tool, axis, v) {
    app.size[tool][axis] = v;
  },
  setSelection(fields) {
    for (const id of app.selected) {
      const o = app.store.get(id);
      if (!o) continue;
      const f = {};
      if ("ink" in fields && o.k !== "note") f.ink = fields.ink;
      if ("w" in fields && o.k !== "note") f.w = fields.w;
      if ("fill" in fields && o.k === "note") f.fill = fields.fill;
      if ("alt" in fields) f.alt = fields.alt;
      if (Object.keys(f).length > 0) send({ t: "set", id, fields: f });
    }
  },
  setSelGeom(axis, v) {
    const [id] = app.selected;
    const o = app.store.get(id);
    if (!o) return;
    const g = [...o.g];
    // Lines and arrows keep their direction; boxes just get the new size.
    g[axis] = o.k === "line" || o.k === "arrow" ? Math.sign(g[axis] || 1) * v : v;
    send({ t: "set", id, fields: { g: g.map(q) } });
  },
  selectionOp(op) {
    const ids = [...app.selected];
    if (op === "edit") startEdit(app.store.get(ids[0]));
    else if (ids.length > 0) send({ t: op, ids });
  },
  undo: () => send({ t: "undo" }),
  redo: () => send({ t: "redo" }),
  zoomStep,
  fit: fitBoard,
  present: (on) => send({ t: "present", on }),
  follow: (on) => send({ t: "follow", on }),
  // The dialogs need what the page knows: the view (export "what I see now") and the selection
  // (export or post the selection).
  menu: (item) => send({ t: "menu", item, ...exportScope() }),
  toggleOutline: () => chrome.toggleOutline(),
  openHelp: () => chrome.openHelp(),
  selectOnly(id, reveal) {
    const o = app.store.get(id);
    if (!o) return;
    setSelection(new Set([id]));
    if (reveal) revealObject(o);
  },
  editOrDescribe(id) {
    const o = app.store.get(id);
    if (!o) return;
    if (o.k === "note" || o.k === "text") startEdit(o);
    else {
      setSelection(new Set([id]));
      app.tool = "select";
      chrome.updateTools();
      const alt = $("alt-input");
      if (alt) alt.focus();
    }
  },
  deleteIds(ids) {
    if (ids.length > 0) send({ t: "delete", ids });
  },
};

const chrome = createChrome(app, act);

function chooseTool(id, fromKey) {
  if (!toolAllowed(app, id)) return;
  if (id === "shapes" && app.tool === "shapes" && fromKey) app.shape = nextShape(app.shape);
  if (id !== app.tool) cancelGesture();
  app.tool = id;
  if (id !== "select") setSelection(new Set());
  chrome.updateTools();
  overlayDirty = true;
  requestFrame();
}

function setSelection(ids) {
  app.selected = ids;
  chrome.renderProps();
  for (const li of $("outline-tree").children) li.setAttribute("aria-selected", String(ids.has(li.dataset.id)));
  overlayDirty = true;
  requestFrame();
}

function canEdit() {
  return app.status.canDraw;
}

// --- Messages from the plugin ----------------------------------------------------------------------

function isNum(v) {
  return typeof v === "number" && Number.isFinite(v);
}

function flatPoints(pts) {
  if (!Array.isArray(pts) || pts.length > 4000) return [];
  const out = [];
  for (let i = 0; i + 1 < pts.length; i += 2) if (isNum(pts[i]) && isNum(pts[i + 1])) out.push(pts[i], pts[i + 1]);
  return out;
}

function onMessage(msg) {
  switch (msg.t) {
    case "init":
      onInit(msg);
      break;
    case "objs":
      onObjs(msg);
      break;
    case "peers":
      onPeers(msg);
      break;
    case "cursor": {
      const p = app.peers.get(msg.author);
      if (!p) break;
      p.cursor = isNum(msg.x) && isNum(msg.y) ? [msg.x, msg.y] : null;
      p.status = typeof msg.status === "string" ? msg.status.slice(0, 20) : "";
      updateCursors();
      break;
    }
    case "live": {
      if (!Number.isInteger(msg.author) || msg.author === app.me.author) break;
      let entry = live.get(msg.author);
      if (!entry || entry.sid !== msg.sid) {
        entry = { sid: msg.sid, pts: [], doneAt: 0 };
        live.set(msg.author, entry);
      }
      const add = flatPoints(msg.pts);
      if (entry.pts.length + add.length <= 4000) entry.pts.push(...add);
      if (msg.done === true) entry.doneAt = performance.now();
      overlayDirty = true;
      requestFrame();
      break;
    }
    case "laser": {
      if (!Number.isInteger(msg.author) || msg.author === app.me.author) break;
      if (!lasers.has(msg.author)) lasers.set(msg.author, []);
      addTrail(lasers.get(msg.author), flatPoints(msg.pts), performance.now());
      overlayDirty = true;
      requestFrame();
      break;
    }
    case "view":
      if (app.status.following && msg.author === app.status.presenting && msg.author !== app.me.author && isNum(msg.x) && isNum(msg.y) && isNum(msg.zoom)) {
        setCamera(cameraFromCenter(msg.x, msg.y, msg.zoom, app.vw, app.vh), false);
      }
      break;
    case "status":
      onStatus(msg);
      break;
    case "announce":
      announce(msg.text);
      break;
    case "render":
      answerRender(msg, {
        objs: app.store.list(),
        selected: app.selected,
        view: viewRect(app.cam, app.vw, app.vh),
        palette,
        peerOf,
        hidden: app.hidden,
        send: (m) => send(m),
      });
      break;
    case "boards":
      showBoards(msg.list);
      break;
    case "dialog":
      dialogs.open(msg);
      break;
    case "note":
      dialogs.note(msg);
      break;
    case "cleared":
      dialogs.cleared(msg);
      break;
    default:
      break;
  }
}

function onInit(msg) {
  app.t = makeT(msg.strings);
  if (typeof msg.strings?.lang === "string") document.documentElement.lang = msg.strings.lang.slice(0, 16);
  if (typeof msg.lang === "string") document.documentElement.lang = msg.lang.slice(0, 16);
  const me = msg.me || {};
  app.me = {
    author: Number.isInteger(me.author) ? me.author : -1,
    name: typeof me.name === "string" ? me.name.slice(0, 64) : "",
    shape: Number.isInteger(me.shape) ? me.shape : 0,
    color: Number.isInteger(me.color) ? me.color : 0,
  };
  const b = msg.board || {};
  app.board = { id: typeof b.id === "string" ? b.id : "", title: typeof b.title === "string" ? b.title.slice(0, 64) : "" };
  document.title = app.board.title || app.t("app.title");
  const s = msg.settings || {};
  app.settings = { policy: String(s.policy ?? "everyone"), laser: s.laser === true, host: Number.isInteger(s.host) ? s.host : -1, canDraw: s.canDraw !== false };
  app.status.canDraw = app.settings.canDraw;
  app.status.relay = msg.relay !== false;
  app.status.host = app.settings.host === app.me.author;
  theme.set(msg);
  closeBoards();
  chrome.build();
  if (!toolAllowed(app, app.tool)) app.tool = "select";
  chrome.updateTools();
  updateCursors();
}

function onObjs(msg) {
  const added = app.store.apply(msg);
  if (msg.reset === true) app.selected = new Set([...app.selected].filter((id) => app.store.get(id)));
  if (Array.isArray(msg.remove)) {
    let changed = false;
    for (const id of msg.remove) changed = app.selected.delete(id) || changed;
    if (changed) setSelection(new Set(app.selected));
  }
  // A finished stroke from someone stops being drawn live once their object arrives.
  for (const id of added) {
    const o = app.store.get(id);
    if (o && live.has(o.by) && live.get(o.by).doneAt) live.delete(o.by);
  }
  pending = null;
  if (editing && editing.id && !app.store.get(editing.id)) cancelEdit();
  sceneDirty = true;
  overlayDirty = true;
  requestFrame();
  chrome.scheduleOutline();
  if (app.selected.size > 0) chrome.renderProps();
}

function onPeers(msg) {
  const next = new Map();
  for (const p of normalizePeers(msg.list)) next.set(p.author, p);
  app.peers = next;
  app.hidden = new Set([...next.values()].filter((p) => p.hidden).map((p) => p.author));
  sceneDirty = true;
  updateCursors();
  chrome.updateBanner();
  chrome.scheduleOutline();
  requestFrame();
}

function onStatus(msg) {
  const s = app.status;
  if (typeof msg.relay === "boolean") s.relay = msg.relay;
  if (Number.isInteger(msg.queue) && msg.queue >= 0) s.queue = msg.queue;
  if (typeof msg.saved === "string") s.saved = msg.saved.slice(0, 120);
  if (typeof msg.host === "boolean") s.host = msg.host;
  const wasPresenting = s.presenting === app.me.author;
  if ("presenting" in msg) s.presenting = Number.isInteger(msg.presenting) ? msg.presenting : null;
  if (typeof msg.following === "boolean") s.following = msg.following;
  if (typeof msg.canDraw === "boolean") s.canDraw = msg.canDraw;
  if ("loading" in msg) s.loading = isNum(msg.loading) ? Math.max(0, Math.min(100, Math.round(msg.loading))) : null;
  if (typeof msg.undo === "boolean") s.undo = msg.undo;
  if (typeof msg.redo === "boolean") s.redo = msg.redo;
  if (typeof msg.laser === "boolean") app.settings.laser = msg.laser;
  if (typeof msg.paper === "string") theme.set({ paper: msg.paper });
  if (!toolAllowed(app, app.tool)) {
    cancelGesture();
    if (editing) cancelEdit();
    app.tool = s.presenting === app.me.author ? "laser" : "select";
  }
  const presentingNow = s.presenting === app.me.author;
  // Starting to present picks the laser, as in the presenter bar.
  if (presentingNow && !wasPresenting) {
    cancelGesture();
    setSelection(new Set());
    app.tool = "laser";
  }
  chrome.updateStatus();
  if (presentingNow && !wasPresenting) sendView(true);
}

function announce(text) {
  if (typeof text !== "string" || !text) return;
  const region = $("announce");
  // Clearing first makes screen readers repeat an identical announcement.
  region.textContent = "";
  setTimeout(() => {
    region.textContent = text.slice(0, 300);
  }, 30);
}

function peerOf(author) {
  if (author === app.me.author) return app.me;
  return app.peers.get(author) || null;
}

// --- Start screen (saved boards) ---------------------------------------------------------------

let boardsEl = null;

function closeBoards() {
  if (boardsEl) boardsEl.remove();
  boardsEl = null;
}

function showBoards(list) {
  closeBoards();
  if (!Array.isArray(list) || list.length === 0) return;
  boardsEl = el("div", { class: "wb-banner glass wb-banner--follow", role: "group", "aria-label": app.t("boards.label") });
  for (const b of list.slice(0, 8)) {
    if (!b || typeof b.id !== "string" || typeof b.title !== "string") continue;
    const open = el("button", { class: "wb-banner__btn", type: "button", text: app.t("boards.continue", { title: b.title.slice(0, 64) }) });
    open.addEventListener("click", () => send({ t: "open", board: b.id }));
    boardsEl.append(open);
  }
  const fresh = el("button", { class: "wb-banner__btn", type: "button", text: app.t("boards.new") });
  fresh.addEventListener("click", () => send({ t: "new-board", title: "" }));
  boardsEl.append(fresh);
  const manage = el("button", { class: "wb-banner__btn", type: "button", text: app.t("boards.manage") });
  manage.addEventListener("click", () => send({ t: "menu", item: "boards" }));
  boardsEl.append(manage);
  wb.append(boardsEl);
}

// --- Camera ------------------------------------------------------------------------------------------

function setCamera(cam, byUser) {
  app.cam = { x: cam.x, y: cam.y, zoom: clampZoom(cam.zoom) };
  // Looking around while following ends following (the presenter's next move would snap back).
  if (byUser && app.status.following && app.status.presenting !== app.me.author) {
    app.status.following = false;
    send({ t: "follow", on: false });
    chrome.updateBanner();
  }
  sceneDirty = true;
  overlayDirty = true;
  updateGrid();
  updateCursors();
  chrome.updateZoom();
  if (editing) placeEditor();
  if (app.status.presenting === app.me.author) sendView(false);
  requestFrame();
}

function sendView(force) {
  const c = viewCenter(app.cam, app.vw, app.vh);
  const msg = { t: "view", x: Math.round(c.x), y: Math.round(c.y), zoom: Math.round(c.zoom * 1000) / 1000 };
  const now = performance.now();
  clearTimeout(viewTimer);
  if (force || now - lastSent.view >= 100) {
    lastSent.view = now;
    send(msg, !force);
  }
  // The last position always goes out, so followers end up exactly where the presenter stopped.
  if (!force) viewTimer = setTimeout(() => sendView(true), 150);
}

function updateGrid() {
  let step = 24;
  while (step * app.cam.zoom < 12) step *= 2;
  const size = step * app.cam.zoom;
  wb.style.setProperty("--grid", `${size}px`);
  wb.style.setProperty("--gx", `${-app.cam.x * app.cam.zoom}px`);
  wb.style.setProperty("--gy", `${-app.cam.y * app.cam.zoom}px`);
}

function zoomStep(dir, at) {
  const steps = [0.1, 0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4];
  const z = app.cam.zoom;
  const next = dir > 0 ? (steps.find((s) => s > z + 0.001) ?? 4) : ([...steps].reverse().find((s) => s < z - 0.001) ?? 0.1);
  const p = at || { x: app.vw / 2, y: app.vh / 2 };
  setCamera(zoomAt(app.cam, next, p.x, p.y), true);
}

function fitBoard() {
  const b = boundsOf(app.store.list().filter((o) => !app.hidden.has(o.by)));
  setCamera(b ? fitRect(b, app.vw, app.vh, 64, 1) : cameraFromCenter(0, 0, 1, app.vw, app.vh), true);
}

function revealObject(o) {
  const r = objectBounds(o);
  const v = viewRect(app.cam, app.vw, app.vh);
  if (!pointInRect(v, r.x, r.y) || !pointInRect(v, r.x + r.w, r.y + r.h)) {
    setCamera(cameraFromCenter(r.x + r.w / 2, r.y + r.h / 2, app.cam.zoom, app.vw, app.vh), true);
  }
}

function resize() {
  const r = wb.getBoundingClientRect();
  const first = app.vw === 0;
  app.vw = Math.max(1, r.width);
  app.vh = Math.max(1, r.height);
  app.dpr = Math.min(3, window.devicePixelRatio || 1);
  for (const c of [scene, overlay]) {
    c.width = Math.round(app.vw * app.dpr);
    c.height = Math.round(app.vh * app.dpr);
  }
  if (first) app.cam = cameraFromCenter(app.vw / 2 - 80, app.vh / 2 - 60, 1, app.vw, app.vh);
  setCamera(app.cam, false);
}

// --- Drawing -------------------------------------------------------------------------------------------

function requestFrame() {
  if (frameQueued) return;
  frameQueued = true;
  requestAnimationFrame(frame);
}

function gestureOverrides() {
  const map = new Map(pending ? pending.override : []);
  if (gesture && gesture.type === "move" && (gesture.d.x !== 0 || gesture.d.y !== 0)) {
    for (const id of gesture.ids) {
      const o = app.store.get(id);
      if (o) map.set(id, translateGeom(o.g, gesture.d.x, gesture.d.y));
    }
  }
  if (gesture && gesture.type === "resize" && gesture.g) map.set(gesture.id, gesture.g);
  return map;
}

function frame(now) {
  frameQueued = false;
  if (!palette) palette = readPalette(document.documentElement);
  if (pending && now > pending.until) {
    pending = null;
    sceneDirty = true;
    overlayDirty = true;
  }
  const override = gestureOverrides();
  if (sceneDirty || override.size > 0) {
    sctx.setTransform(1, 0, 0, 1, 0, 0);
    sctx.clearRect(0, 0, scene.width, scene.height);
    const faded = new Set(pending ? pending.faded : []);
    if (gesture && gesture.type === "erase") for (const id of gesture.ids) faded.add(id);
    drawScene(sctx, app.store.list(), app.cam, app.dpr, palette, {
      hidden: app.hidden,
      skip: editing && editing.id ? new Set([editing.id]) : null,
      faded,
      override,
      peerOf,
    });
    sceneDirty = override.size > 0;
  }
  let animate = false;
  if (overlayDirty || live.size > 0 || lasers.size > 0) {
    animate = drawOverlay(now);
    overlayDirty = false;
  }
  if (animate) requestFrame();
}

function drawOverlay(now) {
  const reduced = theme.reducedMotion();
  octx.setTransform(1, 0, 0, 1, 0, 0);
  octx.clearRect(0, 0, overlay.width, overlay.height);
  const z = app.cam.zoom;
  octx.setTransform(app.dpr * z, 0, 0, app.dpr * z, -app.cam.x * z * app.dpr, -app.cam.y * z * app.dpr);
  let animate = false;

  // My own gesture in progress (and the moment until the plugin confirms it).
  const preview = [];
  if (pending) preview.push(...pending.objs);
  if (gesture && gesture.type === "stroke" && gesture.pts.length >= 2) {
    preview.push({ id: "~", k: gesture.kind, ...strokeObject(gesture.kind, gesture.pts, gesture.ink, gesture.w), by: app.me.author, text: "", alt: "", fill: 0, z: "" });
  }
  if (gesture && gesture.type === "shape") preview.push(shapeObject(gesture.kind, gesture.a, gesture.b, false));
  if (preview.length > 0) drawScene(octx, preview, app.cam, app.dpr, palette, { peerOf });
  octx.setTransform(app.dpr * z, 0, 0, app.dpr * z, -app.cam.x * z * app.dpr, -app.cam.y * z * app.dpr);

  // Other people's strokes while they draw.
  for (const [author, entry] of live) {
    if (entry.doneAt && now - entry.doneAt > LIVE_KEEP_MS) {
      live.delete(author);
      continue;
    }
    if (app.hidden.has(author)) continue;
    const peer = app.peers.get(author);
    const pulse = reduced ? 0 : (Math.sin(now / 220) + 1) / 2;
    drawLive(octx, entry.pts, personColour(peer ? peer.color : -1), 3.4, pulse);
    if (!reduced) animate = true;
  }

  // Selection, handles and the marquee, in screen-constant sizes.
  const accent = palette.accent;
  octx.lineWidth = 1.5 / z;
  octx.strokeStyle = accent;
  octx.setLineDash([5 / z, 4 / z]);
  const override = gestureOverrides();
  for (const id of app.selected) {
    const o = app.store.get(id);
    if (!o) continue;
    const g = override.get(id);
    const r = inflate(objectBounds(g ? { ...o, g } : o), 4 / z);
    octx.strokeRect(r.x, r.y, r.w, r.h);
  }
  octx.setLineDash([]);
  if (app.selected.size === 1 && app.tool === "select" && canEdit()) {
    const o = app.store.get([...app.selected][0]);
    if (o && o.k !== "stroke" && o.k !== "hl") {
      const g = override.get(o.id);
      for (const h of Object.values(handlesOf(g ? { ...o, g } : o))) {
        const s = 8 / z;
        octx.fillStyle = palette.paper;
        octx.fillRect(h.x - s / 2, h.y - s / 2, s, s);
        octx.strokeRect(h.x - s / 2, h.y - s / 2, s, s);
      }
    }
  }
  if (gesture && gesture.type === "marquee") {
    const r = rectFromPoints(gesture.a, gesture.b);
    octx.fillStyle = "rgb(127 127 255 / 12%)";
    octx.fillRect(r.x, r.y, r.w, r.h);
    octx.setLineDash([4 / z, 3 / z]);
    octx.strokeRect(r.x, r.y, r.w, r.h);
    octx.setLineDash([]);
  }

  // Laser trails: a fading comet tail, or only the dot with reduce motion.
  const seen = new Set();
  for (const [author, trail] of lasers) {
    pruneTrail(trail, now);
    if (trail.length === 0) {
      lasers.delete(author);
      continue;
    }
    seen.add(author);
    if (!reduced) {
      octx.lineCap = "round";
      octx.lineWidth = 7 / z;
      for (let i = 1; i < trail.length; i++) {
        const a = trailAlpha(now - trail[i].t, TRAIL_MS) * (i / trail.length);
        if (a <= 0) continue;
        octx.strokeStyle = `rgb(255 59 59 / ${Math.round(a * 95)}%)`;
        octx.beginPath();
        octx.moveTo(trail[i - 1].x, trail[i - 1].y);
        octx.lineTo(trail[i].x, trail[i].y);
        octx.stroke();
      }
    }
    placeLaserDot(author, trail[trail.length - 1]);
    animate = true;
  }
  for (const [author, node] of laserEls) {
    if (!seen.has(author)) {
      node.remove();
      laserEls.delete(author);
    }
  }
  return animate;
}

function placeLaserDot(author, p) {
  let node = laserEls.get(author);
  if (!node) {
    node = el("div", { class: "wb-laser" }, [el("span", { class: "wb-laser__halo" }), el("span", { class: "wb-laser__dot" })]);
    $("lasers").append(node);
    laserEls.set(author, node);
  }
  const s = worldToScreen(app.cam, p.x, p.y);
  node.style.left = `${s.x}px`;
  node.style.top = `${s.y}px`;
}

// --- Cursors -----------------------------------------------------------------------------------------

function cursorTag(peer) {
  return peer.status && app.t(`status.${peer.status}`) !== `status.${peer.status}` ? app.t(`status.${peer.status}`) : "";
}

function updateCursors() {
  const layer = $("cursors");
  const seen = new Set();
  for (const p of app.peers.values()) {
    if (p.author === app.me.author || !p.cursor) continue;
    seen.add(p.author);
    let c = cursorEls.get(p.author);
    if (!c) {
      c = cursorEl(false);
      cursorEls.set(p.author, c);
      layer.append(c.root);
    }
    const s = worldToScreen(app.cam, p.cursor[0], p.cursor[1]);
    updateCursor(c, p, s.x, s.y, cursorTag(p));
  }
  for (const [author, c] of cursorEls) {
    if (!seen.has(author)) {
      c.root.remove();
      cursorEls.delete(author);
    }
  }
}

function sendCursor(p, force) {
  const now = performance.now();
  if (!force && now - lastSent.cursor < SEND_EVERY_MS) return;
  lastSent.cursor = now;
  send({ t: "cursor", x: Math.round(p.x), y: Math.round(p.y), status: myStatus() }, true);
}

function myStatus() {
  if (editing) return "typing";
  if (gesture && gesture.type === "stroke") return gesture.kind === "hl" ? "highlighting" : "drawing";
  return "";
}

// --- Gestures (pointer and keyboard cursor share them) --------------------------------------------------

function tol() {
  return HIT_PX / app.cam.zoom;
}

function visibleObjs() {
  return app.store.list().filter((o) => !app.hidden.has(o.by));
}

function shapeObject(kind, a, b, final) {
  const tool = kind === "arrow" ? "arrow" : "shapes";
  const w = app.widths[tool];
  let g;
  if (kind === "line" || kind === "arrow") {
    g = [a.x, a.y, b.x - a.x, b.y - a.y];
    if (final && Math.hypot(g[2], g[3]) < 4) g = [a.x, a.y, app.size[tool][0], app.size[tool][1]];
  } else {
    const r = rectFromPoints(a, b);
    g = [r.x, r.y, r.w, r.h];
    if (final && r.w < 4 && r.h < 4) g = [a.x, a.y, app.size.shapes[0], app.size.shapes[1]];
  }
  return { id: "~", k: kind, g: g.map(q), ink: app.ink, w, fill: 0, text: "", alt: "", z: "", by: app.me.author };
}

function handleAt(p) {
  if (app.selected.size !== 1 || !canEdit()) return null;
  const o = app.store.get([...app.selected][0]);
  if (!o || o.k === "stroke" || o.k === "hl") return null;
  const s = 8 / app.cam.zoom;
  for (const [name, h] of Object.entries(handlesOf(o))) {
    if (Math.abs(p.x - h.x) <= s && Math.abs(p.y - h.y) <= s) return { id: o.id, handle: name };
  }
  return null;
}

function beginGesture(p, shift) {
  const tool = app.tool;
  if (!toolAllowed(app, tool)) return;
  switch (tool) {
    case "pen":
    case "hl":
      strokeSid = (strokeSid + 1) & 0xffff;
      gesture = { type: "stroke", kind: tool === "hl" ? "hl" : "stroke", pts: [q(p.x), q(p.y)], sent: 0, sid: strokeSid, lastLive: 0, ink: tool === "hl" ? app.hlInk : app.ink, w: app.widths[tool] };
      break;
    case "eraser":
      gesture = { type: "erase", ids: new Set(), last: p };
      eraseAt(p);
      break;
    case "shapes":
      gesture = { type: "shape", kind: app.shape, a: p, b: p };
      break;
    case "arrow":
      gesture = { type: "shape", kind: "arrow", a: p, b: p };
      break;
    case "note":
    case "text":
      gesture = { type: "tap", a: p };
      break;
    case "laser":
      gesture = { type: "laser", buf: [] };
      laserPoint(p);
      break;
    case "select": {
      const h = handleAt(p);
      if (h) {
        gesture = { type: "resize", id: h.id, handle: h.handle, g: null };
        break;
      }
      const hit = topHit(visibleObjs(), p.x, p.y, tol());
      if (hit) {
        if (shift) {
          const next = new Set(app.selected);
          if (next.has(hit.id)) next.delete(hit.id);
          else next.add(hit.id);
          setSelection(next);
          gesture = null;
        } else {
          const wasSelected = app.selected.has(hit.id);
          if (!wasSelected) setSelection(new Set([hit.id]));
          gesture = canEdit() ? { type: "move", start: p, d: { x: 0, y: 0 }, ids: [...app.selected], clickId: wasSelected ? hit.id : null } : null;
        }
      } else {
        gesture = { type: "marquee", a: p, b: p, base: shift ? new Set(app.selected) : new Set() };
        if (!shift) setSelection(new Set());
      }
      break;
    }
    default:
      break;
  }
  overlayDirty = true;
  requestFrame();
}

function moveGesture(p) {
  if (!gesture) return;
  switch (gesture.type) {
    case "stroke": {
      if (appendPoint(gesture.pts, q(p.x), q(p.y), 1 / app.cam.zoom)) {
        sendLive(false);
        // A very long stroke is committed in pieces so no single object passes the point limit.
        if (gesture.pts.length >= MAX_STROKE_POINTS * 2) {
          const last = gesture.pts.slice(-2);
          commitStroke();
          strokeSid = (strokeSid + 1) & 0xffff;
          gesture = { ...gesture, pts: last, sent: 0, sid: strokeSid };
        }
      }
      break;
    }
    case "erase": {
      const steps = Math.ceil(Math.hypot(p.x - gesture.last.x, p.y - gesture.last.y) / (tol() * 1.5)) || 1;
      for (let i = 1; i <= steps; i++) eraseAt({ x: gesture.last.x + ((p.x - gesture.last.x) * i) / steps, y: gesture.last.y + ((p.y - gesture.last.y) * i) / steps });
      gesture.last = p;
      sceneDirty = true;
      break;
    }
    case "shape":
      gesture.b = p;
      break;
    case "laser":
      laserPoint(p);
      break;
    case "move":
      gesture.d = { x: p.x - gesture.start.x, y: p.y - gesture.start.y };
      sceneDirty = true;
      break;
    case "resize": {
      const o = app.store.get(gesture.id);
      if (o) gesture.g = resizeGeom(o.k, o.g, gesture.handle, p.x, p.y);
      sceneDirty = true;
      break;
    }
    case "marquee": {
      gesture.b = p;
      const inside = objectsInRect(visibleObjs(), rectFromPoints(gesture.a, gesture.b)).map((o) => o.id);
      setSelection(new Set([...gesture.base, ...inside]));
      break;
    }
    default:
      break;
  }
  overlayDirty = true;
  requestFrame();
}

function endGesture(p) {
  const g = gesture;
  gesture = null;
  if (!g) return;
  switch (g.type) {
    case "stroke":
      gesture = g;
      commitStroke();
      gesture = null;
      break;
    case "erase":
      if (g.ids.size > 0) {
        send({ t: "delete", ids: [...g.ids] });
        setPending({ objs: [], override: new Map(), faded: new Set(g.ids), until: performance.now() + PENDING_MS });
      }
      break;
    case "shape": {
      const obj = shapeObject(g.kind, g.a, p, true);
      send({ t: "create", obj: stripLocal(obj) });
      setPending({ objs: [obj], override: new Map(), faded: new Set(), until: performance.now() + PENDING_MS });
      break;
    }
    case "tap":
      tapPlace(g.a);
      break;
    case "laser":
      flushLaser();
      break;
    case "move":
      if (g.d.x !== 0 || g.d.y !== 0) {
        const override = new Map();
        for (const id of g.ids) {
          const o = app.store.get(id);
          if (!o) continue;
          const ng = translateGeom(o.g, g.d.x, g.d.y).map(q);
          override.set(id, ng);
          send({ t: "set", id, fields: { g: ng } });
        }
        setPending({ objs: [], override, faded: new Set(), until: performance.now() + PENDING_MS });
      } else if (g.clickId) {
        setSelection(new Set([g.clickId]));
      }
      break;
    case "resize":
      if (g.g) {
        const ng = g.g.map(q);
        send({ t: "set", id: g.id, fields: { g: ng } });
        setPending({ objs: [], override: new Map([[g.id, ng]]), faded: new Set(), until: performance.now() + PENDING_MS });
      }
      break;
    default:
      break;
  }
  sendCursor(p, true);
  sceneDirty = true;
  overlayDirty = true;
  requestFrame();
}

function cancelGesture() {
  if (gesture && gesture.type === "stroke") send({ t: "live", sid: gesture.sid, pts: [], done: true }, true);
  gesture = null;
  kb.down = false;
  kbEl.removeAttribute("data-down");
  sceneDirty = true;
  overlayDirty = true;
  requestFrame();
}

/** Keeps a finished gesture on screen until `objs` confirms it (or briefly, if it never does). */
function setPending(p) {
  pending = p;
  setTimeout(requestFrame, PENDING_MS + 20);
}

/** The `create` payload of a shape, line or arrow: the plugin assigns id, author and z itself. */
function stripLocal(obj) {
  return { k: obj.k, g: obj.g, ink: obj.ink, w: obj.w };
}

function commitStroke() {
  const g = gesture;
  if (!g || g.pts.length < 2) return;
  sendLive(true);
  const obj = strokeObject(g.kind, g.pts, g.ink, g.w);
  send({ t: "create", obj });
  const shown = { id: "~", by: app.me.author, text: "", alt: "", fill: 0, z: "", ...obj };
  setPending({ objs: [...(pending ? pending.objs : []), shown], override: new Map(), faded: new Set(), until: performance.now() + PENDING_MS });
}

/** Sends the points added since the last live update. */
function sendLive(done) {
  const g = gesture;
  if (!g) return;
  const now = performance.now();
  if (!done && now - g.lastLive < SEND_EVERY_MS) return;
  const pts = g.pts.slice(g.sent);
  g.sent = g.pts.length;
  g.lastLive = now;
  send({ t: "live", sid: g.sid, pts, done }, !done);
}

function eraseAt(p) {
  const hit = topHit(visibleObjs().filter((o) => !gesture.ids.has(o.id)), p.x, p.y, tol());
  if (hit) gesture.ids.add(hit.id);
}

function laserPoint(p) {
  const me = app.me.author;
  if (!lasers.has(me)) lasers.set(me, []);
  addTrail(lasers.get(me), [p.x, p.y], performance.now());
  if (gesture && gesture.type === "laser") {
    gesture.buf.push(Math.round(p.x), Math.round(p.y));
    if (performance.now() - (gesture.lastSend || 0) >= SEND_EVERY_MS) flushLaser();
  }
  overlayDirty = true;
  requestFrame();
}

function flushLaser() {
  if (!gesture || gesture.type !== "laser" || gesture.buf.length === 0) return;
  send({ t: "laser", pts: gesture.buf }, true);
  gesture.buf = [];
  gesture.lastSend = performance.now();
}

function tapPlace(p) {
  if (app.tool === "text") {
    const hit = topHit(visibleObjs().filter((o) => o.k === "text"), p.x, p.y, tol());
    if (hit) {
      startEdit(hit);
      return;
    }
    startDraft("text", p);
  } else {
    startDraft("note", p);
  }
}

// --- Text editing ----------------------------------------------------------------------------------

function startDraft(kind, p) {
  const g = kind === "note" ? [q(p.x), q(p.y), app.size.note[0], app.size.note[1]] : [q(p.x), q(p.y), 200, textSize({ w: app.widths.text }) * 1.1];
  openEditor({ id: null, kind, g, ink: app.ink, w: app.widths.text, fill: app.fill, text: "" });
}

function startEdit(o) {
  if (!o || !canEdit() || (o.k !== "note" && o.k !== "text")) return;
  openEditor({ id: o.id, kind: o.k, g: [...o.g], ink: o.ink, w: o.w, fill: o.fill, text: o.text });
}

function openEditor(state) {
  if (editing) commitEdit();
  editing = state;
  editor.hidden = false;
  editor.value = state.text;
  editor.dataset.kind = state.kind;
  editor.maxLength = 500;
  editor.setAttribute("aria-label", app.t("editor.label", { kind: app.t(`kind.${state.kind}`) }));
  if (state.kind === "note") {
    editor.style.background = `var(${["--note-yellow", "--note-pink", "--note-mint", "--note-sky", "--note-lilac"][state.fill] ?? "--note-yellow"})`;
    editor.style.fontSize = "";
  } else {
    editor.style.background = "transparent";
    editor.style.setProperty("--ink", `var(--ink-${state.ink + 1})`);
    editor.style.fontSize = `${textSize(state)}px`;
  }
  placeEditor();
  sceneDirty = true;
  requestFrame();
  editor.focus();
  sendCursor({ x: state.g[0], y: state.g[1] }, true);
}

function placeEditor() {
  const g = editing.g;
  const s = worldToScreen(app.cam, g[0], g[1]);
  editor.style.left = `${s.x}px`;
  editor.style.top = `${s.y}px`;
  const tilt = editing.kind === "note" && editing.id ? noteTilt(editing.id) : 0;
  editor.style.transform = `scale(${app.cam.zoom}) rotate(${tilt}deg)`;
  if (editing.kind === "note") {
    editor.style.width = `${g[2]}px`;
    editor.style.height = `${g[3]}px`;
  } else {
    editor.style.width = `${Math.max(120, measureText(editor.value || " ", editing).w + 24)}px`;
    editor.style.height = `${Math.max(1, editor.value.split("\n").length) * textSize(editing) * 1.1 + 8}px`;
  }
}

function measureText(text, state) {
  const size = textSize(state);
  sctx.save();
  sctx.font = `700 ${size}px ${HAND_FAMILY}`;
  const lines = text.split("\n");
  const w = Math.max(...lines.map((l) => sctx.measureText(l).width), 1);
  sctx.restore();
  return { w: Math.ceil(w), h: Math.ceil(lines.length * size * 1.1) };
}

editor.addEventListener("input", () => {
  if (editing && editing.kind === "text") placeEditor();
});
editor.addEventListener("blur", () => {
  if (editing) commitEdit();
});

function commitEdit() {
  const e = editing;
  if (!e) return;
  editing = null;
  const text = editor.value.slice(0, 500);
  editor.hidden = true;
  const trimmed = text.trim();
  if (e.kind === "text") {
    const m = measureText(text, e);
    const g = [e.g[0], e.g[1], m.w, m.h].map(q);
    if (e.id && text !== e.text) send({ t: "set", id: e.id, fields: trimmed ? { text, g } : { text } });
    else if (!e.id && trimmed) send({ t: "create", obj: { k: "text", g, ink: e.ink, w: e.w, text } });
  } else if (e.id && text !== e.text) {
    send({ t: "set", id: e.id, fields: { text } });
  } else if (!e.id && trimmed) {
    send({ t: "create", obj: { k: "note", g: e.g, fill: e.fill, text } });
  }
  sendCursor({ x: e.g[0], y: e.g[1] }, true);
  sceneDirty = true;
  requestFrame();
}

function cancelEdit() {
  editing = null;
  editor.hidden = true;
  sceneDirty = true;
  requestFrame();
}

// --- Pointer input -------------------------------------------------------------------------------------

function localPoint(e) {
  const r = overlay.getBoundingClientRect();
  return { x: e.clientX - r.left, y: e.clientY - r.top };
}

function pinchState() {
  const [a, b] = [...pointers.values()];
  return { mid: { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }, dist: Math.hypot(a.x - b.x, a.y - b.y) || 1 };
}

overlay.addEventListener("pointerdown", (e) => {
  if (editing) commitEdit();
  wb.focus({ preventScroll: true });
  overlay.setPointerCapture(e.pointerId);
  const s = localPoint(e);
  pointers.set(e.pointerId, s);
  hideKb();
  if (pointers.size === 2) {
    // Two fingers: pinch to zoom and pan; the one-finger gesture that just started is dropped.
    cancelGesture();
    gesture = { type: "pinch", start: pinchState(), cam0: app.cam };
    return;
  }
  if (pointers.size > 2) return;
  if (e.button === 1 || (spaceHeld && app.status.presenting !== app.me.author)) {
    e.preventDefault();
    gesture = { type: "pan", last: s };
    wb.toggleAttribute("data-panning", true);
    return;
  }
  if (e.button !== 0) return;
  beginGesture(screenToWorld(app.cam, s.x, s.y), e.shiftKey);
});

overlay.addEventListener("pointermove", (e) => {
  const s = localPoint(e);
  if (pointers.has(e.pointerId)) pointers.set(e.pointerId, s);
  pointerInside = true;
  lastPointer = s;
  const p = screenToWorld(app.cam, s.x, s.y);
  if (gesture && gesture.type === "pinch") {
    if (pointers.size < 2) return;
    const now = pinchState();
    const z = gesture.cam0.zoom * (now.dist / gesture.start.dist);
    const zoomed = zoomAt(gesture.cam0, z, gesture.start.mid.x, gesture.start.mid.y);
    setCamera(panBy(zoomed, now.mid.x - gesture.start.mid.x, now.mid.y - gesture.start.mid.y), true);
    return;
  }
  if (gesture && gesture.type === "pan") {
    setCamera(panBy(app.cam, s.x - gesture.last.x, s.y - gesture.last.y), true);
    gesture.last = s;
    return;
  }
  if (kb.on && !kb.down) hideKb();
  // Presenter: holding Space points the laser wherever the mouse is, without changing tools.
  if (spaceHeld && app.status.presenting === app.me.author) {
    if (!gesture) gesture = { type: "laser", buf: [] };
    if (gesture.type === "laser") laserPoint(p);
  } else if (gesture) {
    for (const ce of typeof e.getCoalescedEvents === "function" ? e.getCoalescedEvents() : [e]) {
      const cs = localPoint(ce);
      moveGesture(screenToWorld(app.cam, cs.x, cs.y));
    }
  }
  sendCursor(p, false);
});

function pointerEnd(e) {
  pointers.delete(e.pointerId);
  if (!gesture) return;
  if (gesture.type === "pinch") {
    if (pointers.size < 2) gesture = null;
    return;
  }
  if (gesture.type === "pan") {
    gesture = null;
    wb.toggleAttribute("data-panning", spaceHeld);
    return;
  }
  if (e.type === "pointercancel") {
    cancelGesture();
    return;
  }
  const s = localPoint(e);
  endGesture(screenToWorld(app.cam, s.x, s.y));
}

overlay.addEventListener("pointerup", pointerEnd);
overlay.addEventListener("pointercancel", pointerEnd);
overlay.addEventListener("pointerleave", () => {
  pointerInside = false;
  if (!gesture) send({ t: "cursor", off: true }, true);
});
overlay.addEventListener("contextmenu", (e) => e.preventDefault());
overlay.addEventListener("dblclick", (e) => {
  const s = localPoint(e);
  const p = screenToWorld(app.cam, s.x, s.y);
  const hit = topHit(visibleObjs().filter((o) => o.k === "note" || o.k === "text"), p.x, p.y, tol());
  if (hit && (app.tool === "select" || app.tool === "text")) startEdit(hit);
});

overlay.addEventListener(
  "wheel",
  (e) => {
    e.preventDefault();
    const s = localPoint(e);
    const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? app.vh : 1;
    if (e.shiftKey && !e.ctrlKey) {
      setCamera(panBy(app.cam, -e.deltaY * unit, -e.deltaX * unit), true);
      return;
    }
    // Pinch on a touchpad arrives as a wheel with Ctrl and needs a stronger factor.
    const k = e.ctrlKey ? 0.01 : 0.0015;
    setCamera(zoomAt(app.cam, app.cam.zoom * Math.exp(-e.deltaY * unit * k), s.x, s.y), true);
  },
  { passive: false },
);

// --- Keyboard ------------------------------------------------------------------------------------------

function showKb() {
  if (!kb.on) {
    const c = lastPointer && pointerInside ? screenToWorld(app.cam, lastPointer.x, lastPointer.y) : viewCenter(app.cam, app.vw, app.vh);
    kb.x = c.x;
    kb.y = c.y;
    kb.on = true;
  }
  kbEl.hidden = false;
  placeKb();
}

function hideKb() {
  if (kb.down) return;
  kb.on = false;
  kbEl.hidden = true;
}

function placeKb() {
  const s = worldToScreen(app.cam, kb.x, kb.y);
  kbEl.style.left = `${s.x}px`;
  kbEl.style.top = `${s.y}px`;
}

function moveKb(key, shift) {
  showKb();
  const next = moveKbCursor(kb, key, shift, app.cam.zoom);
  kb.x = next.x;
  kb.y = next.y;
  const shiftBy = followShift(kb, viewRect(app.cam, app.vw, app.vh), 40 / app.cam.zoom);
  if (shiftBy.dx || shiftBy.dy) setCamera({ x: app.cam.x + shiftBy.dx, y: app.cam.y + shiftBy.dy, zoom: app.cam.zoom }, true);
  placeKb();
  if (kb.down) moveGesture({ x: kb.x, y: kb.y });
  sendCursor(kb, true);
}

function toggleKbPen() {
  showKb();
  if (!kb.down) {
    beginGesture({ x: kb.x, y: kb.y }, false);
    kb.down = gesture !== null;
  } else {
    kb.down = false;
    endGesture({ x: kb.x, y: kb.y });
  }
  kbEl.toggleAttribute("data-down", kb.down);
}

function placeAtKb() {
  showKb();
  const p = { x: kb.x, y: kb.y };
  if (app.tool === "shapes" || app.tool === "arrow") {
    const kind = app.tool === "arrow" ? "arrow" : app.shape;
    const [w, h] = app.size[app.tool];
    send({ t: "create", obj: stripLocal(shapeObject(kind, p, { x: p.x + w, y: p.y + h }, true)) });
  } else if (app.tool === "note" || app.tool === "text") {
    startDraft(app.tool, p);
  } else {
    const hit = topHit(visibleObjs(), p.x, p.y, tol());
    if (hit) {
      setSelection(new Set([hit.id]));
      if (app.tool === "select") startEdit(hit);
    }
  }
}

function runCommand(cmd) {
  const ids = [...app.selected];
  switch (cmd) {
    case "undo":
    case "redo":
      send({ t: cmd });
      break;
    case "dup":
    case "front":
    case "back":
      if (ids.length > 0 && canEdit()) send({ t: cmd, ids });
      break;
    case "selectAll":
      chooseTool("select", false);
      setSelection(new Set(visibleObjs().map((o) => o.id)));
      break;
    case "fit":
      fitBoard();
      break;
    case "zoomIn":
      zoomStep(1);
      break;
    case "zoomOut":
      zoomStep(-1);
      break;
    default:
      break;
  }
}

function onEscape() {
  if (gesture) cancelGesture();
  else if (app.status.presenting === app.me.author) send({ t: "present", on: false });
  else if (app.selected.size > 0) setSelection(new Set());
  else hideKb();
}

window.addEventListener("keydown", (e) => {
  if (chrome.helpOpen()) return;
  const target = e.target;
  if (editing) {
    if (e.key === "Escape" || (e.key === "Enter" && (e.ctrlKey || e.metaKey))) {
      e.preventDefault();
      commitEdit();
      wb.focus();
    }
    return;
  }
  if (e.key === "Escape") {
    onEscape();
    return;
  }
  if (isTypingTarget(target)) return;
  if (e.key === "F1" || e.key === "?") {
    e.preventDefault();
    chrome.openHelp();
    return;
  }
  const mod = e.ctrlKey || e.metaKey;
  const cmd = commandFor(e.key, mod, e.shiftKey);
  if (cmd) {
    e.preventDefault();
    runCommand(cmd);
    return;
  }
  if (mod || e.altKey) return;
  const onBoard = target === wb || target === document.body;
  if (target.closest && target.closest("#outline-tree")) return;
  if ((e.key === "Delete" || e.key === "Backspace") && app.selected.size > 0 && canEdit()) {
    e.preventDefault();
    send({ t: "delete", ids: [...app.selected] });
    return;
  }
  if (e.key === "+" || e.key === "=" || e.key === "-") {
    e.preventDefault();
    zoomStep(e.key === "-" ? -1 : 1, kb.on ? worldToScreen(app.cam, kb.x, kb.y) : null);
    return;
  }
  if (e.key === " " && onBoard) {
    e.preventDefault();
    if (e.repeat) return;
    if (kb.on) toggleKbPen();
    else {
      spaceHeld = true;
      wb.toggleAttribute("data-panning", app.status.presenting !== app.me.author);
    }
    return;
  }
  if (isArrowKey(e.key) && onBoard) {
    e.preventDefault();
    moveKb(e.key, e.shiftKey);
    return;
  }
  if (e.key === "Enter" && onBoard && kb.on) {
    e.preventDefault();
    placeAtKb();
    return;
  }
  const tool = toolForKey(e.key);
  if (tool && !e.repeat) {
    e.preventDefault();
    chooseTool(tool, true);
  }
});

window.addEventListener("keyup", (e) => {
  if (e.key !== " " || !spaceHeld) return;
  spaceHeld = false;
  wb.toggleAttribute("data-panning", false);
  if (gesture && gesture.type === "laser" && app.tool !== "laser") {
    flushLaser();
    gesture = null;
  }
});

window.addEventListener("blur", () => {
  spaceHeld = false;
  wb.toggleAttribute("data-panning", false);
});

// --- Start ----------------------------------------------------------------------------------------------

new ResizeObserver(resize).observe(wb);
loadHandFont(() => {
  sceneDirty = true;
  requestFrame();
});
chrome.build();
resize();
send({ t: "ready" });
