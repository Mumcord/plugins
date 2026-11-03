// AC-6.29 (spec 06, C14) for EVE fit: axe (WCAG 2.2 AA) on its install prompt and its fit history
// panel in the light and the dark theme; a replaced fit is read out as its card, not as the pasted
// lines (F46); clipped cards offer Show more at a 150 % font size (F44). The bot "Carol" pastes the
// fit as a plain Mumble user.
// Runs inside a Mumcord checkout (e2e/run.sh copies it to e2e/specs/official-<plugin>/).
import { readdirSync } from "node:fs";
import { join } from "node:path";
import { $, browser, expect } from "@wdio/globals";
import { registry, testenv } from "../../helpers/index.ts";
import { ScriptBot } from "../../helpers/chat-ui.ts";
import {
  announcements,
  cardItem,
  CHANNEL,
  check,
  checkCardsAtLargeFont,
  closePanel,
  expectNoFindings,
  openPanel,
  recordAnnouncements,
  restartIn,
  setUp,
  waitText,
} from "../official-shared/a11y.ts";

const FIT = ["[Rifter, Tackle]", "Damage Control II", "Warp Scrambler II"];

/** EVE fit's panel with its fit history opened. */
async function openFitHistory(): Promise<void> {
  await openPanel("EVE fit");
  const toggle = $(".pstrip-panel").$("button*=Fit history");
  await toggle.waitForClickable({ timeout: 20_000 });
  if ((await toggle.getText()).includes("▸")) await toggle.click();
  await waitText(() => $(".pstrip-panel"), "Tackle");
}

describe("EVE fit: accessibility (AC-6.29)", function () {
  this.bail(true);
  let fixtures: registry.FixtureRegistry;
  let carol: ScriptBot | undefined;

  async function startCarol(): Promise<ScriptBot> {
    const bot = new ScriptBot({ name: "Carol", port: testenv.ports.v15, timeout: 120 });
    await bot.connected();
    await bot.run(`join ${CHANNEL}`);
    carol = bot;
    return bot;
  }

  before(async function () {
    this.timeout(20 * 60_000);
    fixtures = await registry.start();
    // EVE fit is large, so the registry script only stages it; publish it for this spec.
    const eveFit = readdirSync(join(fixtures.workDir, "staged")).find((f) => f.startsWith("eve-fit-"));
    if (eveFit === undefined) throw new Error("eve-fit was not built (e2e/run.sh)");
    await fixtures.publish(eveFit.replace(/\.mcplugin$/, ""));
    await setUp(fixtures, "EVE fit");
  });

  after(async () => {
    carol?.stop();
    await fixtures.stop();
  });

  it("light: a pasted fit is read out as its card; the fit history has no violations", async () => {
    await recordAnnouncements();
    const bot = await startCarol();
    await bot.run(`say --channel ${CHANNEL} "${FIT.join("<br>")}"`);
    await cardItem("EVE fit").waitForDisplayed({ timeout: 20_000 });
    await browser.waitUntil(async () => (await announcements()).includes("EVE fit"), {
      timeout: 10_000,
      timeoutMsg: `the fit card was not announced; said:\n${await announcements()}`,
    });
    const said = await announcements();
    expect(said).not.toContain(FIT[1]);
    expect(said).toContain("Carol");
    await check("light-fit-card", ["section[aria-label='Card from EVE fit']"]);
    await openFitHistory();
    await check("light-evefit-history", [".pstrip"]);
    await closePanel("EVE fit");
    expectNoFindings();
  });

  it("F44: at a 150 % font size every clipped card offers Show more", async () => {
    await checkCardsAtLargeFont("light-cards-150");
    expectNoFindings();
  });

  it("dark: the same screens have no violations", async () => {
    carol?.stop();
    carol = undefined;
    await restartIn(fixtures, "dark");
    const bot = await startCarol();
    await bot.run(`say --channel ${CHANNEL} "${FIT.join("<br>")}"`);
    await cardItem("EVE fit").waitForDisplayed({ timeout: 20_000 });
    await check("dark-fit-card", ["section[aria-label='Card from EVE fit']"]);
    await openFitHistory();
    await check("dark-evefit-history", [".pstrip"]);
    await closePanel("EVE fit");
    expectNoFindings();
  });
});
