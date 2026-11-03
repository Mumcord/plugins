// The stage's controls: tool column, context bar, history/zoom, board actions, presenter banner
// and bar, no-relay strip, queue chip, outline and help. State lives in `app`; user actions call
// back into `act`, which turns them into bridge messages.

import { el, icon, roving, setTabStop, shapeGlyph } from "./dom.js";
import { TOOLS, SHAPES, HELP } from "./keyboard.js";
import { WIDTHS, TEXT_SIZES } from "./geom.js";
import { personColour, outlineEntries } from "./objects.js";

const $ = (id) => document.getElementById(id);

export const ICONS = {
  select: ["M5 3l14 8-6 2-3 6z"],
  pen: ["M4 20l1-4L16 5l3 3L8 19z", "M14 7l3 3"],
  hl: ["M9 3l6 6-6 9H5l-1-3z", "M4 21h16"],
  eraser: ["M7 20h13M5 14l8-8 6 6-6 6H8z"],
  shapes: ["M3 11h9v9H3z", "M21 8a5 5 0 1 1-10 0 5 5 0 0 1 10 0z"],
  arrow: ["M5 19L19 5M10 5h9v9"],
  note: ["M4 4h16v11l-5 5H4z", "M15 20v-5h5"],
  text: ["M5 6V4h14v2M12 4v16M9 20h6"],
  laser: ["M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0z", "M12 2v3M12 19v3M2 12h3M19 12h3"],
  undo: ["M9 14L4 9l5-5", "M4 9h10a6 6 0 010 12h-3"],
  redo: ["M15 14l5-5-5-5", "M20 9H10a6 6 0 000 12h3"],
  zoomOut: ["M5 12h14"],
  zoomIn: ["M12 5v14M5 12h14"],
  fit: ["M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"],
  present: ["M3 4h18v12H3z", "M8 20h8M12 16v4"],
  export: ["M12 3v12", "M7 10l5 5 5-5", "M5 21h14"],
  post: ["M4 4h16v12H8l-4 4z"],
  settings: ["M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1", "M15 4v4M9 10v4M17 16v4"],
  clear: ["M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"],
  describe: ["M4 6h16M4 12h16M4 18h10"],
  outline: ["M9 6h11M9 12h11M9 18h11M4 6h.01M4 12h.01M4 18h.01"],
  help: ["M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0z", "M9.1 9a3 3 0 0 1 5.8 1c0 2-3 3-3 3", "M12 17h.01"],
  front: ["M12 19V5M5 12l7-7 7 7"],
  back: ["M12 5v14M5 12l7 7 7-7"],
  dup: ["M8 8h12v12H8z", "M4 16V4h12"],
  edit: ["M4 20l1-4L16 5l3 3L8 19z"],
  check: ["M5 12l5 5 9-10"],
  rect: ["M4 6h16v12H4z"],
  ellipse: ["M12 18c4.4 0 8-2.7 8-6s-3.6-6-8-6-8 2.7-8 6 3.6 6 8 6z"],
  diamond: ["M12 3l9 9-9 9-9-9z"],
  line: ["M5 19L19 5"],
};

const DRAW_TOOLS = new Set(["pen", "hl", "eraser", "shapes", "arrow", "note", "text"]);
const NOTE_VARS = ["--note-yellow", "--note-pink", "--note-mint", "--note-sky", "--note-lilac"];

/** Tools this person may use right now. */
export function toolAllowed(app, id) {
  if (id === "select") return true;
  if (id === "laser") return app.status.canDraw || app.settings.laser || app.status.presenting === app.me.author;
  return app.status.canDraw && DRAW_TOOLS.has(id);
}

function toolButton(app, act, id, key) {
  const name = app.t(`tool.${id}`);
  const label = app.t("tool.withKey", { name, key });
  const b = el("button", { class: "wb-tool", type: "button", "aria-label": label, title: label, "aria-pressed": "false", "data-tool": id }, [
    icon(ICONS[id]),
    el("span", { class: "wb-tool__key", "aria-hidden": "true", text: key }),
  ]);
  b.addEventListener("click", () => act.chooseTool(id, false));
  return b;
}

