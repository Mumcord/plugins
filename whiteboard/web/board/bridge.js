// The message bridge to the plugin: JSON strings both ways. The page holds no authority; every
// edit goes out as a message and comes back as `objs`.

import { Pacer } from "./rate.js";

/** Stay well below the host's 60 messages per second. */
const RATE = 45;
const BURST = 20;
/**
 * The host's UI drops every page -> plugin message above 16 KiB, on the stage too; only plugin ->
 * page messages get the stage's 64 KiB.
 */
const MAX_SEND_BYTES = 16 * 1024;

/**
 * Connects to the plugin. `place` is where the page runs (`stage` or `side`) and `maxBytes` the
 * largest message that place receives (64 KiB on the stage, 16 KiB in the side panel);
 * `onMessage` gets every parsed message with a string `t`. A plugin may show several web views
 * and the host does not say which one a message is for or from, so every message carries the
 * place: `p` going out, `to` coming in.
 */
export function connect(onMessage, maxBytes, place) {
  let timer = 0;
  const pacer = new Pacer((text) => window.parent.postMessage(text, "*"), RATE, BURST, performance.now());

  const pump = () => {
    timer = 0;
    const wait = pacer.pump(performance.now());
    if (wait > 0) timer = setTimeout(pump, wait);
  };

  window.addEventListener("message", (event) => {
    if (event.source !== window.parent || typeof event.data !== "string" || event.data.length > maxBytes) return;
    let msg;
    try {
      msg = JSON.parse(event.data);
    } catch (err) {
      console.warn("whiteboard: dropped a malformed bridge message", err.name);
      return;
    }
    if (msg && typeof msg === "object" && typeof msg.t === "string" && msg.to === place) onMessage(msg);
  });

  return {
    /**
     * Sends `msg`. Ephemeral messages (cursor, live strokes, laser, view) are dropped when the rate
     * budget is used up; everything else waits its turn.
     */
    send(msg, ephemeral = false) {
      const text = JSON.stringify({ p: place, ...msg });
      if (new TextEncoder().encode(text).length > MAX_SEND_BYTES) {
        console.warn(`whiteboard: message "${msg.t}" is too large for the bridge and was not sent`);
        return false;
      }
      const ok = pacer.send(text, ephemeral, performance.now());
      if (!timer && pacer.queue.length > 0) timer = setTimeout(pump, pacer.bucket.wait(performance.now()) || 1);
      return ok;
    },
  };
}
