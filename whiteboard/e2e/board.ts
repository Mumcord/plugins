// Helpers for the Whiteboard specs: setting up a client, starting and joining a board, and driving
// the board page inside its sealed frame (whiteboard/web/board). The page shows no state hash, so
// boards are compared through the board outline: one line per object in reading order, with the
// object id, kind, colour, text and position. Equal outlines on every client mean equal boards.
import { $, $$, browser, expect } from "@wdio/globals";
import { relayenv } from "../../helpers/index.ts";
import { composer, joinChannel } from "../../helpers/chat-ui.ts";
import { registerOnServer } from "../../helpers/files-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import { addAndConnect, completeWizard } from "../../helpers/voice-ui.ts";
import { closeSettings, developerMode, install } from "../official-shared/a11y.ts";

/**
 * Plugin data travels on the voice server only, so every client joins this channel: the root
 * channel of the mumpeer test environment's server (helpers/relayenv.ts).
 */
export const ROOT = "Mumpeer Test Server";
/** The root channel of the plain test server (`testenv.up("v15")`), which has no relay. */
export const PLAIN_ROOT = "Mumcord Test Server";
export const PLUGIN = "Whiteboard";
/** The people of the specs; their names are masked when outlines are compared. */
export const PEOPLE = ["Alice", "Bob", "Carol"];
export const START = "Start whiteboard";

/** Mumcord's own UI (outside the board frame). */
export const inviteCards = () =>
  $$(`section.plugin-chat-card[aria-label='Card from ${PLUGIN}']`);
/** A host dialog by its title; double quotes in the XPath, because titles hold apostrophes. */
export const pluginDialog = (title: string) =>
  $(`//div[@role='dialog'][.//*[normalize-space(.)="${title}"]]`);
/** The board on the stage (the media area); the side panel has its own frame. */
export const stageFrame = () => $(".plugin-stage iframe.plugin-webview__frame");
/** The mini view in the side panel next to the chat. */
export const panelFrame = () => $(".plugin-sidepanel__body iframe.plugin-webview__frame");

/**
 * Starts the mumpeer test environment with a relay that allows plugin data and returns the port of
 * its Mumble server. The board travels only over the relay, so every board spec needs it.
 */
export async function startRelay(): Promise<number> {
  return (await relayenv.startRelay("plugin-data")).port;
}

/**
 * Wizard, connection, the shared channel and the plugin with every optional permission. With
 * `port`, the client connects to the relay environment's server and registers (the relay bot gives
 * relay tokens to registered users only); without, to the plain test server.
 */
export async function setUpClient(name: string, port?: number): Promise<void> {
  await completeWizard(name);
  await developerMode();
  await closeSettings();
  if (port === undefined) {
    await addAndConnect();
    await joinChannel(PLAIN_ROOT);
  } else {
    await addAndConnect(`localhost:${String(port)}`);
    await joinChannel(ROOT);
    await registerOnServer();
  }
  await openSettingsGroup("Plugins");
  await install(PLUGIN);
  await closeSettings();
}

/**
 * The "Start whiteboard" item of the open Apps menu. Matched on its label text alone: the item also
 * holds the plugin's tile letter, its name and, when disabled, the reason.
 */
export const startItem = () =>
  $(
    `//div[contains(@class,'chat-apps__menu')]//button[contains(@class,'appmenu__item')][.//span[contains(@class,'appmenu__text')]/text()[normalize-space()='${START}']]`,
  );

/** "Start whiteboard" from the composer's Apps menu. */
export async function startFromAppsMenu(): Promise<void> {
  await $("button.chat-apps__button").click();
  const item = startItem();
  // The entry stays disabled until the client holds a relay token for this server.
  await item.waitForClickable({ timeout: 60_000 });
  await item.click();
  await stageFrame().waitForDisplayed({ timeout: 30_000 });
}

