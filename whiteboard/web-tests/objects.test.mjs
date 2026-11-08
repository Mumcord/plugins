import { test } from "node:test";
import assert from "node:assert/strict";
import { normalizeObj, ObjectStore, compareZ, describeObj, outlineEntries, normalizePeers, personColour, peerStateKey } from "../web/board/objects.js";
import { makeT } from "../web/board/strings.js";

const note = { id: "1.1", k: "note", g: [0, 0, 156, 112], fill: 0, text: "Docs: update", z: "m", by: 1 };

test("valid objects pass, malformed ones are dropped", () => {
  assert.equal(normalizeObj(note).text, "Docs: update");
  assert.equal(normalizeObj({ ...note, k: "image" }), null);
  assert.equal(normalizeObj({ ...note, g: [0, 0, 1] }), null);
  assert.equal(normalizeObj({ ...note, g: [0, "x", 1, 1] }), null);
  assert.equal(normalizeObj({ ...note, g: [0, 1e12, 1, 1] }), null);
  assert.equal(normalizeObj({ ...note, id: 5 }), null);
  assert.equal(normalizeObj(null), null);
  assert.equal(normalizeObj({ ...note, k: "stroke" }), null, "a stroke needs points");
  assert.equal(normalizeObj({ ...note, k: "stroke", pts: new Array(4002).fill(0) }), null);
  const s = normalizeObj({ ...note, k: "stroke", pts: [0, 0, 1, 1, 9], bw: 1, bh: 1 });
  assert.deepEqual(s.pts, [0, 0, 1, 1]);
});

test("out-of-range fields fall back and long text is cut", () => {
  const o = normalizeObj({ ...note, ink: 9, w: -1, fill: 7, text: "x".repeat(900), alt: 3 });
  assert.equal(o.ink, 0);
  assert.equal(o.w, 0);
  assert.equal(o.fill, 0);
  assert.equal(o.text.length, 500);
  assert.equal(o.alt, "");
});

test("store applies upserts, removals and resets in paint order", () => {
  const s = new ObjectStore();
  assert.deepEqual(s.apply({ upsert: [{ ...note, id: "b", z: "b" }, { ...note, id: "a", z: "c" }, { ...note, id: "c", z: "b" }] }), ["b", "a", "c"]);
  assert.deepEqual(s.list().map((o) => o.id), ["b", "c", "a"]);
  assert.deepEqual(s.apply({ upsert: [{ ...note, id: "b", z: "z" }], remove: ["c"] }), []);
  assert.deepEqual(s.list().map((o) => o.id), ["a", "b"]);
  s.apply({ reset: true, upsert: [{ ...note, id: "q" }] });
  assert.deepEqual(s.list().map((o) => o.id), ["q"]);
  assert.equal(s.size, 1);
  assert.equal(compareZ({ z: "a", id: "2" }, { z: "a", id: "1" }), 1);
});

test("outline describes objects for screen readers in reading order", () => {
  const t = makeT({});
  const names = { 1: "Maya", 2: "Leo" };
  const nameOf = (a) => names[a] ?? "someone";
  const a = normalizeObj(note);
  assert.equal(describeObj(a, t, nameOf, { x: 0, y: 0, w: 900, h: 900 }), "Sticky note by Maya, yellow, 'Docs: update', top left");
  const b = normalizeObj({ id: "2.1", k: "arrow", g: [800, 800, 50, 50], ink: 4, by: 2, alt: "points at QA" });
  const entries = outlineEntries([b, a], t, nameOf);
  assert.deepEqual(entries.map((e) => e.id), ["1.1", "2.1"]);
  assert.equal(entries[1].label, "Arrow by Leo, blue, points at QA, bottom right");
  assert.deepEqual(outlineEntries([a, b], t, nameOf, 2).map((e) => e.id), ["2.1"]);
});

test("peers are validated", () => {
  const list = normalizePeers([{ author: 3, name: "Zoe", shape: 9, color: 2, cursor: [1, 2], status: "typing" }, { name: "no author" }, null]);
  assert.equal(list.length, 1);
  assert.equal(list[0].shape, 0);
  assert.deepEqual(list[0].cursor, [1, 2]);
  assert.deepEqual(normalizePeers("x"), []);
  assert.equal(normalizePeers([{ author: 1, cursor: [1, Number.NaN] }])[0].cursor, null);
  assert.equal(personColour(0), "#0b6fb8");
  assert.equal(peerStateKey("drawing"), "drawing");
  assert.equal(peerStateKey(""), "watching");
});
