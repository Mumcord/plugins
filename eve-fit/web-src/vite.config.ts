import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";

/**
 * The wheel draws the ship from CCP's image server. The web view has no network, so the hull
 * render comes from `renders/<type id>.webp` in the package instead (see scripts/build.mjs).
 */
const HULL_FROM = "`https://images.evetech.net/types/${typeId}/render?size=1024`";
const HULL_TO = "`renders/${typeId}.webp`";

function localHullRenders(): Plugin {
  return {
    name: "eve-fit-local-hull-renders",
    transform(code, id) {
      if (!id.includes("@eveshipfit/ui-ingame/dist/index.mjs")) return null;
      const count = code.split(HULL_FROM).length - 1;
      if (count !== 1) {
        // A new ui-ingame changed how it loads the hull; fail instead of shipping a broken wheel.
        throw new Error(`expected the hull render URL once in ui-ingame, found it ${count} times`);
      }
      return code.replace(HULL_FROM, HULL_TO);
    },
  };
}

export default defineConfig({
  plugins: [react(), localHullRenders()],
  define: { "process.env.NODE_ENV": JSON.stringify("production") },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // Classic script, not a module: module scripts need CORS, which the sealed frame's
    // opaque origin does not get.
    lib: {
      entry: "src/main.tsx",
      formats: ["iife"],
      name: "EveFit",
      fileName: () => "app.js",
      cssFileName: "app",
    },
    cssCodeSplit: false,
    assetsInlineLimit: 0,
    minify: true,
    sourcemap: false,
    reportCompressedSize: false,
  },
});
