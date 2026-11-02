// Snack Race page. The plugin owns the race (lobby, start, other pets, results) and sends its
// state as JSON; this page only runs the user's own pet: the timing bar, taps and hops.
// Everything from the bridge is treated as untrusted and only ever set as text.
"use strict";

/** One full back-and-forth of the marker over the timing bar. */
const SWEEP_MS = 1400;
/** Half the width of the snack zone, in percent of the bar. */
const ZONE_HALF = 9;
/** After a miss the pet stumbles and cannot hop for this long. */
const STUMBLE_MS = 600;
/** The shortest time between two hops: one hop animation. */
const HOP_MS = 250;
const PET_PATH = /^pets\/[a-z0-9-]+\.svg$/;

const $ = (id) => document.getElementById(id);
const el = {
  title: $("title"),
  clock: $("clock"),
  status: $("status"),
  track: $("track"),
  play: $("play"),
  bar: $("bar"),
  zone: $("zone"),
  marker: $("marker"),
  hop: $("hop"),
  hint: $("hint"),
  note: $("note"),
  start: $("start"),
  results: $("results"),
};

/** The plugin's latest state. */
let state = null;
/** The user's run in this page: race id, clock start, hops, when the next tap counts. */
let run = null;
let lobbyEnds = 0;
let frame = 0;

function isState(s) {
  return (
    typeof s === "object" &&
    s !== null &&
    Number.isInteger(s.id) &&
    typeof s.phase === "string" &&
    Number.isInteger(s.goal) &&
    s.goal > 0 &&
    Number.isInteger(s.raceMs) &&
    Number.isInteger(s.leftMs) &&
    Array.isArray(s.course) &&
    s.course.length === s.goal &&
    s.course.every((z) => Number.isInteger(z) && z >= 0 && z <= 100) &&
    Array.isArray(s.runners) &&
    s.runners.length > 0 &&
    Array.isArray(s.results) &&
    typeof s.strings === "object" &&
    s.strings !== null
  );
}

function text(key) {
  const value = state.strings[key];
  return typeof value === "string" ? value : "";
}

function send(message) {
  window.parent.postMessage(message, "*");
}

function secondsLabel(ms) {
  return String(Math.max(0, Math.ceil(ms / 1000)));
}

/** Marker position in percent of the bar, sweeping back and forth. */
function markerAt(now) {
  const t = ((now - run.start) % SWEEP_MS) / SWEEP_MS;
  const across = t < 0.5 ? t * 2 : 2 - t * 2;
  return 2 + across * 96;
}

function running() {
  return (
    run !== null &&
    state.phase === "racing" &&
    run.hops < state.goal &&
    performance.now() - run.start < state.raceMs
  );
}

function petElement(runner) {
  const pet = document.createElement("div");
  pet.className = "pet";
  pet.style.setProperty("--progress", String(Math.min(runner.hops, state.goal) / state.goal));
  if (typeof runner.pet === "string" && PET_PATH.test(runner.pet)) {
    const img = document.createElement("img");
    img.src = runner.pet;
    img.alt = "";
    pet.append(img);
  } else {
    const blob = document.createElement("div");
    blob.className = "blob";
    pet.append(blob);
  }
  return pet;
}

function snackElement() {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 32 32");
  svg.setAttribute("class", "snack");
  svg.setAttribute("aria-hidden", "true");
  const cookie = document.createElementNS(ns, "circle");
  cookie.setAttribute("cx", "16");
  cookie.setAttribute("cy", "16");
  cookie.setAttribute("r", "12");
  cookie.setAttribute("fill", "#e8b56a");
  cookie.setAttribute("stroke", "#7a4a12");
  cookie.setAttribute("stroke-width", "2");
  svg.append(cookie);
  for (const [x, y] of [
    [12, 12],
    [20, 14],
    [14, 20],
    [21, 21],
  ]) {
    const chip = document.createElementNS(ns, "circle");
    chip.setAttribute("cx", String(x));
    chip.setAttribute("cy", String(y));
    chip.setAttribute("r", "1.8");
    chip.setAttribute("fill", "#5a3410");
    svg.append(chip);
  }
  return svg;
}

function hopsText(hops) {
  return text("hops").replace("{{n}}", String(hops));
}

function renderTrack() {
  el.track.setAttribute("aria-label", text("track"));
  const lanes = state.runners.map((runner) => {
    const hops = runner.me && run !== null && run.id === state.id ? run.hops : runner.hops;
    const lane = document.createElement("li");
    lane.className = runner.me ? "lane me" : "lane";
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = typeof runner.name === "string" ? runner.name : "";
    const path = document.createElement("div");
    path.className = "path";
    path.setAttribute("role", "img");
    path.setAttribute("aria-label", hopsText(hops));
    path.append(petElement({ ...runner, hops }), snackElement());
    lane.append(name, path);
    if (runner.me) lane.dataset.me = "1";
    return lane;
  });
  el.track.replaceChildren(...lanes);
}

