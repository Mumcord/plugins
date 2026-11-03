import { test } from "node:test";
import assert from "node:assert/strict";
import * as g from "../web/board/geom.js";

const cam = { x: 100, y: 50, zoom: 2 };

test("screen and world transforms are inverse", () => {
  const w = g.screenToWorld(cam, 40, 60);
  assert.deepEqual(w, { x: 120, y: 80 });
  assert.deepEqual(g.worldToScreen(cam, w.x, w.y), { x: 40, y: 60 });
});

test("zoomAt keeps the point under the pointer fixed and clamps", () => {
  const before = g.screenToWorld(cam, 300, 200);
  const next = g.zoomAt(cam, 3, 300, 200);
  const after = g.screenToWorld(next, 300, 200);
  assert.ok(Math.abs(before.x - after.x) < 1e-9 && Math.abs(before.y - after.y) < 1e-9);
  assert.equal(g.zoomAt(cam, 99, 0, 0).zoom, g.MAX_ZOOM);
  assert.equal(g.zoomAt(cam, 0.001, 0, 0).zoom, g.MIN_ZOOM);
  assert.equal(g.clampZoom(Number.NaN), 1);
});

test("view centre round-trips through cameraFromCenter", () => {
  const c = g.viewCenter(cam, 800, 600);
  assert.deepEqual(c, { x: 300, y: 200, zoom: 2 });
  assert.deepEqual(g.cameraFromCenter(c.x, c.y, c.zoom, 800, 600), cam);
});

test("panBy moves the camera opposite to the drag in world units", () => {
  assert.deepEqual(g.panBy(cam, 20, -10), { x: 90, y: 55, zoom: 2 });
});

test("fitRect centres content and never zooms in past maxZoom", () => {
  const c = g.fitRect({ x: 0, y: 0, w: 100, h: 100 }, 1000, 500, 50, 1);
  assert.equal(c.zoom, 1);
  assert.deepEqual(g.viewCenter(c, 1000, 500), { x: 50, y: 50, zoom: 1 });
  const big = g.fitRect({ x: 0, y: 0, w: 4000, h: 1000 }, 1000, 500, 0, 1);
  assert.equal(big.zoom, 0.25);
});

test("rect helpers", () => {
  assert.deepEqual(g.normRect(10, 10, -5, -4), { x: 5, y: 6, w: 5, h: 4 });
  assert.deepEqual(g.unionRect({ x: 0, y: 0, w: 1, h: 1 }, { x: 5, y: -2, w: 1, h: 1 }), { x: 0, y: -2, w: 6, h: 3 });
  assert.ok(g.rectsIntersect({ x: 0, y: 0, w: 10, h: 10 }, { x: 10, y: 10, w: 1, h: 1 }));
  assert.ok(!g.rectsIntersect({ x: 0, y: 0, w: 10, h: 10 }, { x: 11, y: 0, w: 1, h: 1 }));
  assert.equal(g.distToSegment(5, 5, 0, 0, 10, 0), 5);
  assert.equal(g.distToSegment(-3, 4, 0, 0, 10, 0), 5);
});

const stroke = { id: "1.1", k: "stroke", g: [100, 100, 20, 20], pts: [0, 0, 10, 10], bw: 10, bh: 10, w: 1 };

test("stroke points scale from the original bbox into the geometry", () => {
  assert.deepEqual(g.strokePoints(stroke), [100, 100, 120, 120]);
});

test("hit testing per kind", () => {
  assert.ok(g.hitObject(stroke, 110, 110, 1));
  assert.ok(!g.hitObject(stroke, 120, 100, 1));
  const ell = { k: "ellipse", g: [0, 0, 100, 50], w: 1 };
  assert.ok(g.hitObject(ell, 50, 25, 0));
  assert.ok(!g.hitObject(ell, 2, 2, 0));
  const dia = { k: "diamond", g: [0, 0, 100, 100], w: 1 };
  assert.ok(g.hitObject(dia, 50, 50, 0));
  assert.ok(!g.hitObject(dia, 5, 5, 0));
  const arrow = { k: "arrow", g: [0, 0, -100, 0], w: 1 };
  assert.ok(g.hitObject(arrow, -50, 2, 1));
  const note = { k: "note", g: [0, 0, 156, 112], w: 0 };
  assert.ok(g.hitObject(note, 80, 60, 0));
});

