// Skyloom's page: draws the loom and the heads-up display from what the plugin sends, and turns
// clicks and keys into requests. The plugin owns the game; this page never decides anything and
// has no network or storage. Everything from the plugin may contain other people's names, so all
// text goes through esc() before it reaches innerHTML.

type Pt = [number, number];
/** owner, star index, x, y, locked */
type StarTuple = [number, number, number, number, boolean];

interface Player {
  s: number;
  name: string;
  shape: number;
  talking: boolean;
  weaved: boolean;
}

interface SkyCard {
  name: string;
  lines: Pt[][];
  points: Pt[];
  mirror: boolean;
}

interface Reveal {
  figure: string | null;
  resonance: number;
  bonus: number;
  /** Seconds the Weaver had left when tying off early. */
  spare: number | null;
  woven: boolean;
  gold: boolean;
  /** owner, star index, x, y, resonance */
  stars: [number, number, number, number, number][];
  targets: Pt[];
  order: number[];
  ghost: Pt[][];
  verify: string;
  truest: number | null;
  loosest: number | null;
  /** Where the truest star sat on the figure ("on the beak"). */
  truestAt: string | null;
  /** What the loosest star should have been ("the foot wandered off"). */
  loosestLost: string | null;
  guess: string | null;
}

interface Round {
  no: number;
  of: number;
  weaver: number;
  twin: number | null;
  twist: string;
  msLeft: number;
  dur: number;
  stars: StarTuple[];
  each: number;
  left: number[];
  card: SkyCard | null;
  reveal: Reveal | null;
  void: string | null;
}

interface Results {
  rounds: {
    no: number;
    weaver: string;
    figure: string | null;
    lines: Pt[][] | null;
    resonance: number | null;
    verified: boolean;
  }[];
  team: number | null;
  honours: {
    silver: string | null;
    truest: string | null;
    truestCount: number;
    truestOf: number;
    wanderer: string | null;
  };
  name: string;
  summaryPosted: boolean;
}

interface View {
  t: "view";
  me: number;
  place: string;
  phase: string;
  role: string;
  lamplighter: number | null;
  players: Player[];
  spectators: string[];
  settings: { weave: number; mixed: boolean };
  round: Round | null;
  next: number | null;
  canEnd: boolean;
  results: Results | null;
  past: { name: string; team: number; count: number }[];
  sound: { volume: number; muted: boolean };
  theme: string;
  reduceMotion: boolean;
  live: boolean;
}

type Incoming =
  | View
  | { t: "strings"; s: Record<string, string> }
  | { t: "stars"; r: number; s: StarTuple[] }
  | { t: "talk"; s: number; on: boolean }
  | { t: "pluck"; by: number; s: number; i: number }
  | { t: "say"; text: string };

const LOOM_W = 1600;
const LOOM_H = 900;
const STEP = 20;
const FINE_STEP = 5;
const MOVE_EVERY_MS = 40;
/** The mockups' star colours, one per shape (circle, triangle, diamond, square, star, …). */
const COLOURS = [
  "#ff8fb1",
  "#e69f00",
  "#cc79a7",
  "#2fbf8f",
  "#f0e442",
  "#e8703a",
  "#7aa7ff",
  "#b9a4ff",
  "#56b4e9",
  "#a8e66b",
  "#f08cff",
  "#ff7a7a",
];

let strings: Record<string, string> = {};
let view: View | null = null;
let viewAt = 0;
let showRules = false;
let toast: { text: string; until: number } | null = null;
/** The star the toast points at. */
let toastAt: string | null = null;
let lastAnnounced = -1;
/** Where each star is drawn, gliding towards its target. */
const shown = new Map<string, Pt>();
const target = new Map<string, Pt>();
/** Own stars moved locally win over echoes for a moment. */
let ownMovedAt = 0;
let lastMoveSent = 0;
let dragging: { key: string; star: number } | null = null;

const sky = document.getElementById("sky") as HTMLElement;
const sayRegion = document.getElementById("say") as HTMLElement;

function send(message: object): void {
  window.parent.postMessage(JSON.stringify(message), "*");
}

function tr(key: string, args: Record<string, string | number> = {}): string {
  let text = strings[key] ?? key;
  for (const [k, v] of Object.entries(args)) text = text.split(`{{${k}}}`).join(String(v));
  return text;
}

