// Whiteboard, issue #397: export and post to chat (A alone, plain Mumble user C as mumbot).
// - Export saves PNG, SVG, PDF and the board file through Mumcord's save window; names are off by
//   default, and the plugin never sees a path.
// - Post to chat sends the whole board as a PNG with one click; Mumcord shows it inline and a
//   standard client (mumbot) gets the image message.
import { existsSync, readdirSync } from "node:fs";
import { $, browser, expect } from "@wdio/globals";
import { app, registry, testenv } from "../../helpers/index.ts";
import { ScriptBot } from "../../helpers/chat-ui.ts";
import {
  action,
  dialogButton,
  dialogControl,
  inBoard,
  noteAt,
  pluginDialog,
  setUpClient,
  startFromAppsMenu,
  stroke,
} from "./board.ts";

/** Why these tests are skipped until the host has the APIs. */
const BLOCKED = "blocked by host task HX1: files.save and chat.post-image are not in Mumcord yet";

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

  it.skip(`exports PNG, SVG, PDF and the board file without names (${BLOCKED})`, async () => {
    const downloads = app.downloadsDir();
    for (const format of ["PNG image", "SVG drawing", "PDF document", "Board file"]) {
      await inBoard(() => action("Export").click());
      await pluginDialog('Export "Untitled board"').waitForDisplayed({ timeout: 15_000 });
      await dialogControl(format).click();
      await expect(dialogControl("Add names of who drew what")).not.toBeSelected();
      await dialogButton("Save").click();
      // Mumcord's save window; the test backend saves into the downloads folder.
      await expect($("//div[@role='dialog']")).toHaveText(expect.stringContaining("Saved"), {
        wait: 30_000,
      });
      await browser.keys("Escape");
    }
    const files = existsSync(downloads) ? readdirSync(downloads) : [];
    for (const ext of [".png", ".svg", ".pdf"]) {
      expect(files.some((f) => f.endsWith(ext))).toBe(true);
    }
  });

  it.skip(`posts the board as a picture that a standard client receives (${BLOCKED})`, async () => {
    carol = new ScriptBot({ name: "Carol", timeout: 60 });
    await carol.connected();
    const picture = carol.run("expect message --from Alice --contains \"<img\"", 60_000);
    await inBoard(() => action("Post board to chat").click());
    await pluginDialog("Post to chat").waitForDisplayed({ timeout: 15_000 });
    await dialogButton("Post to chat").click();
    await expect($("//div[@role='dialog']")).toHaveText(
      expect.stringContaining("Posted to chat as a picture."),
      { wait: 30_000 },
    );
    await browser.keys("Escape");
    await expect($("//li[contains(@class,'chat-message')]//img")).toBeDisplayed({ wait: 15_000 });
    await picture;
  });
});
