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

/**
 * `ctx` gives the current state: `objs` (paint order), `selected` (Set of ids), `view` (world
 * rect on screen), `palette`, `peerOf`, `hidden` (authors hidden locally), `send`, `chunk`.
 */
export async function answerRender(msg, ctx) {
  const req = msg.req;
  if (!(typeof req === "string" && req.length <= 64) && !Number.isInteger(req)) return;
  const scale = [1, 2, 4].includes(msg.scale) ? msg.scale : 1;
  const visible = ctx.objs.filter((o) => !ctx.hidden.has(o.by));
  let objs = visible;
  let rect;
  if (msg.scope === "view") {
    rect = ctx.view;
  } else if (msg.scope === "selection" && ctx.selected.size > 0) {
    objs = visible.filter((o) => ctx.selected.has(o.id));
    rect = inflate(boundsOf(objs), PAD);
  } else {
    const b = boundsOf(visible);
    rect = b ? inflate(b, PAD) : EMPTY;
  }
  try {
    const s = renderScale(rect.w, rect.h, scale);
    const canvas = makeCanvas(Math.max(1, Math.ceil(rect.w * s)), Math.max(1, Math.ceil(rect.h * s)));
    const g = canvas.getContext("2d");
    if (msg.transparent !== true) {
      g.fillStyle = ctx.palette.paper;
      g.fillRect(0, 0, canvas.width, canvas.height);
    }
    drawScene(g, objs, { x: rect.x, y: rect.y, zoom: s }, 1, ctx.palette, { peerOf: ctx.peerOf });
    for (const part of pngMessages(req, await toPngBytes(canvas), ctx.chunk)) ctx.send(part);
  } catch (err) {
    console.warn("whiteboard: rendering the PNG failed", err.name);
    // An empty answer (of: 0) tells the plugin the render failed instead of leaving it waiting.
    ctx.send({ t: "png", req, part: 0, of: 0, data: "" });
  }
}