/** Moves the user's pet without rebuilding the track, so its animation can play. */
function moveMyPet(className) {
  const pet = el.track.querySelector('[data-me="1"] .pet');
  const path = el.track.querySelector('[data-me="1"] .path');
  if (pet === null || path === null) return;
  pet.style.setProperty("--progress", String(run.hops / state.goal));
  path.setAttribute("aria-label", hopsText(run.hops));
  pet.classList.remove("hopped", "stumbled");
  // Restart the animation even if the same class is set twice in a row.
  void pet.offsetWidth;
  pet.classList.add(className);
}

function renderResults() {
  const over = state.phase === "over";
  el.results.hidden = !over || state.results.length === 0;
  el.results.setAttribute("aria-label", text("results"));
  el.results.replaceChildren(
    ...state.results.map((line) => {
      const item = document.createElement("li");
      // The plugin numbers the lines; the list numbers are for screen readers only.
      item.textContent = typeof line === "string" ? line.replace(/^\d+\.\s*/, "") : "";
      return item;
    }),
  );
}

function apply(next) {
  const root = document.documentElement;
  state = next;
  root.lang = text("lang") || "en";
  root.classList.toggle("dark", state.dark === true);
  root.classList.toggle("light", state.dark !== true);
  document.body.className = `motion-${["full", "subtle"].includes(state.motion) ? state.motion : "still"}`;
  document.title = text("title");
  el.title.textContent = text("title");
  // Once over, the list below holds the results the status line would repeat.
  const listed = state.phase === "over" && state.results.length > 0;
  el.status.textContent = listed ? text("results") : text("status");
  el.hop.textContent = text("hop");
  el.hint.textContent = text("hint");
  el.bar.setAttribute("aria-label", text("bar"));
  el.start.textContent = text("startNow");
  el.start.hidden = !(state.phase === "waiting" && state.host);
  lobbyEnds =
    state.phase === "waiting" && Number.isInteger(state.startsIn)
      ? performance.now() + state.startsIn * 1000
      : 0;

  if (state.phase === "racing" && (run === null || run.id !== state.id)) {
    const mine = state.runners.find((r) => r.me);
    const spent = Math.max(0, state.raceMs - state.leftMs);
    run = {
      id: state.id,
      start: performance.now() - spent,
      hops: mine && Number.isInteger(mine.hops) ? mine.hops : 0,
      readyAt: 0,
    };
    el.play.hidden = false;
    el.note.textContent = "";
    el.hop.focus();
  }
  if (state.phase !== "racing" && state.phase !== "finished") {
    el.play.hidden = true;
  }
  renderTrack();
  renderResults();
  tick();
}

function tap() {
  if (!running()) return;
  const now = performance.now();
  if (now < run.readyAt) return;
  const zone = state.course[run.hops];
  if (Math.abs(markerAt(now) - zone) <= ZONE_HALF) {
    run.hops += 1;
    run.readyAt = now + HOP_MS;
    el.note.textContent = "";
    send(JSON.stringify({ hop: run.hops, ms: Math.round(now - run.start) }));
    moveMyPet("hopped");
  } else {
    run.readyAt = now + STUMBLE_MS;
    el.note.textContent = text("stumble");
    moveMyPet("stumbled");
  }
}

/** Draws the clock, zone and marker; runs every frame only while the user races. */
function tick() {
  cancelAnimationFrame(frame);
  const now = performance.now();
  if (lobbyEnds > 0) {
    el.clock.textContent = `${text("startsIn")} ${secondsLabel(lobbyEnds - now)}`;
  } else if (run !== null && run.id === state.id && state.phase === "racing") {
    el.clock.textContent = `${text("timeLeft")} ${secondsLabel(state.raceMs - (now - run.start))}`;
  } else {
    el.clock.textContent = "";
  }
  const active = running();
  el.hop.disabled = !active;
  if (active) {
    const zone = state.course[run.hops];
    el.zone.style.left = `${zone - ZONE_HALF}%`;
    el.zone.style.width = `${ZONE_HALF * 2}%`;
    el.marker.style.left = `${markerAt(now)}%`;
  }
  if (active || lobbyEnds > now) {
    frame = requestAnimationFrame(tick);
  }
}

window.addEventListener("message", (event) => {
  if (event.source !== window.parent || typeof event.data !== "string") return;
  let value;
  try {
    value = JSON.parse(event.data);
  } catch {
    return;
  }
  if (isState(value)) apply(value);
});

// Space and Enter tap anywhere; preventing the default also stops the focused button from
// turning the key into a second tap by a click.
window.addEventListener("keydown", (event) => {
  if (state === null || event.repeat || event.altKey || event.ctrlKey || event.metaKey) return;
  if (event.key !== " " && event.key !== "Enter") return;
  if (event.target === el.start) return;
  event.preventDefault();
  tap();
});
el.hop.addEventListener("pointerdown", (event) => {
  if (event.isPrimary && event.button === 0) tap();
});
// A click without a pointer press: a screen reader activating the button.
el.hop.addEventListener("click", (event) => {
  if (event.detail === 0) tap();
});
el.bar.addEventListener("pointerdown", (event) => {
  if (event.isPrimary && event.button === 0) tap();
});
el.start.addEventListener("click", () => send("start"));

// Ask the plugin for the state once we listen; anything it posted earlier was lost.
send("ready");
