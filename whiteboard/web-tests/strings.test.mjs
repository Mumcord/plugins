import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { EN, makeT } from "../web/board/strings.js";

const dir = new URL("../web/board/", import.meta.url);

test("strings-keys.json lists exactly the English texts", () => {
  const listed = JSON.parse(readFileSync(new URL("strings-keys.json", dir), "utf8"));
  assert.deepEqual(listed, EN);
});

test("every literal key used by the page exists", () => {
  const used = new Set();
  for (const f of readdirSync(dir).filter((n) => n.endsWith(".js"))) {
    const src = readFileSync(new URL(f, dir), "utf8");
    for (const m of src.matchAll(/\bt(?:\.n)?\("([a-zA-Z0-9.]+)"/g)) used.add(m[1]);
  }
  assert.ok(used.size > 50);
  for (const key of used) {
    const plural = Object.hasOwn(EN, `${key}.one`) && Object.hasOwn(EN, `${key}.other`);
    assert.ok(Object.hasOwn(EN, key) || plural, `missing string ${key}`);
  }
});

test("dynamic key families are complete", () => {
  for (const k of ["stroke", "hl", "rect", "ellipse", "diamond", "line", "arrow", "note", "text"]) assert.ok(EN[`kind.${k}`], k);
  for (const k of ["select", "pen", "hl", "eraser", "shapes", "arrow", "note", "text", "laser"]) assert.ok(EN[`tool.${k}`], k);
  for (let i = 0; i < 6; i++) assert.ok(EN[`ink.${i}`]);
  for (let i = 0; i < 5; i++) assert.ok(EN[`note.${i}`]);
  for (const k of ["tl", "t", "tr", "l", "c", "r", "bl", "b", "br"]) assert.ok(EN[`pos.${k}`]);
  for (const k of ["watching", "pointing", "drawing", "typing", "highlighting"]) assert.ok(EN[`state.${k}`]);
});

test("lookup falls back to English and fills placeholders and plurals", () => {
  const t = makeT({ "act.export": "Exportieren", "queue.other": "{n} Striche…" });
  assert.equal(t("act.export"), "Exportieren");
  assert.equal(t("act.post"), "Post to chat");
  assert.equal(t("no.such.key"), "no.such.key");
  assert.equal(t("tool.withKey", { name: "Pen", key: "P" }), "Pen (P)");
  assert.equal(t.n("queue", 1), "Sending 1 stroke…");
  assert.equal(t.n("queue", 3), "3 Striche…");
  assert.equal(makeT(null)("act.post"), "Post to chat");
});
