// Whiteboard, issue #397: the host's settings and presenting, between A (host) and B.
// - "Only me" and "Selected people" are enforced on B's client: B is view-only until A allows B.
// - Clearing needs a confirmation, removes everything for both, and Undo brings it back.
// - Hiding B's strokes affects only A.
// - Presenting: B sees that A presents and can follow and stop following; Esc ends presenting.
//   The followed view and the laser travel on the relay only (no relay in this environment):
//   their live movement is not checked here.
import { $, browser, expect } from "@wdio/globals";
import { app, apps, registry, testenv } from "../../helpers/index.ts";
import {
  action,
  boardButton,
  boardDialog,
  boardToast,
  dialogButton,
  dialogControl,
  inBoard,
  joinFromCard,
  onA,
  outline,
  pluginDialog,
  setUpClient,
  startFromAppsMenu,
  stroke,
  waitCount,
  waitSameBoard,
} from "./board.ts";

/** Opens "Board settings" from the board (A) and waits for Mumcord's dialog. */
async function openBoardSettings(): Promise<void> {
  await inBoard(() => action("Board settings").click());
  await pluginDialog("Board settings").waitForDisplayed({ timeout: 15_000 });
}

async function closeDialog(): Promise<void> {
  await dialogButton("Done").click();
  await $("//div[@role='dialog']").waitForExist({ reverse: true, timeout: 10_000 });
}

/** B tries to draw; returns once B's own page took the stroke or refused it. */
async function bDraws(x: number): Promise<void> {
  await apps.onB(() => inBoard(() => stroke(x, 100)));
}

describe("Whiteboard: host settings and presenting", () => {
  let fixtures: registry.FixtureRegistry;
  let objects = 0;

  before(async function () {
    this.timeout(20 * 60_000);
    fixtures = await registry.start();
    await testenv.up("v15");
  });

  after(async () => {
    await apps.stopB();
    await fixtures.stop();
  });

  it("A starts a board and B joins it", async () => {
    await app.restart({ env: fixtures.env() });
    await setUpClient("Alice");
    await apps.startB({ env: fixtures.env() });
    await apps.onB(() => setUpClient("Bob"));
    await startFromAppsMenu();
    await apps.onB(joinFromCard);
    await inBoard(() => stroke(-200, -100));
    await bDraws(0);
    objects = 2;
    await waitSameBoard([onA, apps.onB], objects);
  });

  it("'Only me' makes B view-only on B's own client", async () => {
    await openBoardSettings();
    await dialogControl("Only me").click();
    await closeDialog();
    await apps.onB(() =>
      inBoard(() =>
        expect($("#readonly")).toHaveText("View only: the host decides who may draw.", {
          wait: 15_000,
        }),
      ),
    );
    await bDraws(150);
    // Nothing new reaches A (and B's page drops it, too).
    await browser.pause(5_000);
    await waitSameBoard([onA, apps.onB], objects);
  });

  it("'Selected people' lets B draw once A selects B", async () => {
    await openBoardSettings();
    await dialogControl("Selected people").click();
    await dialogControl("Bob may draw").click();
    await closeDialog();
    await apps.onB(() =>
      inBoard(() => expect($("#readonly")).not.toBeDisplayed({ wait: 15_000 })),
    );
    await bDraws(150);
    objects += 1;
    await waitSameBoard([onA, apps.onB], objects);
  });

  it("hiding B's strokes affects only A", async () => {
    await openBoardSettings();
    await dialogControl("Hide Bob's strokes for me").click();
    await closeDialog();
    await inBoard(() => waitCount(1));
    await apps.onB(() => inBoard(() => waitCount(objects)));
    await openBoardSettings();
    await dialogControl("Hide Bob's strokes for me").click();
    await closeDialog();
    await inBoard(() => waitCount(objects));
  });

  it("clearing needs confirmation, clears for both and can be undone", async () => {
    const before = await inBoard(outline);
    // The confirmation is drawn inside the board (mockup 6b); clearing needs no gesture.
    await inBoard(async () => {
      await action("Clear board").click();
      await boardDialog("Clear the whole board?").waitForDisplayed({ timeout: 15_000 });
      await expect(boardDialog("Clear the whole board?")).toHaveText(
        expect.stringContaining("for all 2 people on the board"),
      );
      await boardButton("Keep the board").click();
    });
    await browser.pause(2_000);
    await waitSameBoard([onA, apps.onB], objects);

    await inBoard(async () => {
      await action("Clear board").click();
      await boardButton("Clear board").click();
      await expect(boardToast()).toHaveText(expect.stringContaining("The board is empty for everyone."), {
        wait: 15_000,
      });
    });
    await waitSameBoard([onA, apps.onB], 0);
    await inBoard(() => boardToast().$("button=Undo").click());
    const after = await waitSameBoard([onA, apps.onB], objects);
    expect(after).toEqual(before);
  });

  it("presenting: B follows and stops following, Esc ends it", async () => {
    await inBoard(async () => {
      await action("Present").click();
      await expect($("#presentbar")).toBeDisplayed({ wait: 10_000 });
    });
    await apps.onB(() =>
      inBoard(async () => {
        const banner = $("#banner");
        await expect(banner).toHaveText(expect.stringContaining("Alice is presenting"), {
          wait: 15_000,
        });
        await banner.$("button=Follow").click();
        await expect(banner).toHaveText(expect.stringContaining("Following Alice"), {
          wait: 10_000,
        });
        await banner.$("button=Stop following").click();
        await expect(banner).toHaveText(expect.stringContaining("Alice is presenting"), {
          wait: 10_000,
        });
        await banner.$("button=Follow").click();
      }),
    );
    await inBoard(async () => {
      await browser.execute(() => document.getElementById("wb")?.focus());
      await browser.keys("Escape");
      await expect($("#presentbar")).not.toBeDisplayed({ wait: 10_000 });
    });
    await apps.onB(() =>
      inBoard(() => expect($("#banner")).not.toBeDisplayed({ wait: 15_000 })),
    );
  });
});