/** Clicks "Join" on the newest invite card and waits for the board on the stage. */
export async function joinFromCard(): Promise<void> {
  await browser.waitUntil(async () => (await inviteCards().length) > 0, {
    timeout: 30_000,
    timeoutMsg: "no Whiteboard invite card",
  });
  const cards = await inviteCards();
  await cards[cards.length - 1].$(".//button[normalize-space(.)='Join']").click();
  await stageFrame().waitForDisplayed({ timeout: 30_000 });
}

/**
 * Replaces the composer text the way React sees typing (native setter + input event), like the
 * Dice spec: WebDriver `setValue` keeps the old text of this controlled box.
 */
export async function typeInComposer(text: string): Promise<void> {
  const box = composer();
  await box.waitForEnabled();
  await box.click();
  await browser.execute((value: string) => {
    const field = document.querySelector<HTMLTextAreaElement>("textarea[aria-label^='Message']");
    if (!field) throw new Error("no composer");
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set?.call(
      field,
      value,
    );
    field.dispatchEvent(new Event("input", { bubbles: true }));
  }, text);
  // A trailing space is part of the command text, so it must not be trimmed away.
  await expect(box).toHaveValue(text, { trim: false });
}

/** Runs `fn` inside the board frame on the stage (or `frame`), then returns to Mumcord. */
export async function inBoard<T>(fn: () => Promise<T>, frame = stageFrame): Promise<T> {
  const el = frame();
  await el.waitForExist({ timeout: 30_000 });
  await browser.switchFrame(el);
  try {
    return await fn();
  } finally {
    await browser.switchFrame(null);
  }
}

/** Picks a tool from the board's toolbar (inside the frame). */
export async function tool(id: string): Promise<void> {
  const button = $(`#toolbar button[data-tool='${id}']`);
  await button.waitForClickable({ timeout: 15_000 });
  await button.click();
}

/** A board action button by its label, e.g. "Board settings" (inside the frame). */
export const action = (label: string) => $(`button[aria-label='${label}']`);

/**
 * Drags the mouse along `points` (pixels from the board's centre) over the drawing layer: draws
 * with the pen, moves with select, erases with the eraser (inside the frame).
 */
export async function drag(points: [number, number][]): Promise<void> {
  const overlay = $("#overlay");
  await overlay.waitForDisplayed({ timeout: 15_000 });
  const [first, ...rest] = points;
  const pointer = browser.action("pointer", { parameters: { pointerType: "mouse" } });
  pointer.move({ x: first[0], y: first[1], origin: overlay }).down({ button: 0 });
  for (const [x, y] of rest) pointer.move({ x, y, origin: overlay, duration: 30 });
  await pointer.up({ button: 0 }).perform();
}

/**
 * Draws a short pen stroke from (x, y) to the right (inside the frame). Points are relative to the
 * board's centre; in the test window the free drawing area is about x -210…+250, y -90…+90 (a
 * joiner's stage is shorter): the tool rail, the toolbar with the pen options, the zoom bar and
 * the save status sit around it and would take the pointer instead.
 */
export async function stroke(x: number, y: number, length = 80): Promise<void> {
  await tool("pen");
  await drag([
    [x, y],
    [x + length / 3, y + 10],
    [x + (2 * length) / 3, y - 10],
    [x + length, y],
  ]);
}

/**
 * Adds a sticky note with `text` at the keyboard cursor (inside the frame): N, an arrow key to
 * show the cursor, Enter to place, type, Escape to finish.
 */
export async function note(text: string, steps: string[] = ["ArrowRight"]): Promise<void> {
  await browser.execute(() => document.getElementById("wb")?.focus());
  await browser.keys("n");
  for (const key of steps) await browser.keys(key);
  await browser.keys("Enter");
  await $("#editor").waitForDisplayed({ timeout: 10_000 });
  await browser.keys(text);
  await browser.keys("Escape");
  await $("#editor").waitForDisplayed({ reverse: true, timeout: 10_000 });
}

