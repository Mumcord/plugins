// Whiteboard, issue #397: export and post to chat (A alone, plain Mumble user C as mumbot).
// - Export saves PNG, SVG, PDF and the board file through Mumcord's save window; names are off by
//   default, and the plugin never sees a path.
// - Post to chat sends the whole board as a PNG with one click; Mumcord shows it inline and a
//   standard client (mumbot) gets the image message.
// Both start from the board's own Export dialog (mockups 5, 5b); its Save and Post buttons are the
// user's gesture (FR-9.45).
import { existsSync, readdirSync } from "node:fs";
import { $, expect } from "@wdio/globals";
import { app, registry, testenv } from "../../helpers/index.ts";
import { ScriptBot } from "../../helpers/chat-ui.ts";
import {
  action,
  boardButton,
  boardDialog,
  boardNote,
  boardOption,
  inBoard,
  noteAt,
  setUpClient,
  startFromAppsMenu,
  stroke,
} from "./board.ts";

const EXPORT_TITLE = 'Export or share "Untitled board"';

describe("Whiteboard: export and post to chat", () => {
  let fixtures: registry.FixtureRegistry;
  let carol: ScriptBot | undefined;

  before(async function () {
    this.timeout(20 * 60_000);
    fixtures = await registry.start();
    await testenv.up("v15");
  });

  after(async () => {
    carol?.stop();
    await fixtures.stop();
  });

  it("A draws a board", async () => {
    await app.restart({ env: fixtures.env() });
    await setUpClient("Alice");
    await startFromAppsMenu();
    await inBoard(async () => {
      await stroke(-100, -50);
      await noteAt(50, 0, "Export me");
    });
  });

  it("exports PNG, SVG, PDF and the board file without names", async () => {
    const downloads = app.downloadsDir();
    for (const format of ["PNG image", "SVG drawing", "PDF document", "Board file"]) {
      await inBoard(async () => {
        await action("Export").click();
        await boardDialog(EXPORT_TITLE).waitForDisplayed({ timeout: 15_000 });
        await boardOption("Save to a file").click();
        await boardOption(format).click();
        await expect(boardOption("Add names of who drew what")).not.toBeSelected();
        await boardButton("Save").click();
        // Mumcord's save window; the test backend saves into the downloads folder.
        await expect(boardNote()).toHaveText(expect.stringContaining("Saved"), { wait: 30_000 });
        await boardButton("Cancel").click();
      });
    }
    const files = existsSync(downloads) ? readdirSync(downloads) : [];
    for (const ext of [".png", ".svg", ".pdf"]) {
      expect(files.some((f) => f.endsWith(ext))).toBe(true);
    }
  });

  it("posts the board as a picture that a standard client receives", async () => {
    carol = new ScriptBot({ name: "Carol", timeout: 60 });
    await carol.connected();
    const picture = carol.run("expect message --from Alice --contains \"<img\"", 60_000);
    await inBoard(async () => {
      await action("Post board to chat").click();
      await boardDialog(EXPORT_TITLE).waitForDisplayed({ timeout: 15_000 });
      await expect(boardOption("Post to chat")).toBeSelected();
      await boardButton("Post to chat").click();
      await expect(boardNote()).toHaveText(expect.stringContaining("Posted to chat as a picture."), {
        wait: 30_000,
      });
      await boardButton("Cancel").click();
    });
    await expect($("//li[contains(@class,'chat-message')]//img")).toBeDisplayed({ wait: 15_000 });
    await picture;
  });
});
