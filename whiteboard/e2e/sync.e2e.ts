// Whiteboard, issue #397: three Mumcord clients on one board over a relay that allows plugin data
// (the board travels only over the relay; start.e2e.ts covers a server without one).
// - With the relay the board shows no "needs a relay" notice, and B's stroke reaches A.
// - Late join: C joins a ~110 KiB board (70 strokes × 400 points) and has all of it within 15 s,
//   drawing before the load finishes (the issue's 150 KiB in 15 s, as agreed: ≤ 110 KiB here).
// - A, B and C draw, place and type notes, move them and erase at the same time; every client ends
//   with the same board (the outline, as the page shows no state hash). Delayed, reordered,
//   duplicated and dropped messages are covered by the simulated network in src/sync.rs tests.
// - After everyone leaves, A restarts Mumcord and "Continue" re-opens the saved board.
import { $, browser, expect } from "@wdio/globals";
import { app, apps, registry } from "../../helpers/index.ts";
import { joinChannel } from "../../helpers/chat-ui.ts";
import { reconnect } from "../../helpers/voice-ui.ts";
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
  startRelay,
  stroke,
  waitSameBoard,
} from "./board.ts";

const BULK_STROKES = 70;
const BULK_POINTS = 400;
const LATE_JOIN_MS = 15_000;

describe("Whiteboard: three clients over the relay", () => {
  let fixtures: registry.FixtureRegistry;
  /** The Mumble server of the relay environment. */
  let port = 0;
  /** Objects every client should have at the end of the current step. */
  let objects = 0;
  let finalBoard: string[] = [];

  before(async function () {
    this.timeout(20 * 60_000);
    fixtures = await registry.start();
    port = await startRelay();
  });

  after(async () => {
    await apps.stopB();
    await apps.stopC();
    await fixtures.stop();
  });

  it("A, B and C install Whiteboard in the same channel", async () => {
    await app.restart({ env: fixtures.env() });
    await setUpClient("Alice", port);
    await apps.startB({ env: fixtures.env() });
    await apps.onB(() => setUpClient("Bob", port));
    await apps.startC({ env: fixtures.env() });
    await apps.onC(() => setUpClient("Carol", port));
  });

  it("shows no relay notice and B's stroke reaches A", async () => {
    await startFromAppsMenu();
    await apps.onB(joinFromCard);
    for (const on of [onA, apps.onB]) {
      await on(() => inBoard(() => expect($("#strip")).not.toBeDisplayed()));
    }
    await apps.onB(() => inBoard(() => stroke(-50, -70)));
    objects = 1;
    await waitSameBoard([onA, apps.onB], objects);
  });

  it(`C joins a ~110 KiB board late and loads it within ${String(LATE_JOIN_MS / 1000)} s`, async function () {
    this.timeout(10 * 60_000);
    await inBoard(() => bulkStrokes(BULK_STROKES, BULK_POINTS, 397));
    objects += BULK_STROKES;
    const board = await waitSameBoard([onA, apps.onB], objects, 5 * 60_000);

    await apps.onC(async () => {
      const clicked = Date.now();
      await joinFromCard();
      // Drawing works before the board has loaded.
      await inBoard(() => stroke(100, 0));
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
      [onA, -210, "Alice's plan"],
      [apps.onB, -20, "Bob's idea"],
      [apps.onC, 170, "Carol's todo"],
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
    await apps.onC(() => inBoard(() => eraseAt(170 + 40, -50)));
    objects += 2 * columns.length - 1;
    finalBoard = await waitSameBoard([onA, apps.onB, apps.onC], objects);
    for (const [, , text] of columns) {
      expect(finalBoard.some((line) => line.includes(`'${text}'`))).toBe(true);
    }
  });

  it("re-opens the saved board with Continue after everyone left and A restarted", async () => {
    // A saves 2 s after the last change (SAVE_DEBOUNCE_MS); a restart before that loses it.
    await browser.pause(4_000);
    await apps.stopB();
    await apps.stopC();
    await app.restart();
    await reconnect();
    await joinChannel(ROOT);
    await startFromAppsMenu();
    await inBoard(async () => {
      const resume = $("//button[normalize-space(.)='Continue Untitled board']");
      await resume.waitForClickable({ timeout: 30_000 });
      await resume.click();
      let board: string[] = [];
      await browser
        .waitUntil(
          async () => {
            board = await outline();
            return board.length === finalBoard.length;
          },
          { timeout: 30_000 },
        )
        .catch((e: unknown) => {
          throw new Error(
            `the saved board has ${String(board.length)} of ${String(finalBoard.length)} objects (${String(e)})`,
          );
        });
      expect(board).toEqual(finalBoard);
    });
  });
});
