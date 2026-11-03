// AC-6.29 (spec 06, C14) for Dice roller: axe (WCAG 2.2 AA) on its install prompt and progress bar
// in the dark theme (the light theme is covered by the other plugins' specs and Mumcord's own).
// Runs inside a Mumcord checkout (e2e/run.sh copies it to e2e/specs/official-<plugin>/).
import { app, registry, testenv } from "../../helpers/index.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import { completeWizard } from "../../helpers/voice-ui.ts";
import {
  closeSettings,
  developerMode,
  expectNoFindings,
  install,
  themeEnv,
} from "../official-shared/a11y.ts";

describe("Dice roller: accessibility (AC-6.29)", function () {
  this.bail(true);
  let fixtures: registry.FixtureRegistry;

  before(async function () {
    this.timeout(20 * 60_000);
    await testenv.up("v15");
    fixtures = await registry.start();
    await app.restart({ env: themeEnv(fixtures, "dark") });
    await completeWizard("Alice");
  });

  after(async () => {
    await fixtures.stop();
  });

  it("dark: the install prompt and its progress bar have no violations", async () => {
    await developerMode();
    await openSettingsGroup("Plugins");
    await install("Dice roller", "dark-dice");
    await closeSettings();
    expectNoFindings();
  });
});
