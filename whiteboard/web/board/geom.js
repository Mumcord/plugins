// Pure geometry for the board: camera transforms, rectangles, hit testing and object bounds.
// No DOM access here so `node --test` can cover it.

export const MIN_ZOOM = 0.1;
export const MAX_ZOOM = 4;

/** Stroke widths in px for the three thickness steps (pen, shapes, arrows, lines). */
export const WIDTHS = [2, 4, 7];
/** The highlighter is a broad marker, so its three steps are wider. */
export const HL_WIDTHS = [12, 22, 32];
/** Text sizes in px for the three thickness steps of the text tool. */
export const TEXT_SIZES = [22, 30, 40];
export const NOTE_SIZE = { w: 156, h: 112 };

export function clamp(v, lo, hi) {
  return Math.min(hi, Math.max(lo, v));
}

export function clampZoom(z) {
  return clamp(Number.isFinite(z) ? z : 1, MIN_ZOOM, MAX_ZOOM);
}

// A camera is { x, y, zoom } where (x, y) is the world point shown at the top-left screen corner.

export function screenToWorld(cam, sx, sy) {
  return { x: cam.x + sx / cam.zoom, y: cam.y + sy / cam.zoom };
}

export function worldToScreen(cam, wx, wy) {
  return { x: (wx - cam.x) * cam.zoom, y: (wy - cam.y) * cam.zoom };
}

/** Zooms to `zoom` keeping the world point under screen point (sx, sy) fixed. */
export function zoomAt(cam, zoom, sx, sy) {
  const z = clampZoom(zoom);
  const p = screenToWorld(cam, sx, sy);
  return { x: p.x - sx / z, y: p.y - sy / z, zoom: z };
}

export function panBy(cam, dsx, dsy) {
  return { x: cam.x - dsx / cam.zoom, y: cam.y - dsy / cam.zoom, zoom: cam.zoom };
}

/** The world rectangle a viewport of `w`×`h` screen px shows. */
export function viewRect(cam, w, h) {
  return { x: cam.x, y: cam.y, w: w / cam.zoom, h: h / cam.zoom };
}

/** Centre of the view: what `view` messages carry, so viewers with other window sizes see the same middle. */
export function viewCenter(cam, w, h) {
  return { x: cam.x + w / (2 * cam.zoom), y: cam.y + h / (2 * cam.zoom), zoom: cam.zoom };
}

export function cameraFromCenter(cx, cy, zoom, w, h) {
  const z = clampZoom(zoom);
  return { x: cx - w / (2 * z), y: cy - h / (2 * z), zoom: z };
}

/** Camera that fits `rect` into a `w`×`h` viewport with `pad` screen px around it. */
export function fitRect(rect, w, h, pad = 48, maxZoom = 1) {
  if (!rect || rect.w <= 0 || rect.h <= 0 || w <= 0 || h <= 0) {
    return { x: (rect ? rect.x : 0) - w / 2, y: (rect ? rect.y : 0) - h / 2, zoom: 1 };
  }
  const z = clamp(Math.min((w - 2 * pad) / rect.w, (h - 2 * pad) / rect.h, maxZoom), MIN_ZOOM, MAX_ZOOM);
  return cameraFromCenter(rect.x + rect.w / 2, rect.y + rect.h / 2, z, w, h);
}

/** Rectangle with non-negative size (lines and arrows store a direction in a signed w/h). */
export function normRect(x, y, w, h) {
  return { x: Math.min(x, x + w), y: Math.min(y, y + h), w: Math.abs(w), h: Math.abs(h) };
}

export function rectFromPoints(a, b) {
  return normRect(a.x, a.y, b.x - a.x, b.y - a.y);
}

export function unionRect(a, b) {
  if (!a) return b;
  if (!b) return a;
  const x = Math.min(a.x, b.x);
  const y = Math.min(a.y, b.y);
  return { x, y, w: Math.max(a.x + a.w, b.x + b.w) - x, h: Math.max(a.y + a.h, b.y + b.h) - y };
}

export function inflate(r, d) {
  return { x: r.x - d, y: r.y - d, w: r.w + 2 * d, h: r.h + 2 * d };
}

export function rectsIntersect(a, b) {
  return a.x <= b.x + b.w && b.x <= a.x + a.w && a.y <= b.y + b.h && b.y <= a.y + a.h;
}