/** Places a sticky note with `text` at (x, y) with the note tool and a click (inside the frame). */
export async function noteAt(x: number, y: number, text: string): Promise<void> {
  await tool("note");
  await drag([[x, y]]);
  await $("#editor").waitForDisplayed({ timeout: 10_000 });
  await browser.keys(text);
  await browser.keys("Escape");
  await $("#editor").waitForDisplayed({ reverse: true, timeout: 10_000 });
}

/** Drags whatever lies at (x, y) by (dx, dy) with the select tool (inside the frame). */
export async function moveAt(x: number, y: number, dx: number, dy: number): Promise<void> {
  await tool("select");
  await drag([
    [x, y],
    [x + dx / 2, y + dy / 2],
    [x + dx, y + dy],
  ]);
}

/** Wipes the eraser across (x, y) (inside the frame). */
export async function eraseAt(x: number, y: number): Promise<void> {
  await tool("eraser");
  await drag([
    [x - 10, y - 20],
    [x, y],
    [x + 10, y + 20],
  ]);
}

const outlineOpen = async (): Promise<boolean> =>
  (await $("#outline").getAttribute("data-open")) !== null;

/** Opens the board outline panel with its toolbar button and leaves it open (inside the frame). */
export async function openOutline(): Promise<void> {
  if (await outlineOpen()) return;
  await action("Board outline").click();
  await browser.waitUntil(outlineOpen, {
    timeout: 10_000,
    timeoutMsg: "the board outline did not open",
  });
}

/**
 * The board outline (inside the frame): "<object id> <description>" per object in reading order,
 * with every person's name masked ("you" on the own client, the name elsewhere). The page keeps
 * the list current while the panel is closed, so it is read without opening it: opening focuses
 * the first item, which selects that object and moves the view to it.
 */
export async function outline(): Promise<string[]> {
  return browser.execute((people: string[]) => {
    const names = [...people, "you", "someone"].join("|");
    const mask = new RegExp(` by (${names}),`, "g");
    return Array.from(document.querySelectorAll<HTMLElement>("#outline-tree li[data-id]"), (li) =>
      `${li.dataset.id ?? ""} ${li.textContent.replace(mask, " by *,")}`,
    );
  }, PEOPLE);
}

/** Waits until the board outline has `count` objects (inside the frame). */
export async function waitCount(count: number, timeout = 30_000): Promise<string[]> {
  let lines: string[] = [];
  // The message is built after the wait, so it shows the last outline read.
  await browser
    .waitUntil(
      async () => {
        lines = await outline();
        return lines.length === count;
      },
      { timeout, interval: 250 },
    )
    .catch((e: unknown) => {
      throw new Error(
        `expected ${String(count)} objects, outline: ${JSON.stringify(lines)} (${String(e)})`,
      );
    });
  return lines;
}

/**
 * Creates `count` pen strokes of `points` random-walk points each straight through the page's
 * bridge, as the pen tool would after a pen lift (inside the frame). Random walks barely
 * compress: about 4 bytes per point on the wire, so 70 × 400 points make a board of ~110 KiB.
 */
export async function bulkStrokes(count: number, points: number, seed: number): Promise<void> {
  await browser.execute(
    (count: number, points: number, seed: number) =>
      new Promise<void>((done) => {
        let state = seed >>> 0;
        const random = () => {
          state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
          return state / 2 ** 32;
        };
        let i = 0;
        // Paced like the page itself (bridge.js stays below the host's 60 messages per second).
        const next = () => {
          if (i === count) {
            done();
            return;
          }
          const pts: number[] = [];
          let x = 100;
          let y = 100;
          for (let p = 0; p < points; p++) {
            x = Math.min(200, Math.max(0, x + (random() - 0.5) * 20));
            y = Math.min(200, Math.max(0, y + (random() - 0.5) * 20));
            pts.push(Math.round(x * 4) / 4, Math.round(y * 4) / 4);
          }
          // Far above the view, so the other specs' clicks and the eraser never touch them.
          const g = [(i % 10) * 220 - 1100, Math.floor(i / 10) * 220 - 4000, 200, 200];
          const obj = { k: "stroke", g, pts, bw: 200, bh: 200, ink: i % 6, w: 1 };
          // Every page message names the place it comes from (bridge.js).
          window.parent.postMessage(JSON.stringify({ p: "stage", t: "create", obj }), "*");
          i += 1;
          setTimeout(next, 40);
        };
        next();
      }),
    count,
    points,
    seed,
  );
}

