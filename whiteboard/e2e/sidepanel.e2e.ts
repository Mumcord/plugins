// Whiteboard, issue #397: the side panel mini view. With the board running, B shares the screen and
// A watches it as the main view of the media area, so the board moves out of sight and the mini
// view opens in the side panel: it shows the live, read-only preview, a quick sticky note lands on
// the board, switching chats stops nothing and the board's stage tab brings it back.
//
// Not automated: the mini view's "Open on stage". A plugin cannot make its activity the main item
// of the media area while another one is (no such host call in Mumcord yet), so the button has no
// visible effect there.
import { $, browser, expect } from "@wdio/globals";
import { app, apps, registry, relayenv } from "../../helpers/index.ts";
import { joinChannel, selectChannel, startScreenShare } from "../../helpers/chat-ui.ts";
import { registerOnServer } from "../../helpers/files-ui.ts";
import { addAndConnect, completeWizard } from "../../helpers/voice-ui.ts";
import {
  inBoard,
  outline,
  panelFrame,
  PLUGIN,
  ROOT,
  setUpClient,
  stageFrame,
  startFromAppsMenu,
  startRelay,
  stroke,
  waitCount,
} from "./board.ts";

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
    await apps.stopB();
    await fixtures.stop();
  });

  it("A starts a board and draws", async () => {
    await app.restart({ env: fixtures.env() });
    await setUpClient("Alice", port);
    await startFromAppsMenu();
    await inBoard(async () => {
      await stroke(-50, 0);
      await waitCount(1);
    });
  });

  it("shows the live preview and adds a quick sticky note while a screen share is on stage", async () => {
    // B only shares the screen; the test backend has no screen picker, so the hook picks it.
    await apps.startB({ env: relayenv.hookEnv({ sharePick: "screen" }) });
    await apps.onB(async () => {
      await completeWizard("Bob");
      await addAndConnect(`localhost:${String(port)}`);
      await joinChannel(ROOT);
      // The relay bot gives relay tokens to registered users only.
      await registerOnServer();
      await startScreenShare();
    });
    const watch = $(
      "//div[contains(@class,'activity-banner')][contains(.,'Bob is sharing their screen')]//button[normalize-space()='Watch']",
    );
    await watch.waitForClickable({ timeout: 30_000 });
    await watch.click();
    // A new item only adds its tab to the media area; selecting it makes the share the main view.
    const shareTab = $(
      `//button[contains(@class,'media-stage__tab')][.//span[contains(@class,'media-stage__label')][normalize-space(.)!='${PLUGIN}']]`,
    );
    await shareTab.waitForClickable({ timeout: 15_000 });
    await shareTab.click();
    await stageFrame().waitForDisplayed({ reverse: true, timeout: 15_000 });
    // The board is out of sight, so the plugin opens its mini view in the side panel.
    await panelFrame().waitForDisplayed({ timeout: 15_000 });
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

  it("keeps the board when switching chats and shows it again from its stage tab", async () => {
    // The relay environment's server has only its root channel, so the other chat is Messages.
    await $("button[data-testid='pm-button']").click();
    await $("button[aria-label^='localhost (connected)']").click();
    await selectChannel(ROOT);
    await inBoard(() => waitCount(2));
    await inBoard(() => expect($("#live-text")).toHaveText("Live · read-only preview"), panelFrame);
    await $(
      `//button[contains(@class,'media-stage__tab')][.//span[contains(@class,'media-stage__label')][normalize-space(.)='${PLUGIN}']]`,
    ).click();
    await stageFrame().waitForDisplayed({ timeout: 15_000 });
    expect(await inBoard(outline)).toHaveLength(2);
  });
});