function esc(text: string): string {
  return text.replace(
    /[&<>"']/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] ?? c,
  );
}

function announce(text: string): void {
  // Clearing first makes screen readers repeat an identical message.
  sayRegion.textContent = "";
  window.setTimeout(() => (sayRegion.textContent = text), 50);
}

function reduceMotion(): boolean {
  return (
    (view?.reduceMotion ?? false) || window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

function player(session: number): Player | undefined {
  return view?.players.find((p) => p.s === session);
}

function nameOf(session: number): string {
  return player(session)?.name ?? "?";
}

function colourOf(session: number): string {
  return COLOURS[(player(session)?.shape ?? 0) % COLOURS.length] ?? "#ccc";
}

function initialOf(session: number): string {
  return (nameOf(session).trim()[0] ?? "?").toUpperCase();
}

// ----- Shapes: every player has their own, so colour is never the only signal -----

function polygon(n: number, r: number, rotate = -Math.PI / 2): string {
  const pts: string[] = [];
  for (let i = 0; i < n; i++) {
    const a = rotate + (i * 2 * Math.PI) / n;
    pts.push(`${(r * Math.cos(a)).toFixed(1)},${(r * Math.sin(a)).toFixed(1)}`);
  }
  return `M${pts.join("L")}Z`;
}

function starPath(r: number): string {
  const pts: string[] = [];
  for (let i = 0; i < 10; i++) {
    const a = -Math.PI / 2 + (i * Math.PI) / 5;
    const rr = i % 2 === 0 ? r : r * 0.45;
    pts.push(`${(rr * Math.cos(a)).toFixed(1)},${(rr * Math.sin(a)).toFixed(1)}`);
  }
  return `M${pts.join("L")}Z`;
}

function shapePath(shape: number, r: number): string {
  const t = r * 0.38;
  switch (shape % 12) {
    case 0:
      return `M${-r},0a${r},${r} 0 1,0 ${2 * r},0a${r},${r} 0 1,0 ${-2 * r},0Z`;
    case 1:
      return polygon(3, r * 1.15);
    case 2:
      return polygon(4, r * 1.15);
    case 3:
      return polygon(4, r * 1.2, -Math.PI / 4);
    case 4:
      return starPath(r * 1.25);
    case 5:
      return polygon(6, r * 1.1);
    case 6:
      return polygon(5, r * 1.1);
    case 7:
      return `M${-t},${-r}H${t}V${-t}H${r}V${t}H${t}V${r}H${-t}V${t}H${-r}V${-t}H${-t}Z`;
    case 8:
      return `M0,${-t * 1.4}L${r * 0.7},${-r}L${r},${-r * 0.7}L${t * 1.4},0L${r},${r * 0.7}L${r * 0.7},${r}L0,${t * 1.4}L${-r * 0.7},${r}L${-r},${r * 0.7}L${-t * 1.4},0L${-r},${-r * 0.7}L${-r * 0.7},${-r}Z`;
    case 9:
      return `M${r * 0.3},${-r}A${r},${r} 0 1,0 ${r * 0.3},${r}A${r * 0.75},${r * 0.75} 0 1,1 ${r * 0.3},${-r}Z`;
    case 10:
      return `M0,${r}C${-r * 1.6},${-r * 0.1} ${-r * 0.6},${-r * 1.3} 0,${-r * 0.4}C${r * 0.6},${-r * 1.3} ${r * 1.6},${-r * 0.1} 0,${r}Z`;
    default:
      return `M0,${-r * 1.2}C${r * 0.9},${-r * 0.1} ${r},${r} 0,${r}C${-r},${r} ${-r * 0.9},${-r * 0.1} 0,${-r * 1.2}Z`;
  }
}

// ----- Small parts -----

const ICONS: Record<string, string> = {
  loom: `<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="M5 17 10 7l4 8 5-9" stroke-dasharray="3 2"/><circle cx="5" cy="17" r="2" fill="currentColor"/><circle cx="10" cy="7" r="2" fill="currentColor"/><circle cx="14" cy="15" r="2" fill="currentColor"/><circle cx="19" cy="6" r="2" fill="currentColor"/></svg>`,
  verified: `<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" fill="var(--color-success, #3ddc97)"><path d="M12 2l3 7h7l-5.5 4.5L18.5 21 12 16.5 5.5 21l2-7.5L2 9h7z"/></svg>`,
  eye: `<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z"/></svg>`,
  help: `<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM9.5 9a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .9-1 1.6V14M12 17h.01"/></svg>`,
  side: `<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 4h16v16H4zM15 4v16"/></svg>`,
  popout: `<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 4h6v6M20 4l-8 8M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"/></svg>`,
  stage: `<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 5h16v14H4zM4 15h16"/></svg>`,
  sound: `<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 5 6 9H3v6h3l5 4zM15.5 8.5a5 5 0 0 1 0 7M18.5 5.5a9 9 0 0 1 0 13"/></svg>`,
  muted: `<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 5 6 9H3v6h3l5 4zM16 9l6 6M22 9l-6 6"/></svg>`,
};

function button(label: string, intent: string, extra = "", cls = "sky-button"): string {
  return `<button type="button" class="${cls}" data-intent="${intent}" ${extra}>${esc(label)}</button>`;
}

function iconButton(icon: string, label: string, intent: string, extra = ""): string {
  return `<button type="button" class="icon-button" data-intent="${intent}" aria-label="${esc(label)}" title="${esc(label)}" ${extra}>${ICONS[icon] ?? ""}</button>`;
}

function glass(inner: string, cls = ""): string {
  return `<span class="glass ${cls}">${inner}</span>`;
}

function muted(text: string): string {
  return `<span class="glass__muted">${esc(text)}</span>`;
}

function avatar(session: number): string {
  return `<span class="weaver-avatar" style="--c:${colourOf(session)}" aria-hidden="true">${esc(initialOf(session))}</span>`;
}

/** Animated bars next to someone who is talking right now. */
function voiceBars(session: number): string {
  return player(session)?.talking
    ? `<span class="voice-bars" aria-hidden="true"><i style="height:6px"></i><i style="height:13px"></i><i style="height:9px"></i><i style="height:12px"></i></span>`
    : "";
}

/** A player's shape with their initial, for lists. */
function shapeIcon(session: number, size = 22): string {
  const p = player(session);
  return `<svg class="people__shape" viewBox="-16 -16 32 32" style="--c:${colourOf(session)};width:${size}px;height:${size}px" aria-hidden="true"><path class="star__body" d="${shapePath(p?.shape ?? 0, 11)}"/><text class="star__initial" y="1" style="font-size:11px">${esc(initialOf(session))}</text></svg>`;
}

function secondsLeft(): number {
  const r = view?.round;
  if (!r) return 0;
  return Math.max(0, Math.ceil((r.msLeft - (performance.now() - viewAt)) / 1000));
}

function timerRing(): string {
  const r = view?.round;
  if (!r) return "";
  const secs = secondsLeft();
  const left = r.dur > 0 ? secs / r.dur : 0;
  const c = 2 * Math.PI * 22;
  return `<div class="timer" role="timer" aria-label="${esc(tr("say.timer", { secs }))}">
    <svg viewBox="0 0 52 52"><circle class="timer__track" cx="26" cy="26" r="22"/><circle class="timer__left" id="timer-left" cx="26" cy="26" r="22" stroke-dasharray="${c.toFixed(1)}" stroke-dashoffset="${(c * (1 - left)).toFixed(1)}"/></svg>
    <span class="timer__text" id="timer-text">${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}</span></div>`;
}

/** The page's own tools. The host frame has pop-out, full screen, leave and mute, but not
 *  these; the volume slider only sits in the side column, where there is room for it. */
function tools(v: View, volume = false): string {
  const sound = v.sound.muted
    ? iconButton("muted", tr("menu.unmute"), "unmute")
    : iconButton("sound", tr("menu.mute"), "mute");
  const places =
    v.place === "stage"
      ? iconButton("side", tr("menu.side"), "place-side") +
        iconButton("popout", tr("menu.popout"), "place-dialog")
      : iconButton("stage", tr("menu.stage"), "place-stage");
  const slider = volume
    ? `<input type="range" min="0" max="100" step="5" value="${v.sound.volume}" id="volume" aria-label="${esc(tr("menu.volume"))}" ${v.sound.muted ? "disabled" : ""}/>`
    : "";
  return `<span class="tools">${iconButton("help", tr("menu.rules"), "rules")}${sound}${slider}${places}</span>`;
}

function rules(): string {
  const items = [1, 2, 3, 4, 5, 6].map((n) => `<li>${esc(tr(`rules.${n}`))}</li>`).join("");
  return `<div class="overlay" role="dialog" aria-modal="true" aria-labelledby="rules-title">
    <div class="glass glass--box"><h2 id="rules-title" class="figure-name">${esc(tr("rules.title"))}</h2><ol>${items}</ol>
    ${button(tr("menu.close"), "rules-close", "autofocus")}</div></div>`;
}

/** One star as drawn on the loom; `r` is the body radius in the SVG's own units. */
function starGlyph(session: number, r: number, label: string, extra = ""): string {
  const p = player(session);
  return `<g class="star${session === view?.me ? " star--you" : ""}" style="--c:${colourOf(session)}" ${extra}>
    <circle class="star__glow" r="${(r * 2.3).toFixed(1)}"/>
    ${p?.talking ? `<circle class="star__talk" r="${(r * 1.6).toFixed(1)}"/>` : ""}
    <circle class="star__focus" r="${(r * 1.65).toFixed(1)}"/>
    <path class="star__body" d="${shapePath(p?.shape ?? 0, r)}"/>
    <text class="star__initial" y="1" style="font-size:${(r * 0.9).toFixed(1)}px">${esc(initialOf(session))}</text>
    ${label ? `<text class="star__name" y="${(r * 2.65).toFixed(1)}" style="font-size:${r * 0.92}px">${esc(label)}</text>` : ""}</g>`;
}

/** Threads with a soft glow and a bright point at every vertex. */
function figureArt(lines: Pt[][], map: (p: Pt) => Pt, dot = 3): string {
  const seen = new Set<string>();
  let threads = "";
  let points = "";
  for (const line of lines) {
    const pts = line.map(map);
    for (let k = 1; k < pts.length; k++) {
      const a = pts[k - 1];
      const b = pts[k];
      if (!a || !b) continue;
      const d = `x1="${a[0].toFixed(1)}" y1="${a[1].toFixed(1)}" x2="${b[0].toFixed(1)}" y2="${b[1].toFixed(1)}"`;
      threads += `<line class="thread-glow" ${d}/><line class="thread" ${d}/>`;
    }
    for (const [x, y] of pts) {
      const key = `${x.toFixed(0)},${y.toFixed(0)}`;
      if (seen.has(key)) continue;
      seen.add(key);
      points += `<circle cx="${x.toFixed(1)}" cy="${y.toFixed(1)}" r="${(dot * 2.4).toFixed(1)}" fill="var(--thread)" opacity="0.25" filter="url(#soft)"/><circle cx="${x.toFixed(1)}" cy="${y.toFixed(1)}" r="${dot}" fill="var(--ink)"/>`;
    }
  }
  return threads + points;
}

/** Maps a figure's lines into a box, keeping its proportions. */
function fitTo(lines: Pt[][], cx: number, cy: number, w: number, h: number): (p: Pt) => Pt {
  const all = lines.flat();
  const xs = all.map((p) => p[0]);
  const ys = all.map((p) => p[1]);
  const minX = Math.min(...xs);
  const minY = Math.min(...ys);
  const bw = Math.max(1, Math.max(...xs) - minX);
  const bh = Math.max(1, Math.max(...ys) - minY);
  const s = Math.min(w / bw, h / bh);
  return ([x, y]) => [cx + (x - minX - bw / 2) * s, cy + (y - minY - bh / 2) * s];
}

const SOFT = `<defs><filter id="soft" x="-100%" y="-100%" width="300%" height="300%"><feGaussianBlur stdDeviation="6"/></filter></defs>`;

/** The lobby's picture: a sample constellation (Two Moths) and tonight's stars in a row. */
const MOTHS: Pt[][] = [
  [
    [422, 180],
    [467, 118],
    [512, 180],
    [467, 242],
    [422, 180],
  ],
  [
    [568, 180],
    [613, 118],
    [658, 180],
    [613, 242],
    [568, 180],
  ],
  [
    [512, 180],
    [568, 180],
  ],
];

// ----- Screens -----

function lobby(v: View): string {
  const me = v.me;
  const lamp = v.lamplighter === me;
  const seated = v.players.some((p) => p.s === me);
  const lampName = v.lamplighter === null ? "" : nameOf(v.lamplighter);
  const n = v.players.length;
  const gap = n > 1 ? Math.min(80, 560 / (n - 1)) : 0;
  const x0 = 365 - (gap * (n - 1)) / 2;
  const stars = v.players
    .map((p, i) =>
      starGlyph(
        p.s,
        12,
        p.s === me ? `${p.name} (${tr("lobby.you")})` : p.name,
        `transform="translate(${(x0 + i * gap).toFixed(1)},${i % 2 ? 306 : 290})"`,
      ),
    )
    .join("");
  const picture = `<svg class="loom" viewBox="0 0 730 558" aria-hidden="true">${SOFT}<g opacity="0.85">${figureArt(MOTHS, (p) => p)}</g>${stars}</svg>`;
  const people = v.players
    .map((p) => {
      const state =
        p.s === v.lamplighter
          ? tr("lobby.opened")
          : p.s === me
            ? tr("lobby.joined-you")
            : tr("lobby.joined");
      return `<li>${shapeIcon(p.s)}<span>${esc(p.name)}</span><span class="people__state people__state--ok">${esc(state)}</span></li>`;
    })
    .join("");
  const watching = v.spectators
    .map(
      (name) =>
        `<li><svg class="people__shape" viewBox="-16 -16 32 32" aria-hidden="true"></svg><span>${esc(name)}</span><span class="people__state">${esc(tr("lobby.watching"))}</span></li>`,
    )
    .join("");
  const seg = (intent: string, options: [string, string, boolean][]): string =>
    `<span class="seg" role="group">${options
      .map(
        ([value, label, on]) =>
          `<button type="button" data-intent="${intent}" data-value="${value}" aria-pressed="${on}" ${lamp ? "" : "disabled"}>${esc(label)}</button>`,
      )
      .join("")}</span>`;
  const weave = seg(
    "weave",
    [60, 90, 120].map((s) => [String(s), tr("lobby.seconds", { secs: s }), v.settings.weave === s]),
  );
  const skySeg = seg("mixed", [
    ["0", tr("lobby.calm"), !v.settings.mixed],
    ["1", tr("lobby.mixed"), v.settings.mixed],
  ]);
  const rounds = Math.min(8, Math.max(n, 1));
  // A round is the weave plus about 40 s for sealing, dawn and getting ready.
  const minutes = Math.max(1, Math.round((rounds * (v.settings.weave + 40)) / 60));
  let actions: string;
  if (!seated) {
    actions = button(tr("lobby.play"), "play", "", "sky-button sky-button--gold");
  } else if (lamp) {
    actions = `<button type="button" class="sky-button sky-button--gold" data-intent="begin" ${n < 2 ? "disabled" : ""}>${ICONS.loom}${esc(tr("lobby.light"))}</button>${n < 2 ? glass(muted(tr("lobby.need-two"))) : ""}`;
  } else {
    actions = `<span class="sky-button sky-button--gold" role="status">${ICONS.loom}${esc(tr("lobby.waiting-light", { name: lampName }))}</span>${button(tr("lobby.watch-instead"), "watch")}`;
  }
  const past = v.past.length
    ? `<h3>${esc(tr("lobby.past"))}</h3><ul class="past">${v.past
        .slice(0, 3)
        .map(
          (p) =>
            `<li>${esc(tr("lobby.past-item", { name: p.name, count: p.count, res: p.team }))}</li>`,
        )
        .join("")}</ul>`
    : "";
  return `<div class="sky-layout">
    <div style="position:relative">
      ${picture}
      <div class="hud hud--top-left lobby-title">
        <div class="wordmark"><h1 class="wordmark__name">Sky<span>loom</span></h1><p class="wordmark__tag">${esc(tr("lobby.tagline"))}</p></div>
      </div>
      <div class="hud lobby-how">${glass(`<span><b>${esc(tr("lobby.how"))}</b> ${esc(tr("lobby.how-text"))}</span>`, "glass--box")}</div>
      <div class="hud lobby-actions">${actions}</div>
    </div>
    <aside class="sky-col sky-col--scroll" aria-label="${esc(tr("lobby.side"))}">
      <h3>${esc(tr("lobby.stars", { count: n }))}</h3>
      <ul class="people">${people}${watching}</ul>
      <h3>${esc(tr("lobby.night", { name: lampName }))}</h3>
      <div class="setting">${esc(tr("lobby.round-length"))}${weave}</div>
      <div class="setting">${esc(tr("lobby.skies"))}${skySeg}<span class="setting__hint">${esc(tr("lobby.skies-hint"))}</span></div>
      <span class="setting__hint">${esc(tr("lobby.rounds", { mins: minutes }))}</span>
      ${past}
      <div class="col-tools">${tools(v, true)}</div>
    </aside></div>`;
}

function twistHint(v: View, r: Round): string {
  const me = v.me;
  const weaving = r.weaver === me || r.twin === me;
  const show =
    (r.twist === "fog" && !weaving) ||
    (r.twist === "drift" && !weaving) ||
    (r.twist === "mirror" && weaving) ||
    (r.twist === "twin" && weaving);
  return show ? glass(muted(tr(`twist.${r.twist}-hint`))) : "";
}

function skyCard(r: Round, card: SkyCard): string {
  const map = fitTo(card.lines.length ? card.lines : [card.points], 0, 0, 220, 270);
  const art = figureArt(card.lines, map, 4);
  // The sampled points the Threaders' stars should land on, when they are not vertices.
  const extra = card.points
    .map(map)
    .map(
      ([x, y]) =>
        `<circle cx="${x.toFixed(1)}" cy="${y.toFixed(1)}" r="2.5" fill="var(--ink)" opacity="0.7"/>`,
    )
    .join("");
  const max = Math.min(r.stars.length, 10);
  return `<aside class="sky-card" aria-label="${esc(tr("card.label", { figure: card.name }))}">
    <span class="sky-card__label">${esc(tr("card.only-you"))}</span>
    <svg class="sky-card__art" viewBox="-130 -160 260 320" aria-hidden="true">${SOFT}${art}${extra}</svg>
    <span class="sky-card__name figure-name">${esc(card.name)}</span>
    <p class="sky-card__rule">${esc(tr(card.mirror ? "twist.mirror-hint" : "card.rule"))}</p>
    <div class="sky-card__actions">${glass(`<span class="key">1–${max % 10 === 0 && max > 9 ? 0 : max}</span>${esc(tr("card.pluck"))}`)}${
      r.weaver === view?.me && view.phase === "weave"
        ? button(tr("card.tie-off"), "tie-off", "", "sky-button sky-button--gold")
        : ""
    }</div>
  </aside>`;
}

function roundChips(r: Round, sky: boolean): string {
  const round = glass(
    `${muted(tr("hud.round-word"))} <b>${r.no}</b> ${muted(tr("hud.of", { of: r.of }))}`,
  );
  const twist = sky
    ? glass(`${muted(tr("hud.sky-word"))} <b>${esc(tr(`twist.${r.twist}`))}</b>`)
    : "";
  return round + twist;
}

function weaverChip(v: View, r: Round): string {
  if (r.weaver === v.me) return glass(`${avatar(r.weaver)}<b>${esc(tr("hud.you-weaver"))}</b>`);
  return glass(
    `${avatar(r.weaver)}<span><b>${esc(nameOf(r.weaver))}</b> ${muted(tr("hud.is-weaving"))}</span>${voiceBars(r.weaver)}`,
  );
}

function timerChip(): string {
  const firstLight = (view?.round?.msLeft ?? 0) - (performance.now() - viewAt) <= 10_000;
  return `${glass(esc(tr(firstLight ? "hud.first-light" : "hud.dawn-in")), "glass__muted")}${timerRing()}`;
}

function weaveHud(v: View, r: Round): string {
  const me = v.me;
  const weaving = r.weaver === me || r.twin === me;
  const card = weaving && r.card ? skyCard(r, r.card) : "";
  const top = `<div class="hud hud--top-left">${weaverChip(v, r)}${roundChips(r, !weaving || r.twist !== "calm")}</div>`;
  const right =
    v.phase === "weave"
      ? `<div class="hud hud--top-right${card ? " hud--beside-card" : ""}">${card ? "" : tools(v)}${timerChip()}${card ? tools(v) : ""}</div>`
      : `<div class="hud hud--top-right">${tools(v)}</div>`;
  let center = "";
  if (v.phase === "seal") {
    center = `<div class="hud hud--center-top" style="top:45%">${glass(esc(tr("hud.reading", { name: nameOf(r.weaver) })))}</div>`;
  } else if (v.phase === "dawn") {
    center = `<div class="hud hud--center-top">${glass(esc(tr("dawn.title")))}</div>`;
  }
  let bottom: string;
  if (v.role === "threader") {
    const own = r.stars.filter((s) => s[0] === me).length;
    bottom = glass(
      `${shapeIcon(me, 20)}<b>${esc(tr(own > 1 ? "hud.your-stars" : "hud.your-star"))}</b>
       <span class="keys"><span class="key">${esc(tr("hud.arrow-keys"))}</span>${muted(tr("hud.keys-move"))}<span class="key">Shift</span>${muted(tr("hud.keys-fine"))}<span class="key">Enter</span>${muted(tr("hud.keys-settle"))}<span class="key">L</span>${muted(tr("hud.keys-where"))}</span>`,
    );
  } else if (weaving) {
    bottom = glass(
      `${muted(tr("hud.tip-a"))} <b>${esc(tr("hud.tip-b"))}</b> ${muted(tr("hud.tip-c"))}`,
    );
  } else {
    bottom = `<label class="glass glass--box guess">${esc(tr("guess.title"))}<input id="guess" maxlength="32" autocomplete="off"/>${muted(tr("guess.hint"))}</label>`;
  }
  const live = weaving
    ? ""
    : glass(
        `${muted(tr("hud.live"))} <b>${esc(tr(v.live ? "hud.relay" : "hud.server"))}</b>`,
        "live-chip",
      );
  return `${top}${right}${center}<div class="hud hud--stack">${twistHint(v, r)}</div>${card}
    <div class="hud hud--bottom${card ? " hud--beside-card" : ""}">${bottom}${live}</div>`;
}

function nextControls(v: View): string {
  const me = v.me;
  if (v.canEnd) return button(tr("next.tapestry"), "tapestry", "", "sky-button sky-button--gold");
  if (v.next === null) {
    return glass(
      esc(tr("next.night-over", { name: v.lamplighter === null ? "" : nameOf(v.lamplighter) })),
    );
  }
  const name = v.next === me ? `${nameOf(me)} (${tr("lobby.you")})` : nameOf(v.next);
  const chip = glass(`${muted(tr("next.weaver-label"))} <b>${esc(name)}</b>`);
  return v.next === me
    ? chip + button(tr("next.ready"), "weave-ready", "", "sky-button sky-button--gold")
    : chip;
}

function award(label: string, body: string): string {
  return `<li class="glass glass--box">${muted(label)}<span>${body}</span></li>`;
}

function revealHud(v: View, r: Round): string {
  const rv = r.reveal;
  if (r.void !== null || !rv) {
    return `<div class="hud hud--center-top"><div class="reveal-title">
      <span class="reveal-title__name figure-name">${esc(tr("void.title"))}</span>
      <span class="reveal-title__note">${esc(tr(`void.${r.void ?? "stalled"}`))}</span></div></div>
      <div class="hud hud--top-right">${tools(v)}</div>
      <div class="hud hud--bottom"><span></span><span class="hud-row">${nextControls(v)}</span></div>`;
  }
  const outcome = rv.gold ? "reveal.gold" : rv.woven ? "reveal.woven" : "reveal.loose";
  // A player with several stars is named for their best (truest) or worst (loosest) one.
  const pcts = (s: number): number[] => rv.stars.filter((x) => x[0] === s).map((x) => x[4]);
  const awards: string[] = [];
  if (rv.truest !== null) {
    const pct = Math.max(...pcts(rv.truest));
    const note = rv.truestAt ? tr("reveal.truest-note", { pct, where: rv.truestAt }) : `${pct}%`;
    awards.push(award(tr("reveal.truest"), `<b>${esc(nameOf(rv.truest))}</b> · ${esc(note)}`));
  }
  if (rv.loosest !== null && rv.loosest !== rv.truest) {
    const note = rv.loosestLost ?? `${Math.min(...pcts(rv.loosest))}%`;
    awards.push(award(tr("reveal.loosest"), `<b>${esc(nameOf(rv.loosest))}</b> · ${esc(note)}`));
  }
  if (rv.bonus > 0) {
    const tied = tr("reveal.tied-off-spare", { secs: rv.spare ?? 0, bonus: rv.bonus });
    awards.push(award(tr("reveal.weaver"), `<b>${esc(nameOf(r.weaver))}</b> · ${esc(tied)}`));
  }
  const guess =
    rv.guess !== null
      ? `<span class="reveal-title__note">${esc(tr("reveal.your-guess", { text: rv.guess }))}</span>`
      : "";
  const verify =
    rv.verify === "verified"
      ? glass(
          `${ICONS.verified}<span><b>${esc(tr("verify.ok"))}</b> ${muted(`· ${tr("verify.ok-detail", { name: nameOf(r.weaver) })}`)}</span>`,
        )
      : glass(esc(tr(`verify.${rv.verify}`)));
  return `<div class="hud hud--center-top"><div class="reveal-title">
      <span class="reveal-title__note">${esc(tr("dawn.title"))}</span>
      <span class="reveal-title__name figure-name">${esc(rv.figure ?? "")}</span>
      <span class="reveal-title__score"><span>${rv.resonance}%</span> ${esc(tr("reveal.resonance-word"))} · ${esc(tr(outcome))}</span>${guess}
    </div></div>
    <ul class="reveal-awards hud">${awards.join("")}</ul>
    <div class="hud hud--top-left">${glass(`${avatar(r.weaver)}<span><b>${esc(nameOf(r.weaver))}</b> ${muted(tr("reveal.wove-this"))}</span>${voiceBars(r.weaver)}`)}</div>
    <div class="hud hud--top-right">${tools(v)}</div>
    <div class="hud hud--bottom">${verify}<span class="hud-row">${nextControls(v)}</span></div>`;
}

/** Compact place (side panel): the stars small, then who's who and the guess. */
function compactHud(v: View, r: Round): string {
  const watching = v.role === "spectator" || v.role === "player";
  const banner =
    watching && v.phase === "weave"
      ? `<div class="sky-banner" style="top:12px">${glass(
          `<span class="banner-title">${ICONS.eye}<b>${esc(tr("watch.title", { no: r.no }))}</b></span>${muted(tr("watch.text"))}`,
          "glass--box banner-box",
        )}</div>`
      : "";
  const locked = new Set(r.stars.filter((s) => s[4]).map((s) => s[0]));
  const owners = [...new Set(r.stars.map((s) => s[0]))];
  const state = (s: number): string => {
    const parts: string[] = [];
    if (s === r.weaver) parts.push(tr("who.weaver"));
    if (player(s)?.talking) parts.push(tr("who.talking"));
    if (!parts.length && s !== r.weaver)
      parts.push(tr(locked.has(s) ? "who.settled" : "who.moving"));
    return parts.join(" · ");
  };
  // Who is doing something comes first; the rest only by name, so the list stays short.
  const people = [r.weaver, ...owners.filter((s) => s !== r.weaver)];
  people.sort(
    (a, b) =>
      Number(b === r.weaver) - Number(a === r.weaver) ||
      Number(!!player(b)?.talking) - Number(!!player(a)?.talking),
  );
  const listed = people.slice(0, 6);
  const rest = people.slice(6).map(nameOf).sort();
  const who = glass(
    `<span class="box-title">${esc(tr("hud.who"))}</span><ul class="people">${listed
      .map(
        (s) =>
          `<li>${shapeIcon(s)}<span>${esc(nameOf(s))}</span><span class="people__state">${esc(state(s))}</span></li>`,
      )
      .join(
        "",
      )}</ul>${rest.length ? `<span class="glass__muted rest">+ ${esc(rest.join(", "))}</span>` : ""}`,
    "glass--box who-box",
  );
  const guess =
    v.role === "spectator" && v.phase === "weave"
      ? `<label class="glass glass--box guess-box"><span class="box-title">${esc(tr("guess.title"))}</span><span>${esc(tr("guess.hint"))}</span><span class="name-input"><input id="guess" maxlength="32" autocomplete="off" aria-label="${esc(tr("guess.title"))}"/><span id="guess-count">0 / 32</span></span></label>`
      : "";
  const timer = v.phase === "weave" ? timerRing() : "";
  const weaving = r.weaver === v.me || r.twin === v.me;
  const card = weaving && r.card && v.phase === "weave" ? skyCard(r, r.card) : "";
  if (v.phase === "reveal" || v.phase === "void") return revealHud(v, r);
  return `${banner}${card}<div class="sky-banner compact-below">
    <span class="hud-row">${weaverChip(v, r)}${timer}</span>
    ${v.phase === "weave" ? who + guess : ""}
    <span class="hud-row">${button(tr("watch.open-big"), "place-stage")}${tools(v)}</span>
  </div>`;
}

function results(v: View): string {
  const res = v.results;
  if (!res) return "";
  const lamp = v.lamplighter === v.me;
  const cols = 4;
  const cellW = 730 / cols;
  const rows = Math.max(1, Math.ceil(res.rounds.length / cols));
  const cellH = Math.min(240, 540 / rows);
  const tiles = res.rounds
    .map((r, i) => {
      const cx = cellW * (i % cols) + cellW / 2;
      const top = 20 + cellH * Math.floor(i / cols);
      const gold = r.resonance !== null && r.resonance >= 90;
      const frayed = r.resonance === null || r.resonance < 60;
      const art = r.lines
        ? figureArt(
            r.lines,
            fitTo(r.lines, cx, top + (cellH - 50) / 2, 130, Math.min(125, cellH - 90)),
          )
        : "";
      const score =
        r.resonance === null
          ? tr("results.clouds")
          : `${r.resonance}% · ${r.weaver}${gold ? ` ✦ ${tr("results.gold")}` : frayed ? ` · ${tr("results.frayed")}` : ""}`;
      return `<g class="${gold ? "const--gold" : frayed ? "const--frayed" : ""}">${art}
        <text class="const-label figure-name" x="${cx.toFixed(0)}" y="${(top + cellH - 30).toFixed(0)}">${esc(r.figure ?? tr("results.clouds"))}</text>
        <text class="const-score" x="${cx.toFixed(0)}" y="${(top + cellH - 14).toFixed(0)}">${esc(score)}</text></g>`;
    })
    .join("");
  const woven = res.rounds.filter((r) => r.resonance !== null && r.resonance >= 60).length;
  const h = res.honours;
  const best = res.rounds
    .filter((r) => r.resonance !== null)
    .reduce<
      (typeof res.rounds)[number] | null
    >((a, r) => (a === null || (r.resonance ?? 0) > (a.resonance ?? 0) ? r : a), null);
  const honour = (key: string, name: string | null, detail: string): string =>
    name ? `<li><b>${esc(tr(key))} · ${esc(name)}</b><span>${esc(detail)}</span></li>` : "";
  const title = res.name ? tr("results.over-named", { name: res.name }) : tr("results.over");
  return `<div class="sky-layout"><div style="position:relative">
      <svg class="loom" viewBox="0 0 730 558" role="img" aria-label="${esc(tr("results.sky-label", { woven, count: res.rounds.length }))}">${SOFT}${tiles}</svg></div>
    <aside class="sky-col sky-col--scroll" aria-label="${esc(tr("results.side"))}">
      <h3>${esc(title)}</h3>
      <div class="big-number">${res.team ?? 0}%<br/><small>${esc(tr("results.team-woven", { woven, count: res.rounds.length }))}</small></div>
      <h3><label for="sky-name">${esc(tr("results.name-this"))}</label></h3>
      <span class="name-input"><input id="sky-name" maxlength="40" value="${esc(res.name)}" placeholder="${esc(tr("results.name-placeholder"))}"/><span id="name-count">${[...res.name].length} / 40</span></span>
      <h3>${esc(tr("results.honours"))}</h3>
      <ul class="awards">
        ${honour("results.silver", h.silver, best ? tr("results.silver-detail", { figure: best.figure ?? "", res: best.resonance ?? 0 }) : "")}
        ${honour("results.truest", h.truest, tr("results.truest-detail", { n: h.truestCount, of: h.truestOf }))}
        ${honour("results.wanderer", h.wanderer, tr("results.wanderer-detail"))}
      </ul>
      <div class="col-actions">
        ${lamp ? button(tr(res.summaryPosted ? "results.posted" : "results.post"), "post-summary", res.summaryPosted ? "disabled" : "") : ""}
        ${lamp ? button(tr("results.close"), "close-night") : ""}
      </div>
      <div class="col-tools">${tools(v, true)}</div>
    </aside></div>`;
}

function render(): void {
  const v = view;
  if (!v) return;
  document.documentElement.dataset.theme = v.theme === "light" ? "light" : "dark";
  const compact = v.place !== "stage" || sky.clientWidth < 640;
  sky.className = `sky${compact ? " sky--compact" : ""}${reduceMotion() ? " reduce-motion" : ""}`;
  const focusedId = document.activeElement?.id ?? "";
  const guess = (document.getElementById("guess") as HTMLInputElement | null)?.value ?? "";
  const skyName = (document.getElementById("sky-name") as HTMLInputElement | null)?.value;
  let hud = "";
  const r = v.round;
  if (v.phase === "none") hud = "";
  else if (v.phase === "lobby") hud = lobby(v);
  else if (v.phase === "between")
    hud = `<div class="hud hud--center-top" style="top:40%">${glass(`<span><b>${esc(tr("hud.spectating"))}</b> ${esc(tr("hud.join-next"))}</span>`)}</div><div class="hud hud--top-right">${tools(v)}</div>`;
  else if (v.phase === "results") hud = results(v);
  else if (r && compact) hud = compactHud(v, r);
  else if (r && (v.phase === "reveal" || v.phase === "void")) hud = revealHud(v, r);
  else if (r) hud = weaveHud(v, r);
  const loomShown = v.phase !== "lobby" && v.phase !== "results" && v.phase !== "none";
  sky.innerHTML = `<div class="sky__weave"></div>${dust()}
    <div class="sky__dawn" id="dawn"></div><div class="sky__horizon"></div>
    ${loomShown ? loomSvg(v) : ""}
    <div id="hud">${hud}</div>
    ${toast && toast.until > performance.now() ? toastMarkup(toast.text) : ""}
    ${showRules ? rules() : ""}`;
  const g = document.getElementById("guess") as HTMLInputElement | null;
  if (g) g.value = guess;
  const n = document.getElementById("sky-name") as HTMLInputElement | null;
  if (n && skyName !== undefined && focusedId === "sky-name") n.value = skyName;
  updateCounters();
  if (focusedId) document.getElementById(focusedId)?.focus();
  updateDawn();
}

/** The pluck toast sits just left of the plucked star, or at the top when it is not drawn. */
function toastMarkup(text: string): string {
  const svg = document.getElementById("loom") as SVGSVGElement | null;
  const at = toastAt === null ? undefined : shown.get(toastAt);
  const ctm = svg?.getScreenCTM();
  const box = sky.getBoundingClientRect();
  if (svg && at && ctm && !sky.classList.contains("sky--compact")) {
    const pt = new DOMPoint(at[0], at[1]).matrixTransform(ctm);
    const x = pt.x - box.left;
    const y = pt.y - box.top;
    // Left of the star when there is room for the text, otherwise right of it.
    const left = x > 240;
    return `<span class="toast" style="left:${(left ? x - 40 : x + 40).toFixed(0)}px;top:${y.toFixed(0)}px;transform:translate(${left ? "-100%" : "0"},-50%)">${esc(text)}</span>`;
  }
  return `<span class="toast" style="left:50%;top:${sky.classList.contains("sky--compact") ? 160 : 84}px;transform:translateX(-50%)">${esc(text)}</span>`;
}

function updateCounters(): void {
  for (const [input, out, max] of [
    ["guess", "guess-count", 32],
    ["sky-name", "name-count", 40],
  ] as const) {
    const i = document.getElementById(input) as HTMLInputElement | null;
    const o = document.getElementById(out);
    if (i && o) o.textContent = `${[...i.value].length} / ${max}`;
  }
}

let dustCache = "";
function dust(): string {
  if (!dustCache) {
    // A fixed pseudo-random field, the same every time.
    let seed = 7;
    const rnd = (): number => (seed = (seed * 16807) % 2147483647) / 2147483647;
    const dots: string[] = [];
    for (let i = 0; i < 90; i++) {
      const cls = i % 3 === 0 ? ' class="twinkle"' : "";
      dots.push(
        `<circle${cls} cx="${(rnd() * LOOM_W).toFixed(0)}" cy="${(rnd() * LOOM_H).toFixed(0)}" r="${(0.8 + rnd() * 1.6).toFixed(1)}" style="animation-delay:${(rnd() * 3).toFixed(1)}s"/>`,
      );
    }
    dustCache = `<svg class="sky__dust" viewBox="0 0 ${LOOM_W} ${LOOM_H}" preserveAspectRatio="xMidYMid slice" aria-hidden="true">${dots.join("")}</svg>`;
  }
  return dustCache;
}

// ----- The loom -----

function starKey(owner: number, index: number): string {
  return `${owner}-${index}`;
}

function loomSvg(v: View): string {
  const r = v.round;
  if (!r) return "";
  const rv = v.phase === "reveal" ? r.reveal : null;
  let figure = "";
  if (rv) {
    const ghost = rv.ghost
      .map(
        (l) =>
          `<polyline class="ghost-line fade-in" points="${l.map((p) => p.join(",")).join(" ")}"/>`,
      )
      .join("");
    const targets = rv.targets
      .map((p) => `<circle class="ghost-point fade-in" cx="${p[0]}" cy="${p[1]}" r="10"/>`)
      .join("");
    const offsets = rv.stars
      .map((s, k) => {
        const t = rv.targets[k];
        return t
          ? `<line class="offset" x1="${s[2]}" y1="${s[3]}" x2="${t[0]}" y2="${t[1]}"/>`
          : "";
      })
      .join("");
    figure = `<g aria-hidden="true">${ghost}${targets}${offsets}${threads(rv)}</g>`;
  }
  const label = esc(tr("loom.label"));
  const withCard = v.phase === "weave" && r.card !== null && (r.weaver === v.me || r.twin === v.me);
  return `<svg class="loom${withCard ? " loom--with-card" : ""}" id="loom" viewBox="0 0 ${LOOM_W} ${LOOM_H}" preserveAspectRatio="xMidYMid meet" role="group" aria-label="${label}">
    <defs><filter id="soft" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="6"/></filter></defs>
    ${figure}<g id="stars">${starsMarkup(v, r)}</g><g id="ripples"></g></svg>`;
}

/** Gold threads between stars that follow each other along the figure's lines. */
function threads(rv: Reveal): string {
  const order = rv.stars.map((s, k) => ({ s, j: rv.order[k] ?? k, t: rv.targets[k] }));
  order.sort((a, b) => a.j - b.j);
  const gaps: number[] = [];
  for (let k = 1; k < order.length; k++) {
    const a = order[k - 1]?.t;
    const b = order[k]?.t;
    if (a && b) gaps.push(Math.hypot(a[0] - b[0], a[1] - b[1]));
  }
  const sorted = [...gaps].sort((a, b) => a - b);
  const median = sorted[Math.floor(sorted.length / 2)] ?? 0;
  let out = "";
  for (let k = 1; k < order.length; k++) {
    const a = order[k - 1];
    const b = order[k];
    const gap = gaps[k - 1] ?? 0;
    if (!a || !b || gap > median * 1.7) continue;
    const d = `M${a.s[2]},${a.s[3]}L${b.s[2]},${b.s[3]}`;
    out += `<path class="thread-glow" d="${d}"/><path class="thread thread-run" d="${d}" style="animation-delay:${(k * 0.12).toFixed(2)}s"/>`;
  }
  return out;
}

/** Screen pixels per loom unit, from the box the loom is laid out in (see the stylesheet). */
function loomScale(v: View, r: Round): number {
  const compact = sky.classList.contains("sky--compact");
  const withCard = v.phase === "weave" && r.card !== null && (r.weaver === v.me || r.twin === v.me);
  let w = sky.clientWidth;
  let h = sky.clientHeight;
  if (compact) h = 220;
  else if (withCard) w -= 330;
  return Math.max(0.05, Math.min(w / LOOM_W, h / LOOM_H));
}

function starK(v: View, r: Round): number {
  return (sky.classList.contains("sky--compact") ? 8 : 12.4) / (20 * loomScale(v, r));
}

function starsMarkup(v: View, r: Round): string {
  const me = v.me;
  const weaving = (r.weaver === me || r.twin === me) && v.phase === "weave";
  const revealed = new Map<string, number>();
  if (v.phase === "reveal" && r.reveal) {
    for (const s of r.reveal.stars) revealed.set(starKey(s[0], s[1]), s[4]);
  }
  // Stars keep the same size on screen however small the loom is drawn: the mockups' 12 px
  // body on the stage and 8 px in the narrow side panel.
  const compact = sky.classList.contains("sky--compact");
  const k = starK(v, r);
  const names = !compact || v.phase !== "weave";
  const counts = new Map<number, number>();
  for (const s of r.stars) counts.set(s[0], (counts.get(s[0]) ?? 0) + 1);
  return r.stars
    .map(([owner, index, x, y]) => {
      const key = starKey(owner, index);
      if (!shown.has(key)) shown.set(key, [x, y]);
      const [sx, sy] = shown.get(key) ?? [x, y];
      const p = player(owner);
      const shape = p?.shape ?? 0;
      const mine = owner === me && v.role === "threader" && v.phase === "weave";
      const many = (counts.get(owner) ?? 1) > 1;
      const name =
        (owner === me ? `${nameOf(owner)} (${tr("lobby.you")})` : nameOf(owner)) +
        (many ? ` ${index + 1}` : "");
      const res = revealed.get(key);
      const label =
        owner === me
          ? tr("star.label-you", { shape: tr(`shape.${shape}`), n: index + 1 })
          : tr("star.label", { name: nameOf(owner), shape: tr(`shape.${shape}`), n: index + 1 });
      const left =
        (r.left.includes(owner) ? " star--left" : "") +
        (owner === me && !mine ? " star--mine" : "");
      const cls = mine
        ? "star star--you"
        : weaving
          ? "star star--other star--pluckable"
          : "star star--other";
      const focusable = mine || weaving ? `tabindex="0" role="button"` : `role="img"`;
      return `<g class="${cls}${left}" id="star-${key}" data-key="${key}" data-owner="${owner}" data-index="${index}" ${focusable} aria-label="${esc(label)}" transform="translate(${sx.toFixed(1)},${sy.toFixed(1)})" style="--c:${colourOf(owner)}">
        <circle class="star__glow" r="${40 * k}"/>
        ${p?.talking ? `<circle class="star__talk" r="${34 * k}"/>` : ""}
        <circle class="star__focus" r="${30 * k}"/>
        <path class="star__body" d="${shapePath(shape, 20 * k)}"/>
        <text class="star__initial" style="font-size:${18 * k}px">${esc(initialOf(owner))}</text>
        ${names ? `<text class="star__name" y="${50 * k}" style="font-size:${19 * k}px">${esc(name)}${res !== undefined ? ` · ${res}%` : ""}</text>` : ""}</g>`;
    })
    .join("");
}

function loomPoint(event: PointerEvent): Pt | null {
  const svg = document.getElementById("loom") as SVGSVGElement | null;
  const ctm = svg?.getScreenCTM();
  if (!svg || !ctm) return null;
  const p = new DOMPoint(event.clientX, event.clientY).matrixTransform(ctm.inverse());
  return [Math.round(p.x), Math.round(p.y)];
}

function clampPt([x, y]: Pt): Pt {
  return [Math.max(0, Math.min(LOOM_W, x)), Math.max(0, Math.min(LOOM_H, y))];
}

function moveOwn(index: number, to: Pt, force = false): void {
  const v = view;
  if (!v) return;
  const key = starKey(v.me, index);
  const p = clampPt(to);
  shown.set(key, p);
  target.set(key, p);
  ownMovedAt = performance.now();
  const el = document.getElementById(`star-${key}`);
  el?.setAttribute("transform", `translate(${p[0]},${p[1]})`);
  const now = performance.now();
  if (force || now - lastMoveSent >= MOVE_EVERY_MS) {
    lastMoveSent = now;
    send({ t: "move", i: index, x: p[0], y: p[1] });
  }
}

function ownPos(index: number): Pt | undefined {
  return view ? shown.get(starKey(view.me, index)) : undefined;
}

function areaOf([x, y]: Pt): string {
  const col = x < LOOM_W / 3 ? "left" : x > (2 * LOOM_W) / 3 ? "right" : "";
  const row = y < LOOM_H / 3 ? "top" : y > (2 * LOOM_H) / 3 ? "bottom" : "";
  const key = row && col ? `${row}-${col}` : row || col || "centre";
  return tr(`area.${key}`);
}

/** "Your star B: centre, 2 left of Ben's triangle" */
function whereAmI(index: number): void {
  const v = view;
  const r = v?.round;
  const pos = ownPos(index);
  if (!v || !r || !pos) return;
  let best: { d: number; s: StarTuple } | null = null;
  for (const s of r.stars) {
    if (s[0] === v.me) continue;
    const p = shown.get(starKey(s[0], s[1])) ?? [s[2], s[3]];
    const d = Math.hypot(p[0] - pos[0], p[1] - pos[1]);
    if (!best || d < best.d) best = { d, s: [s[0], s[1], p[0], p[1], s[4]] };
  }
  let near = "";
  if (best) {
    const dx = Math.round((pos[0] - best.s[2]) / 100);
    const dy = Math.round((pos[1] - best.s[3]) / 100);
    const parts: string[] = [];
    if (dx !== 0) parts.push(tr(dx < 0 ? "say.left" : "say.right", { n: Math.abs(dx) }));
    if (dy !== 0) parts.push(tr(dy < 0 ? "say.up" : "say.down", { n: Math.abs(dy) }));
    if (parts.length > 0) {
      near = tr("say.near", {
        offset: parts.join(tr("say.and")),
        name: nameOf(best.s[0]),
        shape: tr(`shape.${player(best.s[0])?.shape ?? 0}`),
      });
    }
  }
  announce(
    tr("say.where", {
      who: tr("say.your-star", { initial: initialOf(v.me) }),
      area: areaOf(pos),
      near,
    }),
  );
}

function animate(): void {
  const v = view;
  if (v?.round) {
    const now = performance.now();
    for (const [key, to] of target) {
      const from = shown.get(key) ?? to;
      const mine = v.me === Number(key.split("-")[0]);
      if (mine && now - ownMovedAt < 600) continue;
      const k = reduceMotion() ? 1 : 0.12;
      const next: Pt = [from[0] + (to[0] - from[0]) * k, from[1] + (to[1] - from[1]) * k];
      if (Math.abs(next[0] - from[0]) < 0.05 && Math.abs(next[1] - from[1]) < 0.05) continue;
      shown.set(key, next);
      document
        .getElementById(`star-${key}`)
        ?.setAttribute("transform", `translate(${next[0].toFixed(1)},${next[1].toFixed(1)})`);
    }
    updateTimer();
  }
  window.requestAnimationFrame(animate);
}

function updateDawn(): void {
  const r = view?.round;
  const el = document.getElementById("dawn");
  if (!el) return;
  let progress = 0;
  if (r && view?.phase === "weave" && r.dur > 0) progress = 1 - secondsLeft() / r.dur;
  else if (view && ["dawn", "reveal", "void"].includes(view.phase)) progress = 1;
  // The night's sky keeps a little of the last dawn.
  else if (view?.phase === "results") progress = 0.35;
  el.style.setProperty("--dawn-progress", progress.toFixed(3));
  document
    .querySelector<HTMLElement>(".sky__horizon")
    ?.style.setProperty("--dawn-progress", progress.toFixed(3));
}

let lastSecs = -1;
function updateTimer(): void {
  if (view?.phase !== "weave") return;
  const secs = secondsLeft();
  if (secs === lastSecs) return;
  lastSecs = secs;
  const text = document.getElementById("timer-text");
  if (text) text.textContent = `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;
  const ring = document.getElementById("timer-left");
  const r = view.round;
  if (ring && r && r.dur > 0) {
    const c = 2 * Math.PI * 22;
    ring.setAttribute("stroke-dashoffset", (c * (1 - secs / r.dur)).toFixed(1));
  }
  updateDawn();
  // The timer is spoken every 30 s and in the last 10 s.
  if (secs > 0 && secs !== lastAnnounced && (secs % 30 === 0 || secs === 10 || secs === 5)) {
    lastAnnounced = secs;
    announce(tr("say.timer", { secs }));
  }
  if (secs === 10) render();
}

function ripple(owner: number, index: number): void {
  if (reduceMotion()) return;
  const p = shown.get(starKey(owner, index));
  const g = document.getElementById("ripples");
  if (!p || !g) return;
  const c = document.createElementNS("http://www.w3.org/2000/svg", "circle");
  c.setAttribute("class", "ripple");
  c.setAttribute("cx", String(p[0]));
  c.setAttribute("cy", String(p[1]));
  c.setAttribute("r", "18");
  g.appendChild(c);
  window.setTimeout(() => c.remove(), 1300);
}

// ----- Messages from the plugin -----

function setStars(stars: StarTuple[]): void {
  const keys = new Set<string>();
  for (const [owner, index, x, y] of stars) {
    const key = starKey(owner, index);
    keys.add(key);
    target.set(key, [x, y]);
    if (!shown.has(key)) shown.set(key, [x, y]);
  }
  for (const key of [...target.keys()]) if (!keys.has(key)) target.delete(key);
}

window.addEventListener("message", (event: MessageEvent) => {
  if (event.source !== window.parent || typeof event.data !== "string") return;
  let msg: Incoming;
  try {
    msg = JSON.parse(event.data) as Incoming;
  } catch {
    return;
  }
  switch (msg.t) {
    case "strings":
      strings = msg.s;
      document.documentElement.lang = document.documentElement.lang || "en";
      render();
      break;
    case "view": {
      const roundChanged = view?.round?.no !== msg.round?.no;
      if (roundChanged) {
        shown.clear();
        target.clear();
      }
      view = msg;
      viewAt = performance.now();
      if (msg.round) {
        const stars =
          msg.phase === "reveal" && msg.round.reveal
            ? msg.round.reveal.stars.map((s): StarTuple => [s[0], s[1], s[2], s[3], true])
            : msg.round.stars;
        setStars(stars);
        if (msg.phase === "reveal") for (const [k, p] of target) shown.set(k, p);
      }
      render();
      break;
    }
    case "stars":
      if (view?.round?.no === msg.r && view.phase === "weave") {
        view.round.stars = msg.s;
        setStars(msg.s);
        const g = document.getElementById("stars");
        const count = g?.childElementCount ?? 0;
        if (g && count !== msg.s.length) g.innerHTML = starsMarkup(view, view.round);
      }
      break;
    case "talk": {
      const p = player(msg.s);
      if (p) {
        p.talking = msg.on;
        document.querySelectorAll<SVGGElement>(`[data-owner="${msg.s}"]`).forEach((g) => {
          const ring = g.querySelector(".star__talk");
          if (msg.on && !ring) {
            const c = document.createElementNS("http://www.w3.org/2000/svg", "circle");
            c.setAttribute("class", "star__talk");
            c.setAttribute("r", String(view?.round ? 34 * starK(view, view.round) : 34));
            g.insertBefore(c, g.children[1] ?? null);
          } else if (!msg.on) ring?.remove();
        });
        // The talking Weaver's voice bars and the who's who list live in the HUD.
        const hud = document.getElementById("hud");
        if (hud && view && !dragging && document.activeElement?.tagName !== "INPUT") render();
      }
      break;
    }
    case "pluck":
      ripple(msg.s, msg.i);
      toastAt = starKey(msg.s, msg.i);
      toast = {
        text:
          msg.s === view?.me
            ? tr("pluck.you", { name: nameOf(msg.by) })
            : tr("pluck.other", { name: nameOf(msg.by), target: nameOf(msg.s) }),
        until: performance.now() + 2500,
      };
      render();
      window.setTimeout(render, 2600);
      break;
    case "say":
      announce(msg.text);
      break;
  }
});

// ----- Input -----

sky.addEventListener("click", (event) => {
  const el = (event.target as Element).closest<HTMLElement>("[data-intent]");
  if (el) {
    const intent = el.dataset.intent ?? "";
    const v = view;
    switch (intent) {
      case "weave":
        if (v) send({ t: "settings", weave: Number(el.dataset.value), mixed: v.settings.mixed });
        break;
      case "mixed":
        if (v) send({ t: "settings", weave: v.settings.weave, mixed: el.dataset.value === "1" });
        break;
      case "play":
        send({ t: "seat", play: true });
        break;
      case "watch":
        send({ t: "seat", play: false });
        break;
      case "mute":
        send({ t: "mute", on: true });
        break;
      case "unmute":
        send({ t: "mute", on: false });
        break;
      case "rules":
        showRules = true;
        render();
        break;
      case "rules-close":
        showRules = false;
        render();
        break;
      case "place-side":
        send({ t: "place", to: "side-panel" });
        break;
      case "place-dialog":
        send({ t: "place", to: "dialog" });
        break;
      case "place-stage":
        send({ t: "place", to: "stage" });
        break;
      default:
        send({ t: intent });
    }
    return;
  }
  const star = (event.target as Element).closest<SVGGElement>(".star--pluckable");
  if (star) send({ t: "pluck", s: Number(star.dataset.owner), i: Number(star.dataset.index) });
});

sky.addEventListener("change", (event) => {
  const el = event.target as HTMLInputElement;
  if (el.id === "volume") send({ t: "volume", v: Number(el.value) });
  if (el.id === "guess") send({ t: "guess", text: el.value });
  // The sky's name is saved when the field is left or Enter is pressed.
  if (el.id === "sky-name") send({ t: "name", text: el.value });
});

sky.addEventListener("input", updateCounters);

sky.addEventListener("pointerdown", (event) => {
  const g = (event.target as Element).closest<SVGGElement>(".star--you");
  if (!g || !view) return;
  dragging = { key: g.dataset.key ?? "", star: Number(g.dataset.index) };
  g.setPointerCapture(event.pointerId);
  g.focus();
  event.preventDefault();
});

sky.addEventListener("pointermove", (event) => {
  if (!dragging) return;
  const p = loomPoint(event);
  if (p) moveOwn(dragging.star, p);
});

sky.addEventListener("pointerup", () => {
  if (!dragging) return;
  const index = dragging.star;
  const p = ownPos(index);
  if (p) moveOwn(index, p, true);
  send({ t: "settle", i: index });
  dragging = null;
});

document.addEventListener("keydown", (event) => {
  const v = view;
  if (!v) return;
  if (event.key === "Escape" && showRules) {
    showRules = false;
    render();
    event.preventDefault();
    return;
  }
  const active = document.activeElement as HTMLElement | null;
  if (active?.tagName === "INPUT") return;
  const g = active?.closest<SVGGElement>(".star--you");
  if (g) {
    const index = Number(g.dataset.index);
    const step = event.shiftKey ? FINE_STEP : STEP;
    const pos = ownPos(index);
    const moves: Record<string, Pt> = {
      ArrowLeft: [-step, 0],
      ArrowRight: [step, 0],
      ArrowUp: [0, -step],
      ArrowDown: [0, step],
    };
    const delta = moves[event.key];
    if (delta && pos) {
      moveOwn(index, [pos[0] + delta[0], pos[1] + delta[1]], true);
      event.preventDefault();
      return;
    }
    if (event.key === "Enter") {
      send({ t: "settle", i: index });
      event.preventDefault();
      return;
    }
    if (event.key === "l" || event.key === "L") {
      whereAmI(index);
      event.preventDefault();
      return;
    }
  }
  // The Weaver plucks with the number keys: 1 is the first star, 0 the tenth.
  const r = v.round;
  if (
    r &&
    v.phase === "weave" &&
    (r.weaver === v.me || r.twin === v.me) &&
    /^[0-9]$/.test(event.key)
  ) {
    const n = event.key === "0" ? 10 : Number(event.key);
    const star = r.stars[n - 1];
    if (star) send({ t: "pluck", s: star[0], i: star[1] });
    event.preventDefault();
  }
});

window.addEventListener("resize", () => {
  if (view) render();
});

window.requestAnimationFrame(animate);
send({ t: "ready" });
