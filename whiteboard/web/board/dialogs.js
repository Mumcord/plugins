// The in-board dialogs: Export or share (mockups 5, 5b) and the Clear confirmation (mockup 6b),
// plus the "Board cleared · Undo" notice. A genuine click on their buttons counts as the user's
// gesture for saving a file or posting to chat (spec FR-9.45), so the plugin acts on the button's
// message right away. The plugin fills in the texts that count things (`dialog` message) and
// reports each outcome (`note`); everything else is the page's own text.

import { el } from "./dom.js";

/** Formats each export destination accepts; a chat post is always a PNG. */
const FORMATS = ["png", "svg", "pdf", "board"];
const AREAS = ["board", "view", "selection"];

/**
 * `t` reads the current texts, `send` talks to the plugin, `scope()` returns what the page knows
 * for an export (`{ view: [x, y, w, h], sel: [ids] }`) and `preview(canvas, area, transparent)`
 * draws the area into the dialog's preview and returns its size in pixels.
 */
export function createDialogs(t, send, scope, preview) {
  let open = null; // { dialog, note, primary, busy }
  let toast = null;
  let toastTimer = 0;

  const close = () => {
    if (!open) return;
    const d = open.dialog;
    open = null;
    if (d.open) d.close();
    d.remove();
  };

  const shell = (titleId, role) => {
    const dialog = el("dialog", { class: "wb-dialog wb-dialog--board", "aria-labelledby": titleId, role });
    dialog.addEventListener("close", () => {
      if (open?.dialog === dialog) close();
    });
    document.body.append(dialog);
    return dialog;
  };

  const radio = (name, value, label, hint, checked) =>
    el("label", { class: "wb-opt" }, [
      el("input", { type: "radio", name, value, checked }),
      el("span", { class: "wb-opt__t" }, [el("b", { text: label }), hint ? el("span", { text: hint }) : null]),
    ]);

  const fieldset = (legend, children) => el("fieldset", { class: "wb-fs" }, [el("legend", { text: legend }), ...children]);

  const select = (name, label, options, value) => {
    const s = el(
      "select",
      { name, class: "wb-select" },
      options.map(([v, text]) => el("option", { value: v, text, selected: v === value })),
    );
    return el("label", { class: "wb-field" }, [el("span", { text: label }), s]);
  };

  const noteLine = () => el("p", { class: "wb-note", role: "status", "aria-live": "polite" });

  function openExport(msg) {
    const toChat = msg.which === "post";
    const dialog = shell("wb-exp-t", "dialog");
    dialog.classList.add("wb-dialog--wide");
    const form = el("form", { method: "dialog", class: "wb-export-form" });
    const body = el("div", { class: "wb-export" });
    const note = noteLine();
    const privacy = el("p", { class: "wb-note wb-note--muted" });
    const primary = el("button", { class: "wb-button", type: "submit" });
    const cancel = el("button", { class: "wb-button wb-button--quiet", type: "button", text: t("dlg.cancel") });
    cancel.addEventListener("click", close);

    const canvas = el("canvas", { class: "wb-export__canvas", width: 480, height: 270, "aria-hidden": "true" });
    const meta = el("p", { class: "wb-muted wb-export__meta" });
    const right = el("div", { class: "wb-export__right" });
    body.append(el("div", { class: "wb-export__left" }, [el("div", { class: "wb-export__preview" }, [canvas]), meta]), right);
    right.append(
      fieldset(t("exp.sendTo"), [
        radio("dest", "file", t("exp.toFile"), t("exp.toFileHint"), !toChat),
        radio("dest", "chat", t("exp.toChat"), t("exp.toChatHint"), toChat),
      ]),
      fieldset(t("exp.format"), [
        radio("format", "png", t("exp.png"), t("exp.pngHint"), true),
        radio("format", "svg", t("exp.svg"), t("exp.filesOnly"), false),
        radio("format", "pdf", t("exp.pdf"), t("exp.filesOnly"), false),
        radio("format", "board", t("exp.board"), t("exp.boardHint"), false),
      ]),
      fieldset(t("exp.area"), [
        radio("area", "board", t("exp.areaBoard"), msg.board, !msg.has_selection),
        radio("area", "view", t("exp.areaView"), t("exp.areaViewHint"), false),
        radio("area", "selection", t("exp.areaSelection"), msg.selection, msg.has_selection),
      ]),
      el("div", { class: "wb-row2" }, [
        select("scale", t("exp.size"), [["1", t("exp.scale1")], ["2", t("exp.scale2")], ["4", t("exp.scale4")]], toChat ? "1" : "2"),
        select("bg", t("exp.background"), [["paper", t("exp.bgPaper")], ["none", t("exp.bgNone")]], "paper"),
      ]),
      el("label", { class: "wb-switch-row" }, [
        el("input", { type: "checkbox", name: "names", class: "wb-switch" }),
        el("span", {}, [el("b", { text: t("exp.names") }), el("span", { class: "wb-muted", text: t("exp.namesHint") })]),
      ]),
    );
    form.append(body, el("div", { class: "wb-export__foot" }, [note, privacy, el("div", { class: "wb-dlg-actions" }, [cancel, primary])]));
    dialog.append(el("h2", { id: "wb-exp-t", text: msg.title }), form);

    const value = (name) => form.querySelector(`input[name="${name}"]:checked`)?.value;
    const input = (name, v) => form.querySelector(`input[name="${name}"][value="${v}"]`);
    // Keeps the form consistent: chat takes only PNGs, a board file is always the whole board.
    const sync = () => {
      const chat = value("dest") === "chat";
      for (const f of FORMATS) input("format", f).disabled = chat && f !== "png";
      if (chat) input("format", "png").checked = true;
      const format = value("format");
      for (const a of AREAS) input("area", a).disabled = format === "board" || (a === "selection" && !msg.has_selection);
      if (format === "board") input("area", "board").checked = true;
      form.elements.scale.disabled = format !== "png";
      form.elements.bg.disabled = format === "board";
      form.elements.names.disabled = format !== "svg" && format !== "pdf";
      primary.textContent = chat ? t("exp.post") : t("exp.save");
      const size = preview(canvas, value("area"), form.elements.bg.value === "none" && format !== "board");
      const scale = format === "png" ? Number(form.elements.scale.value) : 1;
      meta.textContent = t("exp.meta", { w: size.w * scale, h: size.h * scale });
      privacy.textContent = chat ? t("exp.privacyChat") : t("exp.privacyFile");
    };
    form.addEventListener("change", sync);
    form.addEventListener("submit", (e) => {
      e.preventDefault();
      if (open?.busy) return;
      const { view, sel } = scope();
      send({
        t: "export",
        dest: value("dest"),
        format: value("format"),
        area: value("area"),
        scale: Number(form.elements.scale.value),
        transparent: form.elements.bg.value === "none",
        names: form.elements.names.checked,
        view,
        sel,
      });
      setBusy(true);
    });
    sync();
    open = { dialog, note, primary, busy: false };
    dialog.showModal();
  }

  function openClear(msg) {
    const dialog = shell("wb-clr-t", "alertdialog");
    dialog.classList.add("wb-dialog--small");
    dialog.setAttribute("aria-describedby", "wb-clr-d");
    const keep = el("button", { class: "wb-button wb-button--quiet", type: "button", text: t("clr.keep") });
    const clear = el("button", { class: "wb-button wb-button--danger", type: "button", text: t("clr.button") });
    keep.addEventListener("click", close);
    clear.addEventListener("click", () => {
      send({ t: "clear" });
      close();
    });
    const note = noteLine();
    dialog.append(
      el("h2", { id: "wb-clr-t", text: t("clr.title") }),
      el("p", { id: "wb-clr-d", class: "wb-dialog__text" }, [msg.body, " ", t("clr.undoHint")]),
      note,
      el("div", { class: "wb-dlg-actions" }, [keep, clear]),
    );
    open = { dialog, note, primary: clear, busy: false };
    dialog.showModal();
    // The safe choice has the focus, as in Mumcord's own confirmations.
    keep.focus();
  }

  function setBusy(busy) {
    if (!open) return;
    open.busy = busy;
    open.primary.disabled = busy;
    open.dialog.setAttribute("aria-busy", busy ? "true" : "false");
  }

  function hideToast() {
    clearTimeout(toastTimer);
    toast?.remove();
    toast = null;
  }

  function showToast(text, withUndo, untilMs) {
    hideToast();
    const undo = withUndo ? el("button", { class: "wb-button", type: "button", text: t("clr.undo") }) : null;
    undo?.addEventListener("click", () => send({ t: "undo-clear" }));
    toast = el("div", { class: "wb-toast glass", role: "status" }, [el("span", { text }), undo]);
    document.body.append(toast);
    toastTimer = setTimeout(hideToast, Math.max(1000, untilMs - Date.now()));
  }

  return {
    /** The plugin's `dialog` message: opens the dialog it names. */
    open(msg) {
      close();
      if (msg.which === "clear") openClear(msg);
      else if (msg.which === "export" || msg.which === "post") openExport(msg);
    },
    /** The plugin's `note`: the outcome of a button, shown where the user is looking. */
    note(msg) {
      if (typeof msg.text !== "string") return;
      if (open) {
        open.note.textContent = msg.text;
        open.note.dataset.tone = msg.tone;
        setBusy(msg.busy === true);
      } else {
        showToast(msg.text, false, Date.now() + 6000);
      }
    },
    /** The plugin's `cleared`: Undo is possible until `until` (Unix seconds). */
    cleared(msg) {
      const until = Number(msg.until) * 1000;
      if (Number.isFinite(until)) showToast(t("clr.done"), true, until);
    },
    isOpen: () => open !== null,
  };
}
