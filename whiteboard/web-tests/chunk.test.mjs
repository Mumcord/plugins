import { test } from "node:test";
import assert from "node:assert/strict";
import * as c from "../web/board/chunk.js";

test("base64 matches Node's encoder for all tail lengths", () => {
  for (let n = 0; n < 40; n++) {
    const bytes = Uint8Array.from({ length: n }, (_, i) => (i * 97 + n) & 255);
    assert.equal(c.bytesToBase64(bytes), Buffer.from(bytes).toString("base64"));
  }
});

test("png chunks stay within the limit and reassemble", () => {
  const bytes = Uint8Array.from({ length: 100_000 }, (_, i) => (i * 31) & 255);
  const msgs = c.pngMessages(7, bytes);
  assert.ok(msgs.length > 1);
  for (const [i, m] of msgs.entries()) {
    assert.equal(m.t, "png");
    assert.equal(m.req, 7);
    assert.equal(m.part, i);
    assert.equal(m.of, msgs.length);
    assert.ok(m.data.length <= c.PNG_CHUNK);
    assert.equal(m.data.length % 4, 0);
  }
  assert.deepEqual(Buffer.from(msgs.map((m) => m.data).join(""), "base64"), Buffer.from(bytes));
});

test("png chunks fit the host's 16 KiB page-to-plugin limit on every place", () => {
  // Mumcord's UI drops any page message above 16 KiB, the stage's included, so a larger part
  // never reaches the plugin and the export times out.
  const bytes = Uint8Array.from({ length: 200_000 }, (_, i) => (i * 31) & 255);
  for (const place of ["stage", "side"]) {
    for (const m of c.pngMessages("r1", bytes)) {
      assert.ok(Buffer.byteLength(JSON.stringify({ p: place, ...m })) <= 16 * 1024);
    }
  }
});

test("an empty PNG still gives one part", () => {
  assert.deepEqual(c.pngMessages(1, new Uint8Array(0)), [{ t: "png", req: 1, part: 0, of: 1, data: "" }]);
  assert.throws(() => c.chunkString("abc", 0));
});

test("renderScale shrinks oversized renders", () => {
  assert.equal(c.renderScale(100, 100, 4), 4);
  assert.equal(c.renderScale(10000, 100, 4), 16384 / 10000);
  const s = c.renderScale(8000, 8000, 4);
  assert.ok(8000 * 8000 * s * s <= 64 * 1024 * 1024 + 1);
});
