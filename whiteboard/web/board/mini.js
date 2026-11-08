// Side panel mini view: a read-only, fit-to-content picture of the board with the others'
// cursors, "Open on stage", a quick sticky note and the list of who is doing what.

import { connect } from "./bridge.js";
import { makeT } from "./strings.js";
import { createTheme } from "./theme.js";
import { el, cursorEl, updateCursor, shapeGlyph } from "./dom.js";
import { ObjectStore, normalizePeers, personColour, peerStateKey } from "./objects.js";
import { readPalette, drawScene, loadHandFont } from "./render.js";
import { answerRender } from "./pngrender.js";
import { boundsOf, inflate, fitRect, worldToScreen, viewRect, quickNotePlace } from "./geom.js";

const $ = (id) => document.getElementById(id);
const box = $("mini");
const canvas = $("mini-canvas");
const ctx = canvas.getContext("2d");
const quick = $("quick");

let t = makeT({});
let me = { author: -1 };
let title = "";
let palette = null;
let cam = { x: 0, y: 0, zoom: 1 };
let size = { w: 1, h: 1, dpr: 1 };
let peers = new Map();
let queued = false;
const store = new ObjectStore();
const cursorEls = new Map();

const bridge = connect(onMessage, 16 * 1024, "side");
const send = (msg) => bridge.send(msg, false);
const theme = createTheme(() => {
  palette = readPalette(document.documentElement);
  redraw();
});

function hiddenAuthors() {
  return new Set([...peers.values()].filter((p) => p.hidden).map((p) => p.author));
}

function visible() {
  const hidden = hiddenAuthors();
  return store.list().filter((o) => !hidden.has(o.by));
}

function peerOf(author) {
  return author === me.author ? me : peers.get(author) || null;
}

function onMessage(msg) {
  switch (msg.t) {
    case "init":
      t = makeT(msg.strings);
      me = { author: Number.isInteger(msg.me?.author) ? msg.me.author : -1, shape: msg.me?.shape | 0, color: msg.me?.color | 0, name: String(msg.me?.name ?? "").slice(0, 64) };
      title = typeof msg.board?.title === "string" ? msg.board.title.slice(0, 64) : "";
      theme.set(msg);
      texts();
      redraw();
      break;
    case "objs":
      store.apply(msg);
      redraw();
      break;
    case "peers": {
      peers = new Map(normalizePeers(msg.list).map((p) => [p.author, p]));
      people();
      redraw();
      break;
    }
    case "cursor": {
      const p = peers.get(msg.author);
      if (!p) break;
      p.cursor = Number.isFinite(msg.x) && Number.isFinite(msg.y) ? [msg.x, msg.y] : null;
      p.status = typeof msg.status === "string" ? msg.status.slice(0, 20) : "";
      cursors();
      people();
      break;
    }
    case "status":
      if (typeof msg.paper === "string") theme.set({ paper: msg.paper });
      break;
    case "announce":
      if (typeof msg.text === "string") $("announce").textContent = msg.text.slice(0, 300);
      break;
    case "render":
      answerRender(msg, {
        objs: store.list(),
        selected: new Set(),
        view: viewRect(cam, size.w, size.h),
        palette,
        peerOf,
        hidden: hiddenAuthors(),
        send,
      });
      break;
    default:
      break;
  }
}

function texts() {
  $("live-text").textContent = t("mini.live");
  $("open").textContent = t("mini.open");
  $("quick-label").textContent = t("mini.quick");
  $("quick-add").textContent = t("mini.add");
  $("quick-hint").textContent = t("mini.hint");
  people();
}

function describe() {
  const busy = [...peers.values()].filter((p) => p.author !== me.author && p.status).map((p) => `${p.name}: ${t(`state.${peerStateKey(p.status)}`)}`);
  box.setAttribute("aria-label", [t("mini.label", { title }), ...busy].join(" "));
}

function people() {
  const list = [...peers.values()];
  $("people-title").textContent = t("mini.onboard", { n: list.length });
  $("people").replaceChildren(
    ...list.map((p) => {
      const c = personColour(p.color);
      const state = t(`state.${peerStateKey(p.status)}`);
      return el("li", { class: "wb-prow" }, [
        el("span", { class: "wb-avatar", style: `--c:${c}`, "aria-hidden": "true", text: [...p.name][0]?.toUpperCase() ?? "?" }),
        el("span", { class: "wb-prow__name", text: p.name }),
        shapeGlyph(p.shape, 12, c),
        el("span", { class: "wb-prow__state", text: p.author === me.author ? t("state.you", { state }) : state }),
      ]);
    }),
  );
  describe();
}

function cursors() {
  const layer = $("cursors");
  const seen = new Set();
  for (const p of peers.values()) {
    if (p.author === me.author || !p.cursor) continue;
    seen.add(p.author);
    let c = cursorEls.get(p.author);
    if (!c) {
      c = cursorEl(true);
      cursorEls.set(p.author, c);
      layer.append(c.root);
    }
    const s = worldToScreen(cam, p.cursor[0], p.cursor[1]);
    updateCursor(c, p, s.x, s.y, "");
  }
  for (const [author, c] of cursorEls) {
    if (!seen.has(author)) {
      c.root.remove();
      cursorEls.delete(author);
    }
  }
}

function redraw() {
  if (queued) return;
  queued = true;
  requestAnimationFrame(() => {
    queued = false;
    if (!palette) palette = readPalette(document.documentElement);
    const objs = visible();
    const b = boundsOf(objs);
    cam = b ? fitRect(inflate(b, 16), size.w, size.h, 8, 1) : { x: 0, y: 0, zoom: 0.5 };
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    drawScene(ctx, objs, cam, size.dpr, palette, { peerOf });
    cursors();
  });
}

function resize() {
  const r = box.getBoundingClientRect();
  size = { w: Math.max(1, r.width), h: Math.max(1, r.height), dpr: Math.min(3, window.devicePixelRatio || 1) };
  canvas.width = Math.round(size.w * size.dpr);
  canvas.height = Math.round(size.h * size.dpr);
  redraw();
}

function addQuickNote() {
  const text = quick.value.trim().slice(0, 500);
  if (!text) return;
  const p = quickNotePlace(boundsOf(visible()));
  send({ t: "create", obj: { k: "note", g: [p.x, p.y, 156, 112], fill: 0, text } });
  quick.value = "";
}

$("open").addEventListener("click", () => send({ t: "open-stage" }));
$("quick-add").addEventListener("click", addQuickNote);
quick.addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    addQuickNote();
  }
});

new ResizeObserver(resize).observe(box);
loadHandFont(redraw);
texts();
resize();
send({ t: "ready" });