/** One client's outline: the stage frame of the client `on` points at. */
export type OnClient = <T>(fn: () => Promise<T>) => Promise<T>;
export const onA: OnClient = (fn) => fn();

/**
 * Waits until every client shows the same board with `count` objects and returns it. Each round
 * reads the clients one after another (they share the WebDriver globals).
 */
export async function waitSameBoard(
  clients: OnClient[],
  count: number,
  timeout = 60_000,
): Promise<string[]> {
  let boards: string[][] = [];
  // The message is built after the wait, so it shows the last boards read, not the first.
  await browser
    .waitUntil(
      async () => {
        boards = [];
        for (const on of clients) boards.push(await on(() => inBoard(outline)));
        const first = JSON.stringify(boards[0]);
        return boards[0].length === count && boards.every((b) => JSON.stringify(b) === first);
      },
      { timeout, interval: 1_000 },
    )
    .catch((e: unknown) => {
      throw new Error(
        `boards differ: ${JSON.stringify(boards.map((b) => b.length))} objects, expected ${String(count)} (${String(e)})`,
      );
    });
  return boards[0];
}

/**
 * Picks `option` in the labelled choice of an open plugin dialog of Mumcord: a combobox whose
 * list opens on click.
 */
export async function dialogChoice(label: string, option: string): Promise<void> {
  const box = $(
    `//div[@role='dialog']//*[@role='combobox'][@id=//div[@role='dialog']//label[normalize-space(.)="${label}"]/@for]`,
  );
  await box.click();
  const item = $(
    `//*[@role='listbox']//*[@role='option'][normalize-space(.)="${option}" or .//*[normalize-space(.)="${option}"]]`,
  );
  await item.waitForClickable({ timeout: 10_000 });
  await item.click();
  await item.waitForExist({ reverse: true, timeout: 10_000 });
}

/**
 * A labelled control in an open plugin dialog of Mumcord (radio, checkbox, switch). The label goes
 * into double quotes, because labels hold apostrophes ("Hide Bob's strokes for me").
 */
export function dialogControl(label: string) {
  return $(
    `//div[@role='dialog']//input[@id=//div[@role='dialog']//label[normalize-space(.)="${label}"]/@for]` +
      ` | //div[@role='dialog']//label[normalize-space(.)="${label}"]//input`,
  );
}

/** A button in an open plugin dialog of Mumcord. */
export const dialogButton = (label: string) =>
  $(`//div[@role='dialog']//button[normalize-space(.)='${label}']`);

/** An in-board dialog of the Whiteboard page (Export or share, Clear), by its heading. */
export const boardDialog = (title: string) =>
  $(`//dialog[@open][.//h2[normalize-space(.)='${title}']]`);

/** A button in the open in-board dialog. */
export const boardButton = (label: string) =>
  $(`//dialog[@open]//button[normalize-space(.)='${label}']`);

/** A radio or switch in the open in-board dialog, by its bold label. */
export const boardOption = (label: string) =>
  $(`//dialog[@open]//label[.//b[normalize-space(.)='${label}']]//input`);

/** The outcome line of the open in-board dialog. */
export const boardNote = () => $("//dialog[@open]//p[@role='status']");

/** The page's notice under the board (cleared with Undo, or an outcome after a dialog closed). */
export const boardToast = () => $(".wb-toast");
