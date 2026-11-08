import { test } from "node:test";
import assert from "node:assert/strict";
import { addTrail, pruneTrail, trailAlpha, TRAIL_MAX } from "../web/board/trail.js";

test("trail adds valid points, prunes old ones and caps its length", () => {
  const tr = addTrail([], [0, 0, 1, Number.NaN, 2, 2], 0);
  assert.equal(tr.length, 2);
  addTrail(tr, [5, 5], 500);
  pruneTrail(tr, 1000, 900);
  assert.deepEqual(tr.map((p) => p.x), [5]);
  const long = addTrail([], new Array(TRAIL_MAX * 4).fill(1), 0);
  assert.equal(long.length, TRAIL_MAX);
});

test("trail fades linearly", () => {
  assert.equal(trailAlpha(0, 900), 1);
  assert.equal(trailAlpha(450, 900), 0.5);
  assert.equal(trailAlpha(900, 900), 0);
});
