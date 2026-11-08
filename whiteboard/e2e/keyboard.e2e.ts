// Whiteboard, issue #397: keyboard only. Inside the board A draws with the keyboard cursor
// (P, arrows, Space to press and lift the pen), places and types a sticky note (N, Enter, typing,
// Escape), selects everything (Ctrl+A), deletes, undoes and redoes, zooms with + and -, and walks
// the board outline. The board is started from the Apps menu with the keyboard as well.
import { $, browser, expect } from "@wdio/globals";
import { app, registry } from "../../helpers/index.ts";
import { composer } from "../../helpers/chat-ui.ts";
import { inBoard, note, openOutline, outline, setUpClient, stageFrame, startItem, startRelay, waitCount } from "./board.ts";

async function zoomPercent(): Promise<number> {
  const text = await $("#zoom").getText();
  const match = /(\d+)%/.exec(text);
  if (!match) throw new Error(`no zoom value in ${JSON.stringify(text)}`);
  return Number(match[1]);
}

async function focusBoard(): Promise<void> {
  await browser.execute(() => document.getElementById("wb")?.focus());
}

describe("Whiteboard: keyboard only", () => {
  let fixtures: registry.FixtureRegistry;
  /** The Mumble server of the relay environment. */
  let port = 0;

  before(async function () {
    this.timeout(20 * 60_000);
    fixtures = await registry.start();
    port = await startRelay();
  });

  after(async () => {
    await fixtures.stop();
  });

  it("starts the board from the Apps menu with the keyboard", async () => {
    await app.restart({ env: fixtures.env() });
    await setUpClient("Alice", port);
    // From the composer, Tab reaches the Apps button below it.
    await composer().click();
    const apps = $("button.chat-apps__button");
    for (let i = 0; i < 6 && !(await apps.isFocused()); i++) await browser.keys("Tab");
    await expect(apps).toBeFocused();
    await browser.keys("Enter");
    const item = startItem();
    await item.waitForDisplayed({ timeout: 10_000 });
    for (let i = 0; i < 6 && !(await item.isFocused()); i++) await browser.keys("ArrowDown");
    await expect(item).toBeFocused();
    await browser.keys("Enter");
    await stageFrame().waitForDisplayed({ timeout: 30_000 });
  });

  it("draws, places a note, selects, deletes, undoes, redoes and zooms", async () => {
    await inBoard(async () => {
      await focusBoard();
      await browser.keys("p");
      await browser.keys("ArrowLeft");
      await browser.keys(" ");
      for (let i = 0; i < 5; i++) await browser.keys("ArrowRight");
      await browser.keys(" ");
      await waitCount(1);

      await note("Agenda", ["ArrowDown", "ArrowDown"]);
      const lines = await waitCount(2);
      expect(lines.some((l) => l.includes("Sticky note") && l.includes("'Agenda'"))).toBe(true);

      await focusBoard();
      await browser.keys(["Control", "a"]);
      await expect($("#props")).toHaveText(expect.stringContaining("2 selected"), { wait: 5_000 });
      await browser.keys("Delete");
      await waitCount(0);
      await browser.keys(["Control", "z"]);
      await waitCount(2);
      await browser.keys(["Control", "Shift", "z"]);
      await waitCount(0);
      await browser.keys(["Control", "z"]);
      await waitCount(2);

      const before = await zoomPercent();
      await browser.keys("+");
      await browser.waitUntil(async () => (await zoomPercent()) > before, {
        timeout: 5_000,
        timeoutMsg: "+ did not zoom in",
      });
      await browser.keys("-");
      await browser.waitUntil(async () => (await zoomPercent()) === before, {
        timeout: 5_000,
        timeoutMsg: "- did not zoom back out",
      });
    });
  });

  it("walks the board outline and edits the note from it", async () => {
    await inBoard(async () => {
      await openOutline();
      const items = $("#outline-tree");
      await browser.execute(() =>
        document.querySelector<HTMLElement>("#outline-tree li[data-id]")?.focus(),
      );
      // Reading order: find the note with the arrow keys, then Enter edits it.
      for (let i = 0; i < 3; i++) {
        const focused = await browser.execute(() => document.activeElement?.textContent ?? "");
        if (focused.includes("Sticky note")) break;
        await browser.keys("ArrowDown");
      }
      await expect(items.$("li:focus")).toHaveText(expect.stringContaining("'Agenda'"));
      await browser.keys("Enter");
      await $("#editor").waitForDisplayed({ timeout: 10_000 });
      await browser.keys(" today");
      await browser.keys("Escape");
      await browser.waitUntil(async () => (await outline()).some((l) => l.includes("'Agenda today'")), {
        timeout: 10_000,
        timeoutMsg: "the edited note text is not in the outline",
      });
    });
  });
});
