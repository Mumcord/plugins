// AC-6.29 (spec 06, C14) for Echolings: axe (WCAG 2.2 AA) on its install prompt, the pet picker
// and the hatched pet in the light and the dark theme; reduced motion (GTK animations off) freezes
// the pet and its badge.
// Runs inside a Mumcord checkout (e2e/run.sh copies it to e2e/specs/official-<plugin>/).
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { $, expect } from "@wdio/globals";
import { app, registry } from "../../helpers/index.ts";
import { personRow } from "../../helpers/voice-ui.ts";
import {
  check,
  expectNoFindings,
  expectNoPluginMotion,
  openPanel,
  restartIn,
  setField,
  setUp,
  sheet,
  waitText,
} from "../official-shared/a11y.ts";

describe("Echolings: accessibility (AC-6.29)", function () {
  this.bail(true);
  let fixtures: registry.FixtureRegistry;

  before(async function () {
    this.timeout(20 * 60_000);
    fixtures = await registry.start();
    // Echolings keeps its pet per identity, which plugins only get once a server is focused;
    // setUp connects before it installs.
    await setUp(fixtures, "Echolings");
  });

  after(async () => {
    await fixtures.stop();
  });

  it("light: the pet picker and the hatched pet have no violations", async () => {
    await openPanel("Echolings");
    await $(".pstrip-panel .pstrip-expand").click();
    await sheet().waitForDisplayed({ timeout: 10_000 });
    await waitText(sheet, "Choose your Echoling");
    await check("light-echolings-picker", [".pstrip-sheet"]);
    await setField(sheet().$("//div[contains(@class,'pr-field')][label[normalize-space(.)='Name']]//input"), "Pip");
    await sheet().$("button=Hatch").click();
    await waitText(sheet, "Pip");
    await sheet().$("button=Pat").click();
    await check("light-echolings", [".pstrip-sheet"]);
    await sheet().$("button.pstrip-expand").click();
    // Control for the reduced-motion check: with motion on, the own pet badge animates.
    await expect(personRow("Alice").$(".plugin-badge .pr-anim")).toExist({ wait: 10_000 });
    expectNoFindings();
  });

  it("dark: the pet has no violations", async () => {
    await restartIn(fixtures, "dark");
    await openPanel("Echolings");
    await $(".pstrip-panel .pstrip-expand").click();
    await waitText(sheet, "Pip");
    await check("dark-echolings", [".pstrip-sheet"]);
    await sheet().$("button.pstrip-expand").click();
    expectNoFindings();
  });

  it("reduced motion (GTK animations off) freezes Echolings", async () => {
    const gtk = join(app.profile(), "config", "gtk-3.0");
    mkdirSync(gtk, { recursive: true });
    writeFileSync(join(gtk, "settings.ini"), "[Settings]\ngtk-enable-animations=false\n");
    await restartIn(fixtures, "light");
    await openPanel("Echolings");
    await $(".pstrip-panel .pstrip-expand").click();
    await waitText(sheet, "Pip");
    await sheet().$("button=Pat").click();
    await expectNoPluginMotion();
    await sheet().$("button.pstrip-expand").click();
  });
});
