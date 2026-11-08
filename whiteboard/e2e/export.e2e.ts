// Whiteboard, issue #397: export and post to chat (A alone, plain Mumble user C as mumbot).
// - Export saves PNG, SVG, PDF and the board file through Mumcord's save window; names are off by
//   default, and the plugin never sees a path. Mumcord's test hook answers the save window with one
//   fixed path (MUMCORD_TEST_PLUGIN_SAVE), so each format is checked by its file's first bytes.
// - Post to chat sends the whole board as a PNG with one click; Mumcord shows it inline and a
//   standard client (mumbot) gets the image message.
// Both start from the board's own Export dialog (mockups 5, 5b); its Save and Post buttons are the
// user's gesture (FR-9.45).
import { existsSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { $, expect } from "@wdio/globals";
import { app, registry, relayenv } from "../../helpers/index.ts";
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
  startRelay,
  stroke,
} from "./board.ts";

const EXPORT_TITLE = 'Export or share "Untitled board"';
/** Where the save window "saves" (Mumcord's test hook instead of the system dialog). */
const SAVE_PATH = join(tmpdir(), `whiteboard-export-${String(process.pid)}`);
/** Each export format and the first bytes of its file. */
const FORMATS: [string, string][] = [
  ["PNG image", "\x89PNG"],
  ["SVG drawing", "<svg"],
  ["PDF document", "%PDF"],
  ["Board file", "MCWB"],
];

describe("Whiteboard: export and post to chat", () => {
  let fixtures: registry.FixtureRegistry;
  /** The Mumble server of the relay environment. */
  let port = 0;
  let carol: ScriptBot | undefined;

  before(async function () {
    this.timeout(20 * 60_000);
    fixtures = await registry.start();
    port = await startRelay();
  });

  after(async () => {
    carol?.stop();
    await fixtures.stop();
  });

  it("A draws a board", async () => {
    await app.restart({
      env: { ...fixtures.env(), ...relayenv.hookEnv({ pluginSavePath: SAVE_PATH }) },
    });
    await setUpClient("Alice", port);
    await startFromAppsMenu();
    await inBoard(async () => {
      await stroke(-100, -50);
      await noteAt(50, 0, "Export me");
    });
  });

  it("exports PNG, SVG, PDF and the board file without names", async () => {
    for (const [format, magic] of FORMATS) {
      rmSync(SAVE_PATH, { force: true });
      await inBoard(async () => {
        await action("Export").click();
        await boardDialog(EXPORT_TITLE).waitForDisplayed({ timeout: 15_000 });
        await boardOption("Save to a file").click();
        await boardOption(format).click();
        await expect(boardOption("Add names of who drew what")).not.toBeSelected();
        await boardButton("Save").click();
        await expect(boardNote()).toHaveText(expect.stringContaining("Saved"), { wait: 30_000 });
        await boardButton("Cancel").click();
      });
      expect(existsSync(SAVE_PATH)).toBe(true);
      expect(readFileSync(SAVE_PATH).subarray(0, magic.length).toString("latin1")).toBe(magic);
    }
    rmSync(SAVE_PATH, { force: true });
  });

  it("posts the board as a picture that a standard client receives", async () => {
    carol = new ScriptBot({ name: "Carol", port, timeout: 60 });
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
