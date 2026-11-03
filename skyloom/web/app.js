"use strict";
// Skyloom's page: draws the loom and the heads-up display from what the plugin sends, and turns
// clicks and keys into requests. The plugin owns the game; this page never decides anything and
// has no network or storage. Everything from the plugin may contain other people's names, so all
// text goes through esc() before it reaches innerHTML.
const LOOM_W = 1600;
const LOOM_H = 900;
const STEP = 20;
const FINE_STEP = 5;
const MOVE_EVERY_MS = 40;
const COLOURS = [
    "#ff8fb1",
    "#6fb6ff",
    "#f5c66b",
    "#4fd1a5",
    "#ff9e6b",
    "#b69cff",
    "#7fe0ff",
    "#ffd36b",
    "#ff7a7a",
    "#a8e66b",
    "#f08cff",
    "#9fb3ff",
];
let strings = {};
let view = null;
let viewAt = 0;
let showRules = false;
let toast = null;
let lastAnnounced = -1;
/** Where each star is drawn, gliding towards its target. */
const shown = new Map();
const target = new Map();
/** Own stars moved locally win over echoes for a moment. */
let ownMovedAt = 0;
let lastMoveSent = 0;
let dragging = null;
const sky = document.getElementById("sky");
const sayRegion = document.getElementById("say");
function send(message) {
    window.parent.postMessage(JSON.stringify(message), "*");
}
function tr(key, args = {}) {
    let text = strings[key] ?? key;
    for (const [k, v] of Object.entries(args))
        text = text.split(`{{${k}}}`).join(String(v));
    return text;
}
function esc(text) {
    return text.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] ?? c);
}
function announce(text) {
    // Clearing first makes screen readers repeat an identical message.
    sayRegion.textContent = "";
    window.setTimeout(() => (sayRegion.textContent = text), 50);
}
function reduceMotion() {
    return ((view?.reduceMotion ?? false) || window.matchMedia("(prefers-reduced-motion: reduce)").matches);
}
function player(session) {
    return view?.players.find((p) => p.s === session);
}
function nameOf(session) {
    return player(session)?.name ?? "?";
}
function colourOf(session) {
    return COLOURS[(player(session)?.shape ?? 0) % COLOURS.length] ?? "#ccc";
}
function initialOf(session) {
    return (nameOf(session).trim()[0] ?? "?").toUpperCase();
}
// ----- Shapes: every player has their own, so colour is never the only signal -----
function polygon(n, r, rotate = -Math.PI / 2) {
    const pts = [];
    for (let i = 0; i < n; i++) {
        const a = rotate + (i * 2 * Math.PI) / n;
        pts.push(`${(r * Math.cos(a)).toFixed(1)},${(r * Math.sin(a)).toFixed(1)}`);
    }
    return `M${pts.join("L")}Z`;
}
function starPath(r) {
    const pts = [];
    for (let i = 0; i < 10; i++) {
        const a = -Math.PI / 2 + (i * Math.PI) / 5;
        const rr = i % 2 === 0 ? r : r * 0.45;
        pts.push(`${(rr * Math.cos(a)).toFixed(1)},${(rr * Math.sin(a)).toFixed(1)}`);
    }
    return `M${pts.join("L")}Z`;
}
function shapePath(shape, r) {
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
function shapeIcon(shape, colour, size = 22) {
    return `<svg class="people__shape" viewBox="-14 -14 28 28" width="${size}" height="${size}" aria-hidden="true"><path d="${shapePath(shape, 10)}" fill="${colour}" stroke="currentColor" stroke-width="1"/></svg>`;
}
// ----- Small parts -----
function button(label, intent, extra = "", cls = "sky-button") {
    return `<button type="button" class="${cls}" data-intent="${intent}" ${extra}>${esc(label)}</button>`;
}
function glass(inner, cls = "") {
    return `<div class="glass ${cls}">${inner}</div>`;
}
function avatar(session) {
    return `<span class="weaver-avatar" style="--c:${colourOf(session)}" aria-hidden="true">${esc(initialOf(session))}</span>`;
}
function secondsLeft() {
    const r = view?.round;
    if (!r)
        return 0;
    return Math.max(0, Math.ceil((r.msLeft - (performance.now() - viewAt)) / 1000));
}
function timerRing() {
    const r = view?.round;
    if (!r)
        return "";
    const secs = secondsLeft();
    const left = r.dur > 0 ? secs / r.dur : 0;
    const c = 2 * Math.PI * 22;
    return `<div class="timer" role="timer" aria-label="${esc(tr("say.timer", { secs }))}">
    <svg viewBox="0 0 52 52"><circle class="timer__track" cx="26" cy="26" r="22"/><circle class="timer__left" id="timer-left" cx="26" cy="26" r="22" stroke-dasharray="${c.toFixed(1)}" stroke-dashoffset="${(c * (1 - left)).toFixed(1)}"/></svg>
    <span class="timer__text" id="timer-text">${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}</span></div>`;
}
function menu(v) {
    const sound = v.sound.muted
        ? button("🔇", "unmute", `aria-label="${esc(tr("menu.unmute"))}"`, "icon-button")
        : button("🔊", "mute", `aria-label="${esc(tr("menu.mute"))}"`, "icon-button");
    const places = v.place === "stage"
        ? button("⇥", "place-side", `aria-label="${esc(tr("menu.side"))}"`, "icon-button") +
            button("⧉", "place-dialog", `aria-label="${esc(tr("menu.popout"))}"`, "icon-button")
        : button("⇤", "place-stage", `aria-label="${esc(tr("menu.stage"))}"`, "icon-button");
    return `<div class="menu">
    ${button("?", "rules", `aria-label="${esc(tr("menu.rules"))}"`, "icon-button")}
    ${sound}
    <input type="range" min="0" max="100" step="5" value="${v.sound.volume}" id="volume" aria-label="${esc(tr("menu.volume"))}" ${v.sound.muted ? "disabled" : ""}/>
    ${places}
  </div>`;
}
function rules() {
    const items = [1, 2, 3, 4, 5, 6].map((n) => `<li>${esc(tr(`rules.${n}`))}</li>`).join("");
    return `<div class="overlay" role="dialog" aria-modal="true" aria-labelledby="rules-title">
    <div class="glass glass--box"><h2 id="rules-title" class="figure-name">${esc(tr("rules.title"))}</h2><ol>${items}</ol>
    ${button(tr("menu.close"), "rules-close", "autofocus")}</div></div>`;
}
// ----- Screens -----
function lobby(v) {
    const me = v.me;
    const lamp = v.lamplighter === me;
    const seated = v.players.some((p) => p.s === me);
    const people = v.players
        .map((p) => `<li>${shapeIcon(p.shape, COLOURS[p.shape % COLOURS.length] ?? "#ccc")}<span>${esc(p.name)}${p.s === me ? ` (${esc(tr("lobby.you"))})` : ""}</span>${p.s === v.lamplighter ? `<span class="people__state people__state--ok">${esc(tr("lobby.lamplighter"))}</span>` : ""}</li>`)
        .join("");
    const watching = v.spectators
        .map((n) => `<li><span>${esc(n)}</span><span class="people__state">${esc(tr("lobby.watching"))}</span></li>`)
        .join("");
    const seg = (intent, options) => `<div class="seg" role="group">${options
        .map(([value, label, on]) => `<button type="button" data-intent="${intent}" data-value="${value}" aria-pressed="${on}" ${lamp ? "" : "disabled"}>${esc(label)}</button>`)
        .join("")}</div>`;
    const weave = seg("weave", [60, 90, 120].map((s) => [String(s), tr("lobby.seconds", { secs: s }), v.settings.weave === s]));
    const skySeg = seg("mixed", [
        ["0", tr("lobby.calm"), !v.settings.mixed],
        ["1", tr("lobby.mixed"), v.settings.mixed],
    ]);
    const lampName = v.lamplighter === null ? "" : nameOf(v.lamplighter);
    let action;
    if (lamp) {
        action =
            button(tr("lobby.begin"), "begin", v.players.length < 2 ? "disabled" : "", "sky-button sky-button--gold") + (v.players.length < 2 ? `<p class="setting__hint">${esc(tr("lobby.need-two"))}</p>` : "");
    }
    else {
        action = `<p class="panel-text">${esc(tr("lobby.waiting", { name: lampName }))}</p>`;
    }
    const seat = seated
        ? lamp
            ? ""
            : button(tr("lobby.watch"), "watch")
        : button(tr("lobby.play"), "play", "", "sky-button sky-button--gold");
    const past = v.past.length
        ? `<h3>${esc(tr("lobby.past"))}</h3><ul class="past">${v.past
            .slice(0, 6)
            .map((p) => `<li>${esc(tr("lobby.past-item", { name: p.name, count: p.count, res: p.team }))}</li>`)
            .join("")}</ul>`
        : "";
    return `<div class="sky-layout">
    <div style="display:flex;flex-direction:column;justify-content:center;gap:16px;padding:24px 32px">
      <div class="wordmark"><h1 class="wordmark__name">Sky<span>loom</span></h1>
      <p class="wordmark__tag">${esc(tr("lobby.title"))}</p></div>
      <p class="panel-text" style="max-width:460px">${esc(tr("lobby.pitch"))}</p>
      ${past}
    </div>
    <section class="sky-col sky-col--scroll" aria-label="${esc(tr("lobby.players"))}">
      <h3>${esc(tr("lobby.players"))}</h3><ul class="people">${people}${watching}</ul>
      <div class="setting"><span>${esc(tr("lobby.weave-time"))}</span>${weave}</div>
      <div class="setting"><span>${esc(tr("lobby.sky"))}</span>${skySeg}<span class="setting__hint">${esc(tr("lobby.mixed-hint"))}</span></div>
      ${action}${seat}
    </section></div>`;
}
function twistHint(v, r) {
    const me = v.me;
    const weaving = r.weaver === me || r.twin === me;
    const show = (r.twist === "fog" && !weaving) ||
        (r.twist === "drift" && !weaving) ||
        (r.twist === "mirror" && weaving) ||
        (r.twist === "twin" && weaving);
    return show ? `<p class="glass glass__muted">${esc(tr(`twist.${r.twist}-hint`))}</p>` : "";
}
function skyCard(card) {
    const lines = card.lines
        .map((l) => `<polyline class="thread" points="${l.map((p) => p.join(",")).join(" ")}"/>`)
        .join("");
    const points = card.points
        .map((p) => `<circle cx="${p[0]}" cy="${p[1]}" r="34" fill="var(--thread)"/>`)
        .join("");
    return `<aside class="sky-card hud--side" aria-label="${esc(tr("card.label", { figure: card.name }))}">
    <span class="sky-card__label">${esc(tr("hud.sky-card"))}</span>
    <svg class="sky-card__art" viewBox="-120 -120 1840 1840" preserveAspectRatio="xMidYMid meet" aria-hidden="true">
      <g style="stroke-width:14">${lines}</g>${points}</svg>
    <div class="sky-card__name figure-name">${esc(card.name)}</div>
    ${card.mirror ? `<p class="sky-card__rule">${esc(tr("twist.mirror-hint"))}</p>` : ""}
  </aside>`;
}
function weaveHud(v, r) {
    const me = v.me;
    const weaving = r.weaver === me || r.twin === me;
    const weaverText = r.weaver === me ? tr("hud.you-weave") : tr("hud.weaving", { name: nameOf(r.weaver) });
    const top = `<div class="hud hud--top-left">
    ${glass(`${avatar(r.weaver)}<span><b>${esc(weaverText)}</b></span>`)}
    ${glass(`<span class="glass__muted">${esc(tr("hud.round", { no: r.no, of: r.of }))}</span>`)}
    ${glass(`<span class="glass__muted">${esc(tr("hud.sky", { twist: tr(`twist.${r.twist}`) }))}</span>`)}
  </div>`;
    const firstLight = r.msLeft - (performance.now() - viewAt) <= 10_000;
    const timer = v.phase === "weave"
        ? `<div class="hud hud--top-right">${glass(`<span class="glass__muted">${esc(tr(firstLight ? "hud.first-light" : "hud.dawn-in"))}</span>`)}${timerRing()}</div>`
        : "";
    let center = "";
    if (v.phase === "seal") {
        center = `<div class="hud hud--center-top" style="top:45%">${glass(`<span>${esc(tr("hud.reading", { name: nameOf(r.weaver) }))}</span>`)}</div>`;
    }
    else if (v.phase === "dawn") {
        center = `<div class="hud hud--center-top">${glass(`<span>${esc(tr("dawn.title"))}</span>`)}</div>`;
    }
    let bottomLeft = "";
    if (v.role === "threader") {
        const own = r.stars.filter((s) => s[0] === me).length;
        bottomLeft = glass(`<b>${esc(tr(own > 1 ? "hud.your-stars" : "hud.your-star"))}</b>
       <span class="keys"><span class="key">${esc(tr("hud.arrow-keys"))}</span>${esc(tr("hud.keys-move"))}
       <span class="key">Shift</span>${esc(tr("hud.keys-fine"))}
       <span class="key">Enter</span>${esc(tr("hud.keys-settle"))}
       <span class="key">L</span>${esc(tr("hud.keys-where"))}</span>`);
    }
    else if (weaving) {
        bottomLeft =
            glass(`<span class="keys"><span class="key">${esc(tr("hud.number-keys"))}</span>${esc(tr("hud.keys-pluck"))}</span>`) + (r.weaver === me && v.phase === "weave" ? button(tr("hud.tie-off"), "tie-off") : "");
    }
    else {
        bottomLeft = `<label class="glass glass--box guess">${esc(tr("hud.guess"))}<input id="guess" maxlength="60" autocomplete="off"/><span class="glass__muted">${esc(tr("hud.guess-hint"))}</span></label>`;
    }
    const live = glass(`<span class="glass__muted">${esc(tr("hud.live"))}</span><b>${esc(tr(v.live ? "hud.relay" : "hud.server"))}</b>`);
    const who = v.role === "spectator" || v.role === "player"
        ? `<div class="hud" style="left:16px;top:64px"><details class="glass glass--box"><summary>${esc(tr("hud.who"))}</summary><ul class="people">${v.players
            .map((p) => `<li>${shapeIcon(p.shape, colourOf(p.s))}<span>${esc(p.name)}</span></li>`)
            .join("")}</ul></details></div>`
        : "";
    const card = weaving && r.card ? skyCard(r.card) : "";
    return `${top}${timer}${center}${who}${twistHint(v, r) ? `<div class="hud" style="left:16px;top:64px">${twistHint(v, r)}</div>` : ""}${card}
    <div class="hud hud--bottom"><div class="hud">${bottomLeft}</div>${live}</div>`;
}
function nextControls(v) {
    const me = v.me;
    if (v.canEnd)
        return button(tr("next.tapestry"), "tapestry", "", "sky-button sky-button--gold");
    if (v.next === null) {
        return glass(`<span>${esc(tr("next.night-over", { name: v.lamplighter === null ? "" : nameOf(v.lamplighter) }))}</span>`);
    }
    const chip = glass(`<span class="glass__muted">${esc(tr("next.weaver", { name: v.next === me ? `${nameOf(me)} (${tr("lobby.you")})` : nameOf(v.next) }))}</span>`);
    return v.next === me
        ? chip + button(tr("next.ready"), "weave-ready", "", "sky-button sky-button--gold")
        : chip;
}
function revealHud(v, r) {
    const rv = r.reveal;
    if (r.void !== null || !rv) {
        return `<div class="hud hud--center-top"><div class="reveal-title">
      <span class="reveal-title__name figure-name">${esc(tr("void.title"))}</span>
      <span class="reveal-title__note">${esc(tr(`void.${r.void ?? "stalled"}`))}</span></div></div>
      <div class="hud hud--bottom"><span></span><div class="hud" style="position:static">${nextControls(v)}</div></div>`;
    }
    const outcome = rv.gold ? "reveal.gold" : rv.woven ? "reveal.woven" : "reveal.loose";
    const awards = [];
    if (rv.truest !== null) {
        const s = rv.stars.find((x) => x[0] === rv.truest);
        awards.push(`<li><span>${esc(tr("reveal.truest"))}</span><b>${esc(nameOf(rv.truest))} · ${s?.[4] ?? 0}%</b></li>`);
    }
    if (rv.loosest !== null && rv.loosest !== rv.truest) {
        const s = rv.stars.find((x) => x[0] === rv.loosest);
        awards.push(`<li><span>${esc(tr("reveal.loosest"))}</span><b>${esc(nameOf(rv.loosest))} · ${s?.[4] ?? 0}%</b></li>`);
    }
    if (rv.bonus > 0) {
        awards.push(`<li><span>${esc(tr("reveal.weaver"))}</span><b>${esc(tr("reveal.tied-off", { name: nameOf(r.weaver), bonus: rv.bonus }))}</b></li>`);
    }
    const guess = rv.guess !== null
        ? `<p class="reveal-title__note">${esc(tr("reveal.your-guess", { text: rv.guess }))}</p>`
        : "";
    return `<div class="hud hud--top-left">${glass(`${avatar(r.weaver)}<span>${esc(tr("reveal.wove", { name: nameOf(r.weaver) }))}</span>`)}</div>
    <div class="hud hud--center-top" style="top:14px"><div class="reveal-title">
      <span class="reveal-title__note">${esc(tr("dawn.title"))}</span>
      <span class="reveal-title__name figure-name">${esc(rv.figure ?? "")}</span>
      <span class="reveal-title__score"><span>${esc(tr("reveal.resonance", { res: rv.resonance }))}</span> · ${esc(tr(outcome))}</span>${guess}
    </div></div>
    <ul class="awards hud hud--side" style="right:16px;top:120px;width:220px;flex-direction:column;align-items:stretch">${awards.join("")}</ul>
    <div class="hud hud--bottom">${glass(`<span>${esc(tr(`verify.${rv.verify}`))}</span>`)}
      <div class="hud" style="position:static">${nextControls(v)}</div></div>`;
}
function miniFigure(lines) {
    if (!lines)
        return `<svg viewBox="0 0 100 100" aria-hidden="true"></svg>`;
    const body = lines
        .map((l) => `<polyline class="thread" style="stroke-width:2" points="${l.map((p) => p.join(",")).join(" ")}"/>`)
        .join("");
    return `<svg viewBox="-8 -8 116 116" aria-hidden="true">${body}</svg>`;
}
function results(v) {
    const res = v.results;
    if (!res)
        return "";
    const lamp = v.lamplighter === v.me;
    const tiles = res.rounds
        .map((r) => `<figure class="${r.resonance !== null && r.resonance >= 90 ? "const--gold" : r.resonance !== null && r.resonance < 60 ? "const--frayed" : ""}">
        ${miniFigure(r.lines)}
        <figcaption class="figure-name">${esc(r.figure ?? tr("results.clouds"))}</figcaption>
        <span class="const-score glass__muted">${esc(tr("results.round", { no: r.no }))} · ${esc(r.weaver)}${r.resonance !== null ? ` · ${r.resonance}%` : ""}</span>
      </figure>`)
        .join("");
    const h = res.honours;
    const honour = (key, name) => name ? `<li><span>${esc(tr(key))}</span><b>${esc(name)}</b></li>` : "";
    return `<div class="sky-layout"><div style="position:relative">
      <div class="hud hud--center-top"><span class="reveal-title__name figure-name">${esc(tr("results.title"))}</span></div>
      <div class="tapestry">${tiles}</div></div>
    <section class="sky-col sky-col--scroll" aria-label="${esc(tr("results.title"))}">
      <div class="big-number">${res.team ?? 0}%<br/><small>${esc(tr("results.team", { res: res.team ?? 0 }))}</small></div>
      <ul class="awards">${honour("results.silver", h.silver)}${honour("results.truest", h.truest)}${honour("results.wanderer", h.wanderer)}</ul>
      <label class="setting">${esc(tr("results.name"))}<input class="name-field" id="sky-name" maxlength="40" value="${esc(res.name)}" placeholder="${esc(tr("results.name-placeholder"))}"/></label>
      ${button(tr("results.save"), "name")}
      ${lamp ? button(tr(res.summaryPosted ? "results.posted" : "results.post"), "post-summary", res.summaryPosted ? "disabled" : "") : ""}
      ${lamp ? button(tr("results.end"), "close-night") : ""}
    </section></div>`;
}
function render() {
    const v = view;
    if (!v)
        return;
    document.documentElement.dataset.theme = v.theme === "light" ? "light" : "dark";
    const compact = v.place !== "stage" || sky.clientWidth < 640;
    sky.className = `sky${compact ? " sky--compact" : ""}${reduceMotion() ? " reduce-motion" : ""}`;
    const focusedId = document.activeElement?.id ?? "";
    const guess = document.getElementById("guess")?.value ?? "";
    let hud = "";
    const r = v.round;
    if (v.phase === "none")
        hud = "";
    else if (v.phase === "lobby")
        hud = lobby(v);
    else if (v.phase === "results")
        hud = results(v);
    else if (r && (v.phase === "reveal" || v.phase === "void"))
        hud = revealHud(v, r);
    else if (r)
        hud = weaveHud(v, r);
    const loomShown = v.phase !== "lobby" && v.phase !== "results" && v.phase !== "none";
    sky.innerHTML = `<div class="sky__weave"></div>${dust()}
    <div class="sky__dawn" id="dawn"></div><div class="sky__horizon"></div>
    ${loomShown ? loomSvg(v) : ""}
    <div id="hud">${hud}</div>${menu(v)}
    ${toast && toast.until > performance.now() ? `<div class="toast" style="left:50%;top:84px;transform:translateX(-50%)">${esc(toast.text)}</div>` : ""}
    ${showRules ? rules() : ""}`;
    const g = document.getElementById("guess");
    if (g)
        g.value = guess;
    if (focusedId)
        document.getElementById(focusedId)?.focus();
    updateDawn();
}
let dustCache = "";
function dust() {
    if (!dustCache) {
        // A fixed pseudo-random field, the same every time.
        let seed = 7;
        const rnd = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
        const dots = [];
        for (let i = 0; i < 90; i++) {
            const cls = i % 3 === 0 ? ' class="twinkle"' : "";
            dots.push(`<circle${cls} cx="${(rnd() * LOOM_W).toFixed(0)}" cy="${(rnd() * LOOM_H).toFixed(0)}" r="${(0.8 + rnd() * 1.6).toFixed(1)}" style="animation-delay:${(rnd() * 3).toFixed(1)}s"/>`);
        }
        dustCache = `<svg class="sky__dust" viewBox="0 0 ${LOOM_W} ${LOOM_H}" preserveAspectRatio="xMidYMid slice" aria-hidden="true">${dots.join("")}</svg>`;
    }
    return dustCache;
}
// ----- The loom -----
function starKey(owner, index) {
    return `${owner}-${index}`;
}
function loomSvg(v) {
    const r = v.round;
    if (!r)
        return "";
    const rv = v.phase === "reveal" ? r.reveal : null;
    let figure = "";
    if (rv) {
        const ghost = rv.ghost
            .map((l) => `<polyline class="ghost-line fade-in" points="${l.map((p) => p.join(",")).join(" ")}"/>`)
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
    return `<svg class="loom" id="loom" viewBox="0 0 ${LOOM_W} ${LOOM_H}" preserveAspectRatio="xMidYMid meet" role="group" aria-label="${label}">
    <defs><filter id="soft" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="6"/></filter></defs>
    ${figure}<g id="stars">${starsMarkup(v, r)}</g><g id="ripples"></g></svg>`;
}
/** Gold threads between stars that follow each other along the figure's lines. */
function threads(rv) {
    const order = rv.stars.map((s, k) => ({ s, j: rv.order[k] ?? k, t: rv.targets[k] }));
    order.sort((a, b) => a.j - b.j);
    const gaps = [];
    for (let k = 1; k < order.length; k++) {
        const a = order[k - 1]?.t;
        const b = order[k]?.t;
        if (a && b)
            gaps.push(Math.hypot(a[0] - b[0], a[1] - b[1]));
    }
    const sorted = [...gaps].sort((a, b) => a - b);
    const median = sorted[Math.floor(sorted.length / 2)] ?? 0;
    let out = "";
    for (let k = 1; k < order.length; k++) {
        const a = order[k - 1];
        const b = order[k];
        const gap = gaps[k - 1] ?? 0;
        if (!a || !b || gap > median * 1.7)
            continue;
        const d = `M${a.s[2]},${a.s[3]}L${b.s[2]},${b.s[3]}`;
        out += `<path class="thread-glow" d="${d}"/><path class="thread thread-run" d="${d}" style="animation-delay:${(k * 0.12).toFixed(2)}s"/>`;
    }
    return out;
}
function starsMarkup(v, r) {
    const me = v.me;
    const weaving = (r.weaver === me || r.twin === me) && v.phase === "weave";
    const revealed = new Map();
    if (v.phase === "reveal" && r.reveal) {
        for (const s of r.reveal.stars)
            revealed.set(starKey(s[0], s[1]), s[4]);
    }
    const counts = new Map();
    for (const s of r.stars)
        counts.set(s[0], (counts.get(s[0]) ?? 0) + 1);
    return r.stars
        .map(([owner, index, x, y]) => {
        const key = starKey(owner, index);
        if (!shown.has(key))
            shown.set(key, [x, y]);
        const [sx, sy] = shown.get(key) ?? [x, y];
        const p = player(owner);
        const shape = p?.shape ?? 0;
        const mine = owner === me && v.role === "threader" && v.phase === "weave";
        const many = (counts.get(owner) ?? 1) > 1;
        const name = (owner === me ? `${nameOf(owner)} (${tr("lobby.you")})` : nameOf(owner)) +
            (many ? ` ${index + 1}` : "");
        const res = revealed.get(key);
        const label = owner === me
            ? tr("star.label-you", { shape: tr(`shape.${shape}`), n: index + 1 })
            : tr("star.label", { name: nameOf(owner), shape: tr(`shape.${shape}`), n: index + 1 });
        const left = r.left.includes(owner) ? " star--left" : "";
        const cls = mine
            ? "star star--you"
            : weaving
                ? "star star--other star--pluckable"
                : "star star--other";
        const focusable = mine || weaving ? `tabindex="0" role="button"` : `role="img"`;
        return `<g class="${cls}${left}" id="star-${key}" data-key="${key}" data-owner="${owner}" data-index="${index}" ${focusable} aria-label="${esc(label)}" transform="translate(${sx.toFixed(1)},${sy.toFixed(1)})" style="--c:${colourOf(owner)}">
        <circle class="star__glow" r="26"/>
        ${p?.talking ? `<circle class="star__talk" r="30"/>` : ""}
        <circle class="star__focus" r="24"/>
        <path class="star__body" d="${shapePath(shape, 14)}"/>
        <text class="star__initial">${esc(initialOf(owner))}</text>
        <text class="star__name" y="38">${esc(name)}${res !== undefined ? ` · ${res}%` : ""}</text></g>`;
    })
        .join("");
}
function loomPoint(event) {
    const svg = document.getElementById("loom");
    const ctm = svg?.getScreenCTM();
    if (!svg || !ctm)
        return null;
    const p = new DOMPoint(event.clientX, event.clientY).matrixTransform(ctm.inverse());
    return [Math.round(p.x), Math.round(p.y)];
}
function clampPt([x, y]) {
    return [Math.max(0, Math.min(LOOM_W, x)), Math.max(0, Math.min(LOOM_H, y))];
}
function moveOwn(index, to, force = false) {
    const v = view;
    if (!v)
        return;
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
function ownPos(index) {
    return view ? shown.get(starKey(view.me, index)) : undefined;
}
function areaOf([x, y]) {
    const col = x < LOOM_W / 3 ? "left" : x > (2 * LOOM_W) / 3 ? "right" : "";
    const row = y < LOOM_H / 3 ? "top" : y > (2 * LOOM_H) / 3 ? "bottom" : "";
    const key = row && col ? `${row}-${col}` : row || col || "centre";
    return tr(`area.${key}`);
}
/** "Your star B: centre, 2 left of Ben's triangle" */
function whereAmI(index) {
    const v = view;
    const r = v?.round;
    const pos = ownPos(index);
    if (!v || !r || !pos)
        return;
    let best = null;
    for (const s of r.stars) {
        if (s[0] === v.me)
            continue;
        const p = shown.get(starKey(s[0], s[1])) ?? [s[2], s[3]];
        const d = Math.hypot(p[0] - pos[0], p[1] - pos[1]);
        if (!best || d < best.d)
            best = { d, s: [s[0], s[1], p[0], p[1], s[4]] };
    }
    let near = "";
    if (best) {
        const dx = Math.round((pos[0] - best.s[2]) / 100);
        const dy = Math.round((pos[1] - best.s[3]) / 100);
        const parts = [];
        if (dx !== 0)
            parts.push(tr(dx < 0 ? "say.left" : "say.right", { n: Math.abs(dx) }));
        if (dy !== 0)
            parts.push(tr(dy < 0 ? "say.up" : "say.down", { n: Math.abs(dy) }));
        if (parts.length > 0) {
            near = tr("say.near", {
                offset: parts.join(tr("say.and")),
                name: nameOf(best.s[0]),
                shape: tr(`shape.${player(best.s[0])?.shape ?? 0}`),
            });
        }
    }
    announce(tr("say.where", {
        who: tr("say.your-star", { initial: initialOf(v.me) }),
        area: areaOf(pos),
        near,
    }));
}
function animate() {
    const v = view;
    if (v?.round) {
        const now = performance.now();
        for (const [key, to] of target) {
            const from = shown.get(key) ?? to;
            const mine = v.me === Number(key.split("-")[0]);
            if (mine && now - ownMovedAt < 600)
                continue;
            const k = reduceMotion() ? 1 : 0.12;
            const next = [from[0] + (to[0] - from[0]) * k, from[1] + (to[1] - from[1]) * k];
            if (Math.abs(next[0] - from[0]) < 0.05 && Math.abs(next[1] - from[1]) < 0.05)
                continue;
            shown.set(key, next);
            document
                .getElementById(`star-${key}`)
                ?.setAttribute("transform", `translate(${next[0].toFixed(1)},${next[1].toFixed(1)})`);
        }
        updateTimer();
    }
    window.requestAnimationFrame(animate);
}
function updateDawn() {
    const r = view?.round;
    const el = document.getElementById("dawn");
    if (!el)
        return;
    let progress = 0;
    if (r && view?.phase === "weave" && r.dur > 0)
        progress = 1 - secondsLeft() / r.dur;
    else if (view && ["dawn", "reveal", "void"].includes(view.phase))
        progress = 1;
    el.style.setProperty("--dawn-progress", progress.toFixed(3));
    document
        .querySelector(".sky__horizon")
        ?.style.setProperty("--dawn-progress", progress.toFixed(3));
}
let lastSecs = -1;
function updateTimer() {
    if (view?.phase !== "weave")
        return;
    const secs = secondsLeft();
    if (secs === lastSecs)
        return;
    lastSecs = secs;
    const text = document.getElementById("timer-text");
    if (text)
        text.textContent = `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;
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
    if (secs === 10)
        render();
}
function ripple(owner, index) {
    if (reduceMotion())
        return;
    const p = shown.get(starKey(owner, index));
    const g = document.getElementById("ripples");
    if (!p || !g)
        return;
    const c = document.createElementNS("http://www.w3.org/2000/svg", "circle");
    c.setAttribute("class", "ripple");
    c.setAttribute("cx", String(p[0]));
    c.setAttribute("cy", String(p[1]));
    c.setAttribute("r", "18");
    g.appendChild(c);
    window.setTimeout(() => c.remove(), 1300);
}
// ----- Messages from the plugin -----
function setStars(stars) {
    const keys = new Set();
    for (const [owner, index, x, y] of stars) {
        const key = starKey(owner, index);
        keys.add(key);
        target.set(key, [x, y]);
        if (!shown.has(key))
            shown.set(key, [x, y]);
    }
    for (const key of [...target.keys()])
        if (!keys.has(key))
            target.delete(key);
}
window.addEventListener("message", (event) => {
    if (event.source !== window.parent || typeof event.data !== "string")
        return;
    let msg;
    try {
        msg = JSON.parse(event.data);
    }
    catch {
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
                const stars = msg.phase === "reveal" && msg.round.reveal
                    ? msg.round.reveal.stars.map((s) => [s[0], s[1], s[2], s[3], true])
                    : msg.round.stars;
                setStars(stars);
                if (msg.phase === "reveal")
                    for (const [k, p] of target)
                        shown.set(k, p);
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
                if (g && count !== msg.s.length)
                    g.innerHTML = starsMarkup(view, view.round);
            }
            break;
        case "talk": {
            const p = player(msg.s);
            if (p) {
                p.talking = msg.on;
                document.querySelectorAll(`[data-owner="${msg.s}"]`).forEach((g) => {
                    const ring = g.querySelector(".star__talk");
                    if (msg.on && !ring) {
                        const c = document.createElementNS("http://www.w3.org/2000/svg", "circle");
                        c.setAttribute("class", "star__talk");
                        c.setAttribute("r", "30");
                        g.insertBefore(c, g.children[1] ?? null);
                    }
                    else if (!msg.on)
                        ring?.remove();
                });
            }
            break;
        }
        case "pluck":
            ripple(msg.s, msg.i);
            toast = {
                text: msg.s === view?.me
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
    const el = event.target.closest("[data-intent]");
    if (el) {
        const intent = el.dataset.intent ?? "";
        const v = view;
        switch (intent) {
            case "weave":
                if (v)
                    send({ t: "settings", weave: Number(el.dataset.value), mixed: v.settings.mixed });
                break;
            case "mixed":
                if (v)
                    send({ t: "settings", weave: v.settings.weave, mixed: el.dataset.value === "1" });
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
            case "name": {
                const input = document.getElementById("sky-name");
                send({ t: "name", text: input?.value ?? "" });
                break;
            }
            default:
                send({ t: intent });
        }
        return;
    }
    const star = event.target.closest(".star--pluckable");
    if (star)
        send({ t: "pluck", s: Number(star.dataset.owner), i: Number(star.dataset.index) });
});
sky.addEventListener("change", (event) => {
    const el = event.target;
    if (el.id === "volume")
        send({ t: "volume", v: Number(el.value) });
    if (el.id === "guess")
        send({ t: "guess", text: el.value });
});
sky.addEventListener("pointerdown", (event) => {
    const g = event.target.closest(".star--you");
    if (!g || !view)
        return;
    dragging = { key: g.dataset.key ?? "", star: Number(g.dataset.index) };
    g.setPointerCapture(event.pointerId);
    g.focus();
    event.preventDefault();
});
sky.addEventListener("pointermove", (event) => {
    if (!dragging)
        return;
    const p = loomPoint(event);
    if (p)
        moveOwn(dragging.star, p);
});
sky.addEventListener("pointerup", () => {
    if (!dragging)
        return;
    const index = dragging.star;
    const p = ownPos(index);
    if (p)
        moveOwn(index, p, true);
    send({ t: "settle", i: index });
    dragging = null;
});
document.addEventListener("keydown", (event) => {
    const v = view;
    if (!v)
        return;
    if (event.key === "Escape" && showRules) {
        showRules = false;
        render();
        event.preventDefault();
        return;
    }
    const active = document.activeElement;
    if (active?.tagName === "INPUT")
        return;
    const g = active?.closest(".star--you");
    if (g) {
        const index = Number(g.dataset.index);
        const step = event.shiftKey ? FINE_STEP : STEP;
        const pos = ownPos(index);
        const moves = {
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
    if (r &&
        v.phase === "weave" &&
        (r.weaver === v.me || r.twin === v.me) &&
        /^[0-9]$/.test(event.key)) {
        const n = event.key === "0" ? 10 : Number(event.key);
        const star = r.stars[n - 1];
        if (star)
            send({ t: "pluck", s: star[0], i: star[1] });
        event.preventDefault();
    }
});
window.addEventListener("resize", () => {
    if (view)
        render();
});
window.requestAnimationFrame(animate);
send({ t: "ready" });
