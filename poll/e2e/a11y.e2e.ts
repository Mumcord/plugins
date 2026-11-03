// AC-6.29 (spec 06, C14) for Poll: axe (WCAG 2.2 AA) on its install prompt, strip icon and card
// with "Show original text" in the light and the dark theme; a keyboard-only walkthrough; a hidden
// "!poll" vote line is not read out (F46); clipped cards offer Show more at a 150 % font size (F44);
// the German text of the chat.triggers permission. The bot "Carol" stands in for a plain Mumble user.
// Runs inside a Mumcord checkout (e2e/run.sh copies it to e2e/specs/official-<plugin>/).
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { $, browser, expect } from "@wdio/globals";
import { registry, repoRoot, testenv } from "../../helpers/index.ts";
import { composer, ScriptBot } from "../../helpers/chat-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import { chooseLanguage, focus, pressUntil, tabTo, typeText } from "../app/flows.ts";
import {
  announcements,
  cardItem,
  CHANNEL,
  check,
  checkCardsAtLargeFont,
  closePanel,
  closeSettings,
  expectNoFindings,
  recordAnnouncements,
  restartIn,
  setUp,
  sheet,
  waitText,
} from "../official-shared/a11y.ts";

const QUESTION = "Pizza or pasta?";

/** Creates a poll with the keyboard only: strip button, New poll, fields, Create poll. */
async function createPollByKeyboard(): Promise<void> {
  await closePanel("Poll");
  await $(".pstrip-icons button[aria-label='Poll']").waitForExist({ timeout: 30_000 });
  // Start from the message field, then walk the focus order to the strip.
  await composer().click();
  // The strip is a toolbar: one Tab stop, arrow keys move between its icons.
  await tabTo((f) => f.cls.includes("pstrip-icon"), "the Plugins strip", true);
  await pressUntil("ArrowRight", (f) => f.label === "Poll", "Poll in the Plugins strip");
  await browser.keys("Enter");
  await $(".pstrip-panel").waitForDisplayed({ timeout: 10_000 });
  await tabTo((f) => f.text === "New poll", "New poll");
  await browser.keys("Enter");
  await sheet().waitForDisplayed({ timeout: 10_000 });
  await tabTo((f) => f.tag === "input" && f.value === "", "the question field");
  await typeText(QUESTION);
  for (const option of ["Pizza", "Pasta"]) {
    await tabTo((f) => f.tag === "input" && f.value === "", `field for ${option}`);
    await typeText(option);
  }
  await tabTo((f) => f.text === "Create poll", "Create poll");
  await browser.keys("Enter");
  await waitText(() => $("body"), "Your poll · 0 votes");
  if (await sheet().isExisting()) await browser.keys("Escape");
}

describe("Poll: accessibility (AC-6.29)", function () {
  this.bail(true);
  let fixtures: registry.FixtureRegistry;
  let carol: ScriptBot | undefined;

  before(async function () {
    this.timeout(20 * 60_000);
    fixtures = await registry.start();
    await setUp(fixtures, "Poll");
  });

  after(async () => {
    carol?.stop();
    await fixtures.stop();
  });

  it("keyboard only: opens Poll from the strip, creates a poll and toggles the original text", async () => {
    await recordAnnouncements();
    await createPollByKeyboard();
    const item = cardItem("Poll");
    await item.waitForDisplayed({ timeout: 20_000 });
    // The card's buttons are in the Tab order after the message field.
    await composer().click();
    await tabTo((f) => f.text === "Show original text", "Show original text", true);
    await browser.keys("Enter");
    await expect(item).toHaveText(expect.stringContaining("[Poll]"), { wait: 5_000 });
    expect((await focus()).text).toBe("Hide original text");
    await browser.keys("Enter");
    await expect(item).not.toHaveText(expect.stringContaining("[Poll]"), { wait: 5_000 });
  });

  it("light: strip and Poll card with the original text; a hidden vote line is not read out", async () => {
    carol = new ScriptBot({ name: "Carol", port: testenv.ports.v15, timeout: 120 });
    await carol.connected();
    await carol.run(`join ${CHANNEL}`);
    await check("light-strip", [".pstrip"]);
    await check("light-poll-card", ["section[aria-label='Card from Poll']"]);
    await cardItem("Poll").$("button=Show original text").click();
    await check("light-poll-original", ["section[aria-label='Card from Poll']"]);
    await cardItem("Poll").$("button=Hide original text").click();

    // F46: a hidden vote line is not read out; Carol's next line is.
    await carol.run(`say --channel ${CHANNEL} "!poll 1"`);
    await carol.run(`say --channel ${CHANNEL} "Count me in"`);
    await browser.waitUntil(async () => (await announcements()).includes("Count me in"), {
      timeout: 10_000,
      timeoutMsg: `Carol's line was not announced; said:\n${await announcements()}`,
    });
    const said = await announcements();
    expect(said).not.toContain("!poll 1");
    expect(said).toContain("Carol");
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
    await createPollByKeyboard();
    await cardItem("Poll").waitForDisplayed({ timeout: 20_000 });
    await check("dark-strip", [".pstrip"]);
    await check("dark-poll-card", ["section[aria-label='Card from Poll']"]);
    await cardItem("Poll").$("button=Show original text").click();
    await check("dark-poll-original", ["section[aria-label='Card from Poll']"]);
    await cardItem("Poll").$("button=Hide original text").click();
    expectNoFindings();
  });

  it("German: the chat.triggers permission text is translated", async () => {
    const de = JSON.parse(
      readFileSync(join(repoRoot, "ui", "src", "locales", "de", "pluginPermissions.json"), "utf8"),
    ) as { texts: { chatTriggers: string } };
    const installed = JSON.parse(
      readFileSync(join(repoRoot, "ui", "src", "locales", "de", "pluginInstalled.json"), "utf8"),
    ) as { details: { permissions: string } };
    await openSettingsGroup("General");
    await chooseLanguage("de", "German");
    await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.lang)) === "de", {
      timeoutMsg: "the app did not switch to German",
    });
    await $("[role='tab'][id$='-tab-plugins']").click();
    const row = $(`//li[.//span[contains(@class,'plugins-installed-name')][normalize-space(.)='Poll']]`);
    await row.$(`.//button[normalize-space(.)='${installed.details.permissions}']`).click();
    await expect(row).toHaveText(expect.stringContaining(de.texts.chatTriggers), { wait: 10_000 });
    await check("de-settings-installed", [".overlay__dialog"]);
    await closeSettings();
    await check("de-strip", [".pstrip"]);
    expectNoFindings();
  });
});