test("topHit returns the frontmost object", () => {
  const a = { id: "a", k: "rect", g: [0, 0, 100, 100], w: 1 };
  const b = { id: "b", k: "rect", g: [50, 50, 100, 100], w: 1 };
  assert.equal(g.topHit([a, b], 75, 75, 0).id, "b");
  assert.equal(g.topHit([a, b], 10, 10, 0).id, "a");
  assert.equal(g.topHit([a, b], 500, 500, 0), null);
});

test("marquee selects only objects fully inside", () => {
  const a = { id: "a", k: "note", g: [0, 0, 10, 10] };
  const b = { id: "b", k: "note", g: [5, 5, 100, 100] };
  assert.deepEqual(g.objectsInRect([a, b], { x: -1, y: -1, w: 20, h: 20 }).map((o) => o.id), ["a"]);
});

test("resizeGeom handles corners, flips and line ends", () => {
  assert.deepEqual(g.resizeGeom("rect", [0, 0, 100, 100], "se", 150, 50), [0, 0, 150, 50]);
  assert.deepEqual(g.resizeGeom("rect", [0, 0, 100, 100], "nw", 150, 150), [100, 100, 50, 50]);
  assert.deepEqual(g.resizeGeom("line", [0, 0, 10, 10], "a", 5, 0), [5, 0, 5, 10]);
  assert.deepEqual(g.resizeGeom("arrow", [0, 0, 10, 10], "b", -5, 3), [0, 0, -5, 3]);
});

test("regionOf names nine regions", () => {
  const board = { x: 0, y: 0, w: 300, h: 300 };
  assert.equal(g.regionOf({ x: 0, y: 0, w: 10, h: 10 }, board), "tl");
  assert.equal(g.regionOf({ x: 140, y: 140, w: 10, h: 10 }, board), "c");
  assert.equal(g.regionOf({ x: 290, y: 290, w: 10, h: 10 }, board), "br");
  assert.equal(g.regionOf({ x: 140, y: 0, w: 10, h: 10 }, board), "t");
  assert.equal(g.regionOf({ x: 0, y: 0, w: 1, h: 1 }, null), "c");
});

test("reading order goes by rows, then left to right", () => {
  const objs = [
    { id: "c", k: "note", g: [0, 300, 10, 10] },
    { id: "b", k: "note", g: [200, 10, 10, 10] },
    { id: "a", k: "note", g: [0, 20, 10, 10] },
  ];
  assert.deepEqual(g.readingOrder(objs).map((o) => o.id), ["a", "b", "c"]);
});

test("appendPoint drops jitter", () => {
  const pts = [0, 0];
  assert.equal(g.appendPoint(pts, 0.5, 0, 1), false);
  assert.equal(g.appendPoint(pts, 2, 0, 1), true);
  assert.deepEqual(pts, [0, 0, 2, 0]);
});

test("strokeObject makes points local to a quarter-pixel bbox", () => {
  const o = g.strokeObject("stroke", [10.1, 20, 30, 25.3], 4, 1);
  assert.deepEqual(o.g, [10, 20, 20, 5.25]);
  assert.deepEqual(o.pts, [0, 0, 20, 5.25]);
  assert.equal(o.bw, 20);
  assert.equal(o.ink, 4);
  assert.deepEqual(g.strokePoints({ ...o, k: "stroke" }), [10, 20, 30, 25.25]);
});

test("handles and quick note placement", () => {
  assert.deepEqual(Object.keys(g.handlesOf({ k: "rect", g: [0, 0, 1, 1] })), ["nw", "ne", "sw", "se"]);
  assert.deepEqual(Object.keys(g.handlesOf({ k: "line", g: [0, 0, 1, 1] })), ["a", "b"]);
  assert.deepEqual(g.quickNotePlace(null), { x: 0, y: 0 });
  assert.deepEqual(g.quickNotePlace({ x: 10, y: 20, w: 100, h: 50 }), { x: 142, y: 20 });
});
