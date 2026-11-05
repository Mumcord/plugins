import { test } from "node:test";
import assert from "node:assert/strict";
import * as k from "../web/board/keyboard.js";

test("single letters pick the tools V P H E S A N T L", () => {
  const map = Object.fromEntries("VPHESANTL".split("").map((c) => [c, k.toolForKey(c.toLowerCase())]));
  assert.deepEqual(map, { V: "select", P: "pen", H: "hl", E: "eraser", S: "shapes", A: "arrow", N: "note", T: "text", L: "laser" });
  assert.equal(k.toolForKey("x"), null);
  assert.equal(k.toolForKey("Enter"), null);
});

test("S cycles through the shapes", () => {
  assert.equal(k.nextShape("rect"), "ellipse");
  assert.equal(k.nextShape("line"), "rect");
});

test("keyboard cursor moves 8 or 64 screen px at any zoom", () => {
  assert.deepEqual(k.moveKbCursor({ x: 0, y: 0 }, "ArrowRight", false, 1), { x: 8, y: 0 });
  assert.deepEqual(k.moveKbCursor({ x: 0, y: 0 }, "ArrowUp", true, 2), { x: 0, y: -32 });
  assert.deepEqual(k.moveKbCursor({ x: 1, y: 1 }, "a", false, 1), { x: 1, y: 1 });
  assert.ok(k.isArrowKey("ArrowDown"));
  assert.ok(!k.isArrowKey("Down"));
});

test("followShift keeps the cursor inside the view", () => {
  const view = { x: 0, y: 0, w: 100, h: 100 };
  assert.deepEqual(k.followShift({ x: 50, y: 50 }, view, 10), { dx: 0, dy: 0 });
  assert.deepEqual(k.followShift({ x: 95, y: 2 }, view, 10), { dx: 5, dy: -8 });
});

test("Ctrl shortcuts", () => {
  assert.equal(k.commandFor("z", true, false), "undo");
  assert.equal(k.commandFor("Z", true, true), "redo");
  assert.equal(k.commandFor("y", true, false), "redo");
  assert.equal(k.commandFor("d", true, false), "dup");
  assert.equal(k.commandFor("z", false, false), null);
});

test("help lists every tool shortcut", () => {
  const keys = k.HELP.map(([key]) => key);
  for (const { key } of k.TOOLS) assert.ok(keys.includes(key), key);
  assert.ok(keys.includes("? / F1"));
});

test("typing targets silence board shortcuts", () => {
  assert.ok(k.isTypingTarget({ tagName: "input" }));
  assert.ok(k.isTypingTarget({ tagName: "DIV", isContentEditable: true }));
  assert.ok(!k.isTypingTarget({ tagName: "BUTTON" }));
  assert.ok(!k.isTypingTarget(null));
});