export function rectContainsRect(outer, inner) {
  return inner.x >= outer.x && inner.y >= outer.y && inner.x + inner.w <= outer.x + outer.w && inner.y + inner.h <= outer.y + outer.h;
}

export function pointInRect(r, x, y) {
  return x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h;
}

export function distToSegment(px, py, ax, ay, bx, by) {
  const dx = bx - ax;
  const dy = by - ay;
  const len2 = dx * dx + dy * dy;
  const t = len2 === 0 ? 0 : clamp(((px - ax) * dx + (py - ay) * dy) / len2, 0, 1);
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy));
}

export function strokeWidthOf(obj) {
  const w = clamp(obj.w | 0, 0, 2);
  return obj.k === "hl" ? HL_WIDTHS[w] : WIDTHS[w];
}

/**
 * World points of a stroke or highlight as a flat array. Points are stored local to the original
 * bounding box (bw × bh) and scaled into the current geometry, so a resize needs no new points.
 */
export function strokePoints(obj) {
  const pts = obj.pts || [];
  const [gx, gy, gw, gh] = obj.g;
  const sx = obj.bw > 0 ? gw / obj.bw : 1;
  const sy = obj.bh > 0 ? gh / obj.bh : 1;
  const out = new Array(pts.length);
  for (let i = 0; i + 1 < pts.length; i += 2) {
    out[i] = gx + pts[i] * sx;
    out[i + 1] = gy + pts[i + 1] * sy;
  }
  return out;
}

/** Bounds of an object in world px, including half the stroke width. */
export function objectBounds(obj) {
  const [x, y, w, h] = obj.g;
  const r = normRect(x, y, w, h);
  if (obj.k === "note" || obj.k === "text") return r;
  return inflate(r, strokeWidthOf(obj) / 2);
}

/** Is world point (px, py) on the object, with `tol` world px of slack? */
export function hitObject(obj, px, py, tol) {
  const [x, y, w, h] = obj.g;
  const half = strokeWidthOf(obj) / 2 + tol;
  switch (obj.k) {
    case "stroke":
    case "hl": {
      if (!pointInRect(inflate(objectBounds(obj), tol), px, py)) return false;
      const p = strokePoints(obj);
      if (p.length === 2) return Math.hypot(px - p[0], py - p[1]) <= half;
      for (let i = 0; i + 3 < p.length; i += 2) {
        if (distToSegment(px, py, p[i], p[i + 1], p[i + 2], p[i + 3]) <= half) return true;
      }
      return false;
    }
    case "line":
    case "arrow":
      return distToSegment(px, py, x, y, x + w, y + h) <= half;
    case "ellipse": {
      const r = normRect(x, y, w, h);
      const rx = r.w / 2;
      const ry = r.h / 2;
      if (rx <= 0 || ry <= 0) return false;
      const nx = (px - (r.x + rx)) / (rx + tol);
      const ny = (py - (r.y + ry)) / (ry + tol);
      return nx * nx + ny * ny <= 1;
    }
    case "diamond": {
      const r = normRect(x, y, w, h);
      const rx = r.w / 2 + tol;
      const ry = r.h / 2 + tol;
      if (rx <= 0 || ry <= 0) return false;
      return Math.abs(px - (r.x + r.w / 2)) / rx + Math.abs(py - (r.y + r.h / 2)) / ry <= 1;
    }
    default:
      // Notes, text and rectangles are solid boxes: clicking inside picks them.
      return pointInRect(inflate(normRect(x, y, w, h), tol), px, py);
  }
}

/** Topmost object at a point; `objs` is in paint order (back to front). */
export function topHit(objs, px, py, tol) {
  for (let i = objs.length - 1; i >= 0; i--) {
    if (hitObject(objs[i], px, py, tol)) return objs[i];
  }
  return null;
}

/** Objects fully inside a marquee rectangle. */
export function objectsInRect(objs, rect) {
  return objs.filter((o) => rectContainsRect(rect, objectBounds(o)));
}

export function boundsOf(objs) {
  let r = null;
  for (const o of objs) r = unionRect(r, objectBounds(o));
  return r;
}

export function translateGeom(g, dx, dy) {
  return [g[0] + dx, g[1] + dy, g[2], g[3]];
}

