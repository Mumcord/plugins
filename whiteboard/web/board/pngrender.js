// Answers the plugin's `render` requests: draws the board, the current view or the selection to
// an offscreen canvas and sends the PNG back in base64 chunks.

import { boundsOf, inflate } from "./geom.js";
import { pngMessages, renderScale } from "./chunk.js";
import { drawScene } from "./render.js";

const PAD = 24;
const EMPTY = { x: 0, y: 0, w: 320, h: 200 };

function makeCanvas(w, h) {
  if (typeof OffscreenCanvas === "function") return new OffscreenCanvas(w, h);
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  return c;
}

async function toPngBytes(canvas) {
  const blob =
    typeof canvas.convertToBlob === "function"
      ? await canvas.convertToBlob({ type: "image/png" })
      : await new Promise((resolve, reject) => canvas.toBlob((b) => (b ? resolve(b) : reject(new Error("toBlob failed"))), "image/png"));
  return new Uint8Array(await blob.arrayBuffer());
}

/** The objects and the world rectangle of an export area: the board, the view or the selection. */
function sceneArea(scope, ctx) {
  const visible = ctx.objs.filter((o) => !ctx.hidden.has(o.by));
  if (scope === "view") return { objs: visible, rect: ctx.view };
  if (scope === "selection" && ctx.selected.size > 0) {
    const objs = visible.filter((o) => ctx.selected.has(o.id));
    return { objs, rect: inflate(boundsOf(objs), PAD) };
  }
  const b = boundsOf(visible);
  return { objs: visible, rect: b ? inflate(b, PAD) : EMPTY };
}

/** Draws the export dialog's preview of `scope` into `canvas`, centred and fitted. */
export function drawPreview(canvas, scope, transparent, ctx) {
  const { objs, rect } = sceneArea(scope, ctx);
  const g = canvas.getContext("2d");
  g.clearRect(0, 0, canvas.width, canvas.height);
  const s = Math.min(canvas.width / rect.w, canvas.height / rect.h);
  const x = rect.x - (canvas.width / s - rect.w) / 2;
  const y = rect.y - (canvas.height / s - rect.h) / 2;
  if (!transparent) {
    g.fillStyle = ctx.palette.paper;
    g.fillRect((rect.x - x) * s, (rect.y - y) * s, rect.w * s, rect.h * s);
  }
  drawScene(g, objs, { x, y, zoom: s }, 1, ctx.palette, { peerOf: ctx.peerOf });
  return { w: Math.round(rect.w), h: Math.round(rect.h) };
}

/**
 * `ctx` gives the current state: `objs` (paint order), `selected` (Set of ids), `view` (world
 * rect on screen), `palette`, `peerOf`, `hidden` (authors hidden locally), `send`.
 */
export async function answerRender(msg, ctx) {
  const req = msg.req;
  if (!(typeof req === "string" && req.length <= 64) && !Number.isInteger(req)) return;
  const scale = [1, 2, 4].includes(msg.scale) ? msg.scale : 1;
  const { objs, rect } = sceneArea(msg.scope, ctx);
  try {
    const s = renderScale(rect.w, rect.h, scale);
    const canvas = makeCanvas(Math.max(1, Math.ceil(rect.w * s)), Math.max(1, Math.ceil(rect.h * s)));
    const g = canvas.getContext("2d");
    if (msg.transparent !== true) {
      g.fillStyle = ctx.palette.paper;
      g.fillRect(0, 0, canvas.width, canvas.height);
    }
    drawScene(g, objs, { x: rect.x, y: rect.y, zoom: s }, 1, ctx.palette, { peerOf: ctx.peerOf });
    for (const part of pngMessages(req, await toPngBytes(canvas))) ctx.send(part);
  } catch (err) {
    console.warn("whiteboard: rendering the PNG failed", err.name);
    // An empty answer (of: 0) tells the plugin the render failed instead of leaving it waiting.
    ctx.send({ t: "png", req, part: 0, of: 0, data: "" });
  }
}
