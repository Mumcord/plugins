// Entry for `node --test whiteboard/web-tests/`: Node resolves a directory argument to its
// index.js, so this loads every *.test.mjs next to it.
import { readdirSync } from "node:fs";

const dir = new URL("./", import.meta.url);
for (const name of readdirSync(dir).filter((n) => n.endsWith(".test.mjs")).sort()) {
  await import(new URL(name, dir).href);
}