/**
 * New geometry after dragging resize handle `handle` ("nw", "ne", "sw", "se", or "a"/"b" for the
 * two ends of a line) to world point (px, py).
 */
export function resizeGeom(kind, g, handle, px, py) {
  const [x, y, w, h] = g;
  if (kind === "line" || kind === "arrow") {
    if (handle === "a") return [px, py, x + w - px, y + h - py];
    return [x, y, px - x, py - y];
  }
  const r = normRect(x, y, w, h);
  let left = r.x;
  let top = r.y;
  let right = r.x + r.w;
  let bottom = r.y + r.h;
  if (handle.includes("w")) left = px;
  if (handle.includes("e")) right = px;
  if (handle.includes("n")) top = py;
  if (handle.includes("s")) bottom = py;
  const n = normRect(left, top, right - left, bottom - top);
  return [n.x, n.y, Math.max(n.w, 1), Math.max(n.h, 1)];
}

/** Screen positions of the resize handles of one object (keys as in `resizeGeom`). */
export function handlesOf(obj) {
  const [x, y, w, h] = obj.g;
  if (obj.k === "line" || obj.k === "arrow") return { a: { x, y }, b: { x: x + w, y: y + h } };
  const r = normRect(x, y, w, h);
  return {
    nw: { x: r.x, y: r.y },
    ne: { x: r.x + r.w, y: r.y },
    sw: { x: r.x, y: r.y + r.h },
    se: { x: r.x + r.w, y: r.y + r.h },
  };
}

/** Rough place of `rect` within `board` as one of nine regions, for spoken descriptions. */
export function regionOf(rect, board) {
  if (!board || board.w <= 0 || board.h <= 0) return "c";
  const cx = (rect.x + rect.w / 2 - board.x) / board.w;
  const cy = (rect.y + rect.h / 2 - board.y) / board.h;
  const col = cx < 1 / 3 ? "l" : cx > 2 / 3 ? "r" : "";
  const row = cy < 1 / 3 ? "t" : cy > 2 / 3 ? "b" : "";
  return row + col || "c";
}

/**
 * Reading order for the outline: top to bottom in bands, left to right inside a band, so
 * objects that sit side by side are read as a row.
 */
export function readingOrder(objs, band = 80) {
  return objs
    .map((o) => ({ o, r: objectBounds(o) }))
    .sort((a, b) => Math.floor(a.r.y / band) - Math.floor(b.r.y / band) || a.r.x - b.r.x || a.r.y - b.r.y)
    .map((e) => e.o);
}

/**
 * Drops points closer than `minDist` to the previous kept point. The plugin simplifies strokes
 * properly before sending; this only keeps pointer jitter from filling the 2,000 point budget.
 */
export function appendPoint(flat, x, y, minDist) {
  const n = flat.length;
  if (n >= 2 && Math.hypot(x - flat[n - 2], y - flat[n - 1]) < minDist) return false;
  flat.push(x, y);
  return true;
}

/** Rounds to quarter pixels, the precision the wire format keeps anyway. */
export function q(v) {
  return Math.round(v * 4) / 4;
}

/**
 * Builds the `create` object for a finished stroke from flat world points: geometry is the
 * bounding box and the points are made local to it.
 */
export function strokeObject(kind, flat, ink, width) {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (let i = 0; i + 1 < flat.length; i += 2) {
    minX = Math.min(minX, flat[i]);
    maxX = Math.max(maxX, flat[i]);
    minY = Math.min(minY, flat[i + 1]);
    maxY = Math.max(maxY, flat[i + 1]);
  }
  const ox = q(minX);
  const oy = q(minY);
  const pts = flat.map((v, i) => q(v - (i % 2 === 0 ? ox : oy)));
  const bw = q(maxX - ox);
  const bh = q(maxY - oy);
  return { k: kind, g: [ox, oy, bw, bh], pts, bw, bh, ink, w: width };
}

/**
 * Where the side panel's quick sticky note goes: to the right of everything on the board, so it
 * never covers anyone's work. An empty board gets it at the origin.
 */
export function quickNotePlace(bounds, gap = 32) {
  if (!bounds) return { x: 0, y: 0 };
  return { x: q(bounds.x + bounds.w + gap), y: q(bounds.y) };
}
