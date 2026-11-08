// Whiteboard, issue #397: the side panel mini view. With the board running, A opens Whiteboard
// in the plugin strip: the mini view shows the live, read-only preview, a quick sticky note lands
// on the board, "Open on stage" brings the board back, and switching chats stops nothing.
// The issue's case is a screen share on the stage; here the panel is opened next to the running
// board instead.
import { $, browser, expect } from "@wdio/globals";
import { app, registry } from "../../helpers/index.ts";
import { selectChannel } from "../../helpers/chat-ui.ts";
import { openPanel } from "../official-shared/a11y.ts";
import {
  inBoard,
  outline,
  panelFrame,
  PLUGIN,
  ROOT,
  setUpClient,
  startFromAppsMenu,
  startRelay,
  stroke,
  waitCount,
} from "./board.ts";

const stage = () => $(`iframe.plugin-webview__frame[title='${PLUGIN}']`);

describe("Whiteboard: side panel mini view", () => {
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

  it("A starts a board and draws", async () => {
    await app.restart({ env: fixtures.env() });
    await setUpClient("Alice", port);
    await startFromAppsMenu();
    await inBoard(() => stroke(-50, 0));
  });

  it("shows the live preview and adds a quick sticky note", async () => {
    await openPanel(PLUGIN);
    await inBoard(async () => {
      await expect($("#live-text")).toHaveText("Live · read-only preview", { wait: 15_000 });
      const quick = $("#quick");
      await quick.click();
      await browser.keys("From the panel");
      await browser.keys("Enter");
    }, panelFrame);
    await inBoard(async () => {
      const lines = await waitCount(2);
      expect(lines.some((l) => l.includes("'From the panel'"))).toBe(true);
    });
  });

  it("keeps the board when switching chats and opens it on the stage again", async () => {
    await selectChannel("Lobby");
    await selectChannel(ROOT);
    await inBoard(() => waitCount(2));
    await inBoard(() => $("#open").click(), panelFrame);
    await stage().waitForDisplayed({ timeout: 15_000 });
    expect(await inBoard(outline)).toHaveLength(2);
  });
});
