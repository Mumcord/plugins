// Builds eve-fit's web/ folder: the eveship.fit fitting wheel, EVE's data and images, and the
// ship renders, all for the sealed, offline web view.
//
//   node scripts/build.mjs                    build into ../web
//   node scripts/build.mjs --check            build into a temporary folder and fail if ../web differs
//   node scripts/build.mjs --update-renders   also accept changed or new ship renders into renders.lock
//
// Run it through build.sh, which installs the exact packages of package-lock.json first.

import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { cp, mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

import sharp from "sharp";
import { build } from "vite";
import { loadSde } from "@eveshipfit/sde-loader";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const WEB = join(ROOT, "..", "web");
const MODULES = join(ROOT, "node_modules");
const RENDER_CACHE = join(ROOT, ".cache", "renders");
const RENDERS_LOCK = join(ROOT, "renders.lock");
/** Base64 characters per data script; a multiple of 4, so every part decodes on its own. */
const PART_CHARS = 4 * 1024 * 1024;
/** Matches the wheel's largest size; CCP's 1024 px renders would double the package. */
const RENDER_SIZE = 512;
const RENDER_QUALITY = 80;
const USER_AGENT = "Mumcord eve-fit plugin build (+https://github.com/Mumcord/plugins)";

const args = new Set(process.argv.slice(2));
const check = args.has("--check");
const updateRenders = args.has("--update-renders");

async function main() {
  const out = check ? await mkdtemp(join(tmpdir(), "eve-fit-web-")) : WEB;
  try {
    await rm(out, { recursive: true, force: true });
    await mkdir(out, { recursive: true });
    await buildApp(out);
    await cp(join(ROOT, "index.html"), join(out, "index.html"));
    await cp(join(MODULES, "@eveshipfit/images/dist/images"), join(out, "images"), { recursive: true });
    await writeData(out);
    await writeLicences(out);
    await writeRenders(out);
    if (check) await compare(out, WEB);
  } finally {
    if (check) await rm(out, { recursive: true, force: true });
  }
}

async function buildApp(out) {
  const dist = join(ROOT, "dist");
  await build({ root: ROOT, configFile: join(ROOT, "vite.config.ts"), logLevel: "warn" });
  for (const name of await readdir(dist)) {
    if (name === "app.js" || name === "app.css") {
      await cp(join(dist, name), join(out, name));
    } else if (!name.endsWith(".wasm")) {
      // The engine's default .wasm is never fetched (the page passes the bytes); anything else
      // would be a file the page needs but cannot load.
      throw new Error(`unexpected build output ${name}`);
    }
  }
  await rm(dist, { recursive: true, force: true });
}

/** Wraps each binary file as base64 parts in classic scripts (the frame cannot fetch). */
async function writeData(out) {
  const files = {
    sde: "@eveshipfit/sde/dist/sde.dat",
    wasm: "@eveshipfit/dogma-engine/esf_dogma_engine_bg.wasm",
    images: "@eveshipfit/images/dist/images.dat",
    texts: "@eveshipfit/sde/dist/texts.dat",
  };
  await mkdir(join(out, "data"), { recursive: true });
  const parts = {};
  for (const [name, path] of Object.entries(files)) {
    const base64 = (await readFile(join(MODULES, path))).toString("base64");
    parts[name] = Math.ceil(base64.length / PART_CHARS);
    for (let i = 0; i < parts[name]; i++) {
      const chunk = base64.slice(i * PART_CHARS, (i + 1) * PART_CHARS);
      await writeFile(join(out, "data", `${name}-${i}.js`), `eveFitData(${JSON.stringify(name)},${i},"${chunk}");\n`);
    }
  }
  await writeFile(join(out, "data", "parts.js"), `window.eveFitParts=${JSON.stringify(parts)};\n`);
}

async function writeLicences(out) {
  const read = (path) => readFile(join(MODULES, path), "utf8");
  const sections = [
    [
      "EVEShip.fit: @eveshipfit/ui-ingame, react-hooks, fitting, sde-loader, dogma-engine, esi, zkillboard (MIT)",
      await read("@eveshipfit/ui-ingame/LICENSE"),
    ],
    ["React, React DOM and scheduler (MIT)", await read("react/LICENSE")],
    ["FlatBuffers (Apache-2.0)", await read("flatbuffers/LICENSE")],
    ["EVE Online data: @eveshipfit/sde", await read("@eveshipfit/sde/LICENSE")],
    ["EVE Online icons: @eveshipfit/images", await read("@eveshipfit/images/LICENSE")],
    ["EVE Online ship renders: CCP's image server, under the licence below", ""],
    ["CCP hf. Developer License Agreement", await read("@eveshipfit/sde/LICENSE.EVE")],
  ];
  const text = sections.map(([title, body]) => `${title}\n${"=".repeat(title.length)}\n\n${body.trim()}`).join("\n\n\n");
  await writeFile(join(out, "data", "licences.js"), `window.eveFitLicences=${JSON.stringify(text)};\n`);
  const escaped = text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
  await writeFile(
    join(out, "licences.html"),
    `<!doctype html>\n<html lang="en">\n<head><meta charset="utf-8"><title>EVE fit: licences</title></head>\n<body><pre>${escaped}</pre></body>\n</html>\n`,
  );
}

/** `type id -> sha256 of CCP's JPEG`, or "missing" when CCP has no render for the hull. */
async function readLock() {
  if (!existsSync(RENDERS_LOCK)) return new Map();
  const lines = (await readFile(RENDERS_LOCK, "utf8")).split("\n").filter((l) => l && !l.startsWith("#"));
  return new Map(lines.map((line) => line.split(" ")));
}

async function fetchRender(id) {
  const cached = join(RENDER_CACHE, `${id}.jpg`);
  if (existsSync(cached)) return readFile(cached);
  const url = `https://images.evetech.net/types/${id}/render?size=${RENDER_SIZE}`;
  const response = await fetch(url, { headers: { "User-Agent": USER_AGENT } });
  // CCP answers 400 or 404 for hulls without a render (e.g. some special editions).
  if (response.status === 400 || response.status === 404) return undefined;
  if (!response.ok) throw new Error(`${url}: ${response.status} ${response.statusText}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  await mkdir(RENDER_CACHE, { recursive: true });
  await writeFile(cached, bytes);
  return bytes;
}

async function writeRenders(out) {
  const sde = await loadSde({ bytes: new Uint8Array(await readFile(join(MODULES, "@eveshipfit/sde/dist/sde.dat"))) });
  const ids = [...new Set(sde.shipTree().flatMap((g) => g.races.flatMap((r) => r.ships.map((s) => s.id))))].sort(
    (a, b) => a - b,
  );
  const lock = await readLock();
  const next = new Map();
  const problems = [];
  await mkdir(join(out, "renders"), { recursive: true });
  const queue = [...ids];
  const worker = async () => {
    for (let id = queue.shift(); id !== undefined; id = queue.shift()) {
      const bytes = await fetchRender(id);
      const digest = bytes ? createHash("sha256").update(bytes).digest("hex") : "missing";
      next.set(String(id), digest);
      const pinned = lock.get(String(id));
      if (pinned !== digest && !updateRenders) {
        problems.push(`${id}: renders.lock has ${pinned ?? "nothing"}, CCP serves ${digest}`);
        continue;
      }
      if (bytes) {
        await sharp(bytes)
          .webp({ quality: RENDER_QUALITY, effort: 6 })
          .toFile(join(out, "renders", `${id}.webp`));
      }
    }
  };
  await Promise.all(Array.from({ length: 8 }, worker));
  if (problems.length > 0) {
    throw new Error(
      `ship renders differ from renders.lock (run build.sh --update-renders after checking them):\n${problems.join("\n")}`,
    );
  }
  if (updateRenders) {
    const lines = [...next].sort(([a], [b]) => Number(a) - Number(b)).map(([id, digest]) => `${id} ${digest}`);
    const header = `# SHA-256 of CCP's ${RENDER_SIZE} px hull renders (images.evetech.net), by type id. Written by build.sh --update-renders.`;
    await writeFile(RENDERS_LOCK, `${header}\n${lines.join("\n")}\n`);
  }
}

async function listFiles(dir) {
  const entries = await readdir(dir, { recursive: true, withFileTypes: true });
  return entries
    .filter((e) => e.isFile())
    .map((e) => relative(dir, join(e.parentPath, e.name)))
    .sort();
}

async function compare(built, committed) {
  const [a, b] = await Promise.all([listFiles(built), listFiles(committed)]);
  const differ = [...new Set([...a, ...b])].filter((name) => !a.includes(name) || !b.includes(name));
  for (const name of a.filter((n) => b.includes(n))) {
    const [x, y] = await Promise.all([readFile(join(built, name)), readFile(join(committed, name))]);
    if (!x.equals(y)) differ.push(name);
  }
  if (differ.length > 0) {
    throw new Error(`web/ is not what build.sh builds; differing files:\n${differ.slice(0, 50).join("\n")}`);
  }
  console.log(`web/ matches the build (${a.length} files).`);
}

main().catch((error) => {
  console.error(error.message ?? error);
  process.exit(1);
});
