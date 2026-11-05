// Whiteboard, issue #397: three Mumcord clients on one board over a server without a relay.
// - No relay: the board shows the "no relay" strip, and another person's cursor shows "x s ago".
// - Late join: C joins a ~110 KiB board (70 strokes × 400 points) and has all of it within 15 s,
//   drawing before the load finishes (the issue's 150 KiB in 15 s, as agreed: ≤ 110 KiB here).
// - A, B and C draw, place and type notes, move them and erase at the same time; every client ends
//   with the same board (the outline, as the page shows no state hash). Delayed, reordered,
//   duplicated and dropped messages are covered by the simulated network in src/sync.rs tests.
// - After everyone leaves, A restarts Mumcord and "Continue" re-opens the saved board.
import { $, browser, expect } from "@wdio/globals";
import { app, apps, registry, testenv } from "../../helpers/index.ts";
import { joinChannel, reconnectSaved } from "../../helpers/chat-ui.ts";
import {
  bulkStrokes,
  eraseAt,
  inBoard,
  joinFromCard,
  moveAt,
  noteAt,
  onA,
  outline,
  ROOT,
  setUpClient,
  startFromAppsMenu,
  stroke,
  waitSameBoard,
} from "./board.ts";

const BULK_STROKES = 70;
const BULK_POINTS = 400;
const LATE_JOIN_MS = 15_000;

describe("Whiteboard: three clients without a relay", () => {
  let fixtures: registry.FixtureRegistry;
  /** Objects every client should have at the end of the current step. */
  let objects = 0;
  let finalBoard: string[] = [];

  before(async function () {
    this.timeout(20 * 60_000);
    fixtures = await registry.start();
    await testenv.up("v15");
  });

  after(async () => {
    await apps.stopB();
    await apps.stopC();
    await fixtures.stop();
  });

  it("A, B and C install Whiteboard in the same channel", async () => {
    await app.restart({ env: fixtures.env() });
    await setUpClient("Alice");
    await apps.startB({ env: fixtures.env() });
    await apps.onB(() => setUpClient("Bob"));
    await apps.startC({ env: fixtures.env() });
    await apps.onC(() => setUpClient("Carol"));
  });

  it("shows the no-relay strip and other people's cursors as 'x s ago'", async () => {
    await startFromAppsMenu();
    await apps.onB(joinFromCard);
    for (const on of [onA, apps.onB]) {
      await on(() =>
        inBoard(async () => {
          await expect($("#strip")).toBeDisplayed({ wait: 15_000 });
          await expect($("#strip-title")).toHaveText("This server has no relay for plugin data.");
        }),
      );
    }
    await apps.onB(() => inBoard(() => stroke(-50, -150)));
    objects = 1;
    await waitSameBoard([onA, apps.onB], objects);
    // Without a relay the cursor travels with the stroke: A shows where B was and since when.
    await inBoard(() =>
      expect($("#cursors")).toHaveText(expect.stringMatching(/\d+ s ago/), { wait: 15_000 }),
    );
  });

  it(`C joins a ~110 KiB board late and loads it within ${String(LATE_JOIN_MS / 1000)} s`, async function () {
    this.timeout(10 * 60_000);
    await inBoard(() => bulkStrokes(BULK_STROKES, BULK_POINTS, 397));
    objects += BULK_STROKES;
    // Without a relay A sends the strokes to B at the server's message rate: that may take minutes.
    const board = await waitSameBoard([onA, apps.onB], objects, 5 * 60_000);

    await apps.onC(async () => {
      const clicked = Date.now();
      await joinFromCard();
      // Drawing works before the board has loaded.
      await inBoard(() => stroke(150, -150));
      await inBoard(async () => {
        let mine: string[] = [];
        await browser.waitUntil(
          async () => {
            mine = await outline();
            return board.every((line) => mine.includes(line));
          },
          {
            timeout: Math.max(LATE_JOIN_MS - (Date.now() - clicked), 1),
            interval: 250,
            timeoutMsg: `C had ${String(mine.length)} of ${String(board.length)} objects after 15 s`,
          },
        );
      });
    });
    objects += 1;
    await waitSameBoard([onA, apps.onB, apps.onC], objects);
  });

  it("A, B and C draw, move notes, type and erase at once and end with the same board", async () => {
    // Each person works in their own column of the view; the bulk strokes lie far above it.
    const columns: [typeof onA, number, string][] = [
      [onA, -250, "Alice's plan"],
      [apps.onB, -50, "Bob's idea"],
      [apps.onC, 150, "Carol's todo"],
    ];
    for (const [on, x, text] of columns) {
      await on(() =>
        inBoard(async () => {
          await stroke(x, -50);
          await noteAt(x, 50, text);
        }),
      );
    }
    for (const [on, x] of columns) {
      await on(() => inBoard(() => moveAt(x + 20, 70, 0, 60)));
    }
    // C erases its own new stroke; A and B keep theirs.
    await apps.onC(() => inBoard(() => eraseAt(150 + 40, -50)));
    objects += 2 * columns.length - 1;
    finalBoard = await waitSameBoard([onA, apps.onB, apps.onC], objects);
    for (const [, , text] of columns) {
      expect(finalBoard.some((line) => line.includes(`'${text}'`))).toBe(true);
    }
  });

  it("re-opens the saved board with Continue after everyone left and A restarted", async () => {
    await apps.stopB();
    await apps.stopC();
    await app.restart();
    await reconnectSaved();
    await joinChannel(ROOT);
    await startFromAppsMenu();
    await inBoard(async () => {
      const resume = $("//button[normalize-space(.)='Continue Untitled board']");
      await resume.waitForClickable({ timeout: 30_000 });
      await resume.click();
      let board: string[] = [];
      await browser.waitUntil(
        async () => {
          board = await outline();
          return board.length === finalBoard.length;
        },
        { timeout: 30_000, timeoutMsg: `the saved board has ${String(board.length)} objects` },
      );
      expect(board).toEqual(finalBoard);
    });
  });
});