/** A radio group of buttons in the mockup's style with roving focus. */
function radioGroup(label, options, current, onPick) {
  const group = el("div", { class: "wb-props__group", role: "radiogroup", "aria-label": label });
  const buttons = options.map((o, i) => {
    const b = el("button", { class: o.cls, role: "radio", type: "button", "aria-label": o.label, title: o.label, "aria-checked": String(i === current), style: o.style || null }, o.children || []);
    if (i === current) b.dataset.active = "";
    b.addEventListener("click", () => onPick(i));
    group.append(b);
    return b;
  });
  setTabStop(buttons, buttons[current] || buttons[0]);
  roving(group, () => buttons, false, (b) => b.click());
  return group;
}

function numField(label, title, value, onSet) {
  const input = el("input", { type: "number", min: "1", max: "4000", step: "1", value: String(Math.round(Math.abs(value))), "aria-label": title });
  input.addEventListener("change", () => {
    const v = Number.parseFloat(input.value);
    if (Number.isFinite(v) && v >= 1 && v <= 4000) onSet(v);
  });
  return el("label", { class: "wb-num" }, [el("span", { "aria-hidden": "true", text: label }), input]);
}

export function createChrome(app, act) {
  const toolbar = $("toolbar");
  const props = $("props");
  const zoom = $("zoom");
  const actions = $("actions");
  const banner = $("banner");
  const presentbar = $("presentbar");
  const tree = $("outline-tree");
  const filter = $("outline-filter");
  const outline = $("outline");
  const help = $("help");
  let toolButtons = [];
  let presentButtons = [];
  let actionButtons = [];
  let zoomVal = null;
  let undoBtn = null;
  let redoBtn = null;
  let outlineBtn = null;
  let clearBtn = null;
  let outlineAuthor = null;
  let outlineTimer = 0;
  let focusedOutline = null;
  let queuePeak = 0;

  // Arrow keys move between the buttons of each bar; set up once, the buttons are rebuilt on init.
  roving(toolbar, () => toolButtons, true);
  roving(presentbar, () => presentButtons, false);
  roving(actions, () => actionButtons, false);

  function buildToolbar() {
    toolbar.replaceChildren();
    toolbar.setAttribute("aria-label", app.t("tools.label"));
    toolButtons = [];
    const seps = new Set(["pen", "shapes", "laser"]);
    for (const { id, key } of TOOLS) {
      if (seps.has(id)) toolbar.append(el("hr", { class: "wb-sep" }));
      const b = toolButton(app, act, id, key);
      toolButtons.push(b);
      toolbar.append(b);
    }
  }

  function buildPresentbar() {
    presentbar.replaceChildren();
    presentbar.setAttribute("aria-label", app.t("present.tools"));
    presentButtons = ["laser", "pen", "eraser"].map((id) => toolButton(app, act, id, TOOLS.find((x) => x.id === id).key));
    presentbar.append(...presentButtons, el("span", { class: "wb-props__sep" }));
    const [before, after] = app.t("present.hint").split("{key}");
    presentbar.append(el("span", { class: "wb-presentbar__hint" }, [document.createTextNode(before ?? ""), el("span", { class: "kbd", text: "Space" }), document.createTextNode(after ?? "")]));
  }

  function miniButton(name, label, onClick) {
    const b = el("button", { class: "wb-mini", type: "button", "aria-label": label, title: label }, [icon(ICONS[name], 16)]);
    b.addEventListener("click", onClick);
    return b;
  }

  function buildZoom() {
    zoom.replaceChildren();
    zoom.setAttribute("aria-label", app.t("zoom.label"));
    undoBtn = miniButton("undo", app.t("zoom.undo"), act.undo);
    redoBtn = miniButton("redo", app.t("zoom.redo"), act.redo);
    zoomVal = el("span", { class: "wb-zoom__val", "aria-live": "polite" });
    zoom.append(
      undoBtn,
      redoBtn,
      el("span", { class: "wb-props__sep" }),
      miniButton("zoomOut", app.t("zoom.out"), () => act.zoomStep(-1)),
      zoomVal,
      miniButton("zoomIn", app.t("zoom.in"), () => act.zoomStep(1)),
      miniButton("fit", app.t("zoom.fit"), act.fit),
    );
  }

  function actionButton(name, text, label, onClick) {
    const b = el("button", { class: text ? "wb-action" : "wb-mini", type: "button", "aria-label": label, title: label }, [icon(ICONS[name], 16), text ? el("span", { text }) : null]);
    b.addEventListener("click", onClick);
    return b;
  }

  function buildActions() {
    actions.replaceChildren();
    actions.setAttribute("aria-label", app.t("act.label"));
    outlineBtn = actionButton("outline", null, app.t("act.outline"), act.toggleOutline);
    outlineBtn.setAttribute("aria-pressed", "false");
    clearBtn = actionButton("clear", null, app.t("act.clear"), () => act.menu("clear"));
    actionButtons = [
      actionButton("present", app.t("act.present"), app.t("act.present"), () => act.present(true)),
      actionButton("export", app.t("act.export"), app.t("act.export"), () => act.menu("export")),
      actionButton("post", app.t("act.post"), app.t("act.postLabel"), () => act.menu("post")),
      actionButton("describe", null, app.t("act.describe"), () => act.menu("describe")),
      outlineBtn,
      actionButton("settings", null, app.t("act.settings"), () => act.menu("settings")),
      clearBtn,
      actionButton("help", null, app.t("act.help"), act.openHelp),
    ];
    actions.append(...actionButtons);
    setTabStop(actionButtons, actionButtons[0]);
  }

  function buildStatic() {
    const wb = $("wb");
    wb.setAttribute("aria-label", app.t("app.label"));
    wb.setAttribute("aria-roledescription", app.t("app.roledesc"));
    $("strip-title").textContent = app.t("relay.title");
    $("strip-body").textContent = app.t("relay.body");
    $("strip-why-text").textContent = app.t("relay.whyText");
    $("strip-why").textContent = app.t("relay.why");
    $("outline-title").textContent = app.t("outline.title");
    $("outline-filter-label").textContent = app.t("outline.filter");
    $("outline-empty").textContent = app.t("outline.empty");
    $("help-title").textContent = app.t("help.title");
    $("help-close").textContent = app.t("help.close");
    const keys = $("help-keys");
    keys.replaceChildren();
    for (const [k, desc] of HELP) keys.append(el("dt", { text: k }), el("dd", { text: app.t(desc) }));
  }

  // --- Context bar ---------------------------------------------------------------------------

  function inkOptions(hl) {
    return [0, 1, 2, 3, 4, 5].map((i) => ({
      cls: "wb-swatch",
      label: hl && i === 0 ? app.t("hl.0") : app.t(`ink.${i}`),
      style: `--sw:var(${hl && i === 0 ? "--hl" : `--ink-${i + 1}`})`,
    }));
  }

  function widthOptions(text) {
    if (text) {
      return TEXT_SIZES.map((n, i) => ({ cls: "wb-width wb-width--text", label: app.t("props.textSize", { n }), children: [document.createTextNode(["S", "M", "L"][i])] }));
    }
    return WIDTHS.map((n) => ({ cls: "wb-width", label: app.t("props.thicknessPx", { n }), children: [el("span", { style: `height:${n}px` })] }));
  }

  function noteOptions() {
    return NOTE_VARS.map((v, i) => ({ cls: "wb-swatch wb-swatch--note", label: app.t(`note.${i}`), style: `--sw:var(${v})` }));
  }

  function sep() {
    return el("span", { class: "wb-props__sep" });
  }

  function sizeFields(tool) {
    const [w, h] = app.size[tool];
    return el("div", { class: "wb-props__group" }, [
      numField(app.t("props.width"), app.t("props.widthLabel"), w, (v) => act.setSize(tool, 0, v)),
      numField(app.t("props.height"), app.t("props.heightLabel"), h, (v) => act.setSize(tool, 1, v)),
    ]);
  }

  function selectionProps(objs) {
    const children = [el("span", { class: "wb-props__label", text: app.t.n("sel.count", objs.length) })];
    const inked = objs.filter((o) => o.k !== "note");
    const notes = objs.filter((o) => o.k === "note");
    if (inked.length > 0) {
      const ink = inked[0].ink;
      const hl = inked.every((o) => o.k === "hl");
      children.push(sep(), radioGroup(app.t("props.colour"), inkOptions(hl), ink, (i) => act.setSelection({ ink: i })));
      const lined = inked.filter((o) => o.k !== "text");
      if (lined.length > 0) children.push(sep(), radioGroup(app.t("props.thickness"), widthOptions(false), lined[0].w, (i) => act.setSelection({ w: i })));
    }
    if (notes.length > 0) children.push(sep(), radioGroup(app.t("props.noteColour"), noteOptions(), notes[0].fill, (i) => act.setSelection({ fill: i })));
    const opButton = (name, label, op) => {
      const b = el("button", { class: "wb-mini", type: "button", "aria-label": label, title: label }, [icon(ICONS[name], 16)]);
      b.addEventListener("click", () => act.selectionOp(op));
      return b;
    };
    const ops = [opButton("front", app.t("sel.front"), "front"), opButton("back", app.t("sel.back"), "back"), opButton("dup", app.t("sel.dup"), "dup"), opButton("clear", app.t("sel.delete"), "delete")];
    if (objs.length === 1 && (objs[0].k === "note" || objs[0].k === "text")) ops.unshift(opButton("edit", app.t("sel.edit"), "edit"));
    children.push(sep(), el("div", { class: "wb-props__group" }, ops));
    if (objs.length === 1) {
      const o = objs[0];
      if (o.k !== "stroke" && o.k !== "hl" && o.k !== "text") {
        children.push(
          sep(),
          el("div", { class: "wb-props__group" }, [
            numField(app.t("props.width"), app.t("props.widthLabel"), o.g[2], (v) => act.setSelGeom(2, v)),
            numField(app.t("props.height"), app.t("props.heightLabel"), o.g[3], (v) => act.setSelGeom(3, v)),
          ]),
        );
      }
      const alt = el("input", { id: "alt-input", class: "wb-alt", type: "text", maxlength: "200", value: o.alt, placeholder: app.t("props.altPlaceholder"), "aria-label": app.t("props.alt") });
      alt.addEventListener("change", () => act.setSelection({ alt: alt.value.slice(0, 200) }));
      alt.addEventListener("keydown", (e) => {
        if (e.key === "Enter") {
          alt.blur();
          $("wb").focus();
        }
      });
      children.push(sep(), alt);
    }
    return children;
  }

  function renderProps() {
    const sel = [...app.selected].map((id) => app.store.get(id)).filter(Boolean);
    const tool = app.tool;
    let children = [];
    let label = app.t("props.label", { tool: app.t(`tool.${tool}`) });
    if (app.status.canDraw && tool === "select" && sel.length > 0) {
      label = app.t("props.selection");
      children = selectionProps(sel);
    } else if (app.status.canDraw) {
      if (["pen", "shapes", "arrow", "text"].includes(tool)) {
        children.push(radioGroup(app.t("props.colour"), inkOptions(false), app.ink, (i) => act.setInk(i)));
        children.push(sep(), radioGroup(app.t("props.thickness"), widthOptions(tool === "text"), app.widths[tool], (i) => act.setWidth(i)));
      } else if (tool === "hl") {
        children.push(radioGroup(app.t("props.colour"), inkOptions(true), app.hlInk, (i) => act.setInk(i)));
        children.push(sep(), radioGroup(app.t("props.thickness"), widthOptions(false), app.widths.hl, (i) => act.setWidth(i)));
      } else if (tool === "note") {
        children.push(radioGroup(app.t("props.noteColour"), noteOptions(), app.fill, (i) => act.setFill(i)));
      }
      if (tool === "shapes") {
        const opts = SHAPES.map((s) => ({ cls: "wb-width", label: app.t(`kind.${s}`), children: [icon(ICONS[s], 16)] }));
        children.push(sep(), radioGroup(app.t("props.shape"), opts, SHAPES.indexOf(app.shape), (i) => act.setShape(SHAPES[i])));
      }
      if (tool === "shapes" || tool === "arrow" || tool === "note") children.push(sep(), sizeFields(tool));
    }
    const hadFocus = props.contains(document.activeElement) ? document.activeElement.getAttribute("aria-label") : null;
    props.replaceChildren(...children);
    props.setAttribute("aria-label", label);
    props.hidden = children.length === 0;
    if (hadFocus) {
      const again = [...props.querySelectorAll("[aria-label]")].find((n) => n.getAttribute("aria-label") === hadFocus);
      if (again) again.focus();
    }
  }

  // --- State-driven updates --------------------------------------------------------------------

  function updateTools() {
    const mine = app.status.presenting === app.me.author;
    for (const b of [...toolButtons, ...presentButtons]) {
      const id = b.dataset.tool;
      const on = id === app.tool;
      b.toggleAttribute("data-active", on);
      b.setAttribute("aria-pressed", String(on));
      b.disabled = !toolAllowed(app, id);
    }
    setTabStop(toolButtons, toolButtons.find((b) => b.dataset.tool === app.tool) || toolButtons[0]);
    setTabStop(presentButtons, presentButtons.find((b) => b.dataset.tool === app.tool) || presentButtons[0]);
    $("wb").dataset.tool = app.tool;
    toolbar.hidden = mine;
    presentbar.hidden = !mine;
    zoom.hidden = mine;
    actions.hidden = mine;
    $("readonly").hidden = app.status.canDraw || mine;
    $("readonly").textContent = app.t("readonly");
    renderProps();
  }

  function updateZoom() {
    if (zoomVal) zoomVal.textContent = app.t("zoom.value", { n: Math.round(app.cam.zoom * 100) });
  }

  function updateStatus() {
    const s = app.status;
    undoBtn.disabled = !s.undo;
    redoBtn.disabled = !s.redo;
    clearBtn.hidden = !s.host;
    $("strip").hidden = s.relay;
    const saved = $("saved");
    saved.hidden = !s.saved || s.presenting === app.me.author;
    if (s.saved) saved.replaceChildren(icon(ICONS.check, 12), document.createTextNode(` ${s.saved}`));
    // Queue chip: "Sending n strokes…" with a bar that fills as the queue drains; also loading progress.
    const queue = $("queue");
    if (s.loading !== null) {
      queue.hidden = false;
      $("queue-text").textContent = app.t("loading", { n: s.loading });
      $("queue-bar").style.width = `${s.loading}%`;
    } else if (s.queue > 0) {
      queuePeak = Math.max(queuePeak, s.queue);
      queue.hidden = false;
      $("queue-text").textContent = app.t.n("queue", s.queue);
      $("queue-bar").style.width = `${Math.round((1 - s.queue / (queuePeak + 1)) * 100)}%`;
    } else {
      queuePeak = 0;
      queue.hidden = true;
    }
    updateBanner();
    updateTools();
  }

  function whoStack(peers) {
    const stack = el("div", { class: "wb-who-stack", role: "list", "aria-label": app.t("present.followers", { names: peers.map((p) => p.name).join(", ") }) });
    for (const p of peers.slice(0, 5)) {
      const c = personColour(p.color);
      stack.append(
        el("span", { class: "wb-who", role: "listitem", style: `--c:${c}`, title: p.name }, [
          el("span", { "aria-hidden": "true", text: [...p.name][0]?.toUpperCase() ?? "?" }),
          el("span", { class: "wb-who__shape" }, [shapeGlyph(p.shape, 8, "#fff")]),
        ]),
      );
    }
    return stack;
  }

  function updateBanner() {
    const s = app.status;
    const others = [...app.peers.values()].filter((p) => p.author !== app.me.author);
    banner.classList.remove("wb-banner--follow");
    if (s.presenting === null) {
      banner.hidden = true;
      return;
    }
    banner.hidden = false;
    if (s.presenting === app.me.author) {
      const stop = el("button", { class: "wb-banner__btn", type: "button" }, [document.createTextNode(app.t("present.stop")), el("span", { class: "kbd", text: "Esc" })]);
      stop.addEventListener("click", () => act.present(false));
      banner.replaceChildren(whoStack(others), el("span", {}, [el("b", { text: app.t("present.you") }), document.createTextNode(` · ${app.t.n("present.count", others.length)}`)]), stop);
      return;
    }
    const presenter = app.peers.get(s.presenting);
    const name = presenter ? presenter.name : app.t("outline.someone");
    banner.classList.add("wb-banner--follow");
    const btn = el("button", { class: "wb-banner__btn", type: "button", text: s.following ? app.t("present.stopFollow") : app.t("present.follow") });
    btn.addEventListener("click", () => act.follow(!s.following));
    banner.replaceChildren(el("span", {}, [el("b", { text: s.following ? app.t("present.following", { name }) : app.t("present.other", { name }) })]), btn);
  }

  // --- Outline ----------------------------------------------------------------------------------

  function nameOf(author) {
    if (author === app.me.author) return app.t("outline.you");
    const p = app.peers.get(author);
    return p ? p.name : app.t("outline.someone");
  }

  function renderFilter() {
    const authors = new Set(app.store.list().map((o) => o.by));
    const value = filter.value;
    filter.replaceChildren(el("option", { value: "", text: app.t("outline.everyone") }));
    for (const a of authors) filter.append(el("option", { value: String(a), text: nameOf(a) }));
    filter.value = [...filter.options].some((o) => o.value === value) ? value : "";
  }

  function renderOutline() {
    outlineTimer = 0;
    renderFilter();
    const objs = app.store.list().filter((o) => !app.hidden.has(o.by));
    const entries = outlineEntries(objs, app.t, nameOf, outlineAuthor);
    const focusId = tree.contains(document.activeElement) ? document.activeElement.dataset.id : null;
    const items = entries.map((e) => {
      const li = el("li", { role: "treeitem", tabindex: "-1", "data-id": e.id, "aria-selected": String(app.selected.has(e.id)), text: e.label });
      li.addEventListener("click", () => li.focus());
      return li;
    });
    tree.replaceChildren(...items);
    $("outline-empty").hidden = items.length > 0;
    const stop = items.find((li) => li.dataset.id === (focusId ?? focusedOutline)) || items[0];
    if (stop) {
      stop.tabIndex = 0;
      if (focusId) stop.focus();
    }
  }

  function scheduleOutline() {
    if (!outlineTimer) outlineTimer = setTimeout(renderOutline, 200);
  }

  filter.addEventListener("change", () => {
    outlineAuthor = filter.value === "" ? null : Number(filter.value);
    renderOutline();
  });

  tree.addEventListener("focusin", (e) => {
    const li = e.target.closest("li[role=treeitem]");
    if (!li) return;
    for (const n of tree.children) n.tabIndex = n === li ? 0 : -1;
    focusedOutline = li.dataset.id;
    act.selectOnly(li.dataset.id, true);
    for (const n of tree.children) n.setAttribute("aria-selected", String(n === li));
  });

  tree.addEventListener("keydown", (e) => {
    const li = e.target.closest("li[role=treeitem]");
    if (!li) return;
    const items = [...tree.children];
    const i = items.indexOf(li);
    let j = -1;
    if (e.key === "ArrowDown") j = Math.min(items.length - 1, i + 1);
    else if (e.key === "ArrowUp") j = Math.max(0, i - 1);
    else if (e.key === "Home") j = 0;
    else if (e.key === "End") j = items.length - 1;
    if (j >= 0) {
      e.preventDefault();
      items[j].focus();
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      act.editOrDescribe(li.dataset.id);
    } else if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      const next = items[i + 1] || items[i - 1];
      focusedOutline = next ? next.dataset.id : null;
      act.deleteIds([li.dataset.id]);
      if (next) next.focus();
    }
  });

  function setOutlineOpen(open) {
    outline.toggleAttribute("data-open", open);
    outlineBtn.setAttribute("aria-pressed", String(open));
    if (open) {
      renderOutline();
      (tree.querySelector("[tabindex='0']") || filter).focus();
    }
  }

  // --- Help -------------------------------------------------------------------------------------

  $("help-close").addEventListener("click", () => help.close());
  help.addEventListener("close", () => $("wb").focus());

  $("strip-why").addEventListener("click", () => {
    const btn = $("strip-why");
    const open = btn.getAttribute("aria-expanded") !== "true";
    btn.setAttribute("aria-expanded", String(open));
    $("strip-why-text").hidden = !open;
  });

  return {
    build() {
      buildStatic();
      buildToolbar();
      buildPresentbar();
      buildZoom();
      buildActions();
      updateStatus();
      updateZoom();
      renderOutline();
    },
    updateTools,
    updateZoom,
    updateStatus,
    updateBanner,
    renderProps,
    scheduleOutline,
    toggleOutline() {
      setOutlineOpen(!outline.hasAttribute("data-open"));
    },
    openHelp() {
      if (!help.open) help.showModal();
    },
    helpOpen() {
      return help.open;
    },
  };
}
