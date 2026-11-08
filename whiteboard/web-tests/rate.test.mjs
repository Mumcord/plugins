import { test } from "node:test";
import assert from "node:assert/strict";
import { TokenBucket, Pacer } from "../web/board/rate.js";

test("token bucket refills over time", () => {
  const b = new TokenBucket(10, 2, 0);
  assert.ok(b.take(0));
  assert.ok(b.take(0));
  assert.ok(!b.take(0));
  assert.equal(b.wait(0), 100);
  assert.ok(b.take(100));
});

test("pacer queues edits and drops ephemeral traffic when over budget", () => {
  const out = [];
  const p = new Pacer((m) => out.push(m), 10, 2, 0);
  p.send("a", false, 0);
  p.send("b", false, 0);
  assert.equal(p.send("cursor", true, 0), false);
  p.send("c", false, 0);
  assert.deepEqual(out, ["a", "b"]);
  assert.ok(p.pump(0) > 0);
  assert.equal(p.pump(100), 0);
  assert.deepEqual(out, ["a", "b", "c"]);
});

test("pacer keeps order: nothing jumps a waiting queue", () => {
  const out = [];
  const p = new Pacer((m) => out.push(m), 10, 1, 0);
  p.send("a", false, 0);
  p.send("b", false, 0);
  // A token is back, but "b" waits in the queue, so "c" must queue behind it.
  p.send("c", false, 150);
  p.pump(300);
  assert.deepEqual(out, ["a", "b"]);
  p.pump(500);
  assert.deepEqual(out, ["a", "b", "c"]);
});
