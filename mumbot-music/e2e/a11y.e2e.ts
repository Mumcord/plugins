// AC-6.29 (spec 06, C14) for Mumbot Music: axe (WCAG 2.2 AA) on its install prompt and its bot
// dialog (setup and player) in the light and the dark theme, left with the keyboard. "Tunes" is a
// local mumbot music module.
// Runs inside a Mumcord checkout (e2e/run.sh copies it to e2e/specs/official-<plugin>/).
import { type ChildProcess, spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { $, browser, expect } from "@wdio/globals";
import { bin, registry, testenv } from "../../helpers/index.ts";
import { runOk } from "../../helpers/run.ts";
import { certFingerprint } from "../../helpers/servers-ui.ts";
import { personRow } from "../../helpers/voice-ui.ts";
import { describeFocus, focus } from "../app/flows.ts";
import {
  CHANNEL,
  check,
  dialog,
  dialogField,
  expectNoFindings,
  restartIn,
  setField,
  setUp,
} from "../official-shared/a11y.ts";

/** A fresh name per run: the server keeps registrations between runs. */
const MUSIC_BOT = `Tunes${String(Date.now() % 100_000)}`;

async function openMusicDialog(): Promise<void> {
  const row = personRow(MUSIC_BOT);
  await row.waitForDisplayed({ timeout: 30_000 });
  // A contextmenu event like the menu key sends: WebKitWebDriver's right click also sends a click,
  // which pins the profile preview over the dialog a moment later.
  await browser.execute((el: HTMLElement) => {
    const box = el.getBoundingClientRect();
    el.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: box.left + 10, clientY: box.top + 5 }),
    );
  }, (await row.getElement()) as unknown as HTMLElement);
  const item = $("//*[@role='menuitem'][normalize-space()='Configure bot']");
  await item.waitForClickable({ timeout: 20_000 });
  await item.click();
  await dialog().waitForDisplayed({ timeout: 10_000 });
}

async function closeDialog(): Promise<void> {
  // The keyboard way out: Escape from wherever the focus is in the dialog.
  const where = describeFocus(await focus());
  expect(where).toContain("userpanel-modal__dialog");
  await browser.keys("Escape");
  await dialog().waitForExist({ reverse: true, timeout: 10_000 });
}

/** A local mumbot that runs only the music module (its HTTP API on 127.0.0.1). */
class MusicBot {
  private child: ChildProcess | undefined;
  private log = "";
  readonly key = randomBytes(16).toString("hex");
  readonly port = testenv.ports.v15 + 61;

  constructor(private readonly dir: string) {}

  private env(): NodeJS.ProcessEnv {
    return { ...process.env, MUMBOT_DATA: this.dir };
  }

  async start(): Promise<void> {
    const args = ["--name", MUSIC_BOT, "--port", String(testenv.ports.v15), "register"];
    for (let attempt = 1; ; attempt++) {
      try {
        await runOk(bin.bot, args, this.env());
        break;
      } catch (error) {
        if (attempt === 5) throw error;
        await new Promise((r) => setTimeout(r, 2_000));
      }
    }
    const media = join(this.dir, "media");
    mkdirSync(media, { recursive: true });
    const config = join(this.dir, "bot.toml");
    writeFileSync(
      config,
      `[server]\nhost = "127.0.0.1"\nport = ${String(testenv.ports.v15)}\nname = "${MUSIC_BOT}"\n` +
        `channel = "${CHANNEL}"\ndata_dir = "${this.dir}"\n` +
        `server_cert_sha256 = "${await certFingerprint("A")}"\n\n` +
        `[music]\nlisten = "127.0.0.1:${String(this.port)}"\nchannel = "${CHANNEL}"\nmedia_dir = "${media}"\n\n` +
        `[[music.keys]]\nkey = "${this.key}"\nlabel = "c14"\nchannels = ["${CHANNEL}"]\n`,
    );
    const child = spawn(bin.bot, ["music", "--config", config], { env: this.env() });
    child.stdout.on("data", (d: Buffer) => (this.log += d.toString()));
    child.stderr.on("data", (d: Buffer) => (this.log += d.toString()));
    this.child = child;
    const base = `http://127.0.0.1:${String(this.port)}`;
    await browser.waitUntil(
      async () =>
        (await fetch(`${base}/health`)
          .then((r) => r.json() as Promise<{ connected?: boolean }>)
          .catch(() => null))?.connected === true,
      { timeout: 60_000, timeoutMsg: `the music bot did not connect:\n${this.log}` },
    );
  }

  stop(): void {
    this.child?.kill("SIGTERM");
    this.child = undefined;
  }
}

describe("Mumbot Music: accessibility (AC-6.29)", function () {
  this.bail(true);
  let fixtures: registry.FixtureRegistry;
  let music: MusicBot;
  let dir: string;

  before(async function () {
    this.timeout(20 * 60_000);
    fixtures = await registry.start();
    await testenv.up("v15");
    dir = mkdtempSync(join(tmpdir(), "mumcord-c14-"));
    music = new MusicBot(dir);
    await music.start();
    await setUp(fixtures, "Mumbot Music");
  });

  after(async () => {
    music.stop();
    await fixtures.stop();
    rmSync(dir, { recursive: true, force: true });
  });

  it("light: the setup and the player of the bot dialog have no violations", async () => {
    await openMusicDialog();
    await check("light-mumbot-setup", [".userpanel-modal__dialog"]);
    await setField(dialogField("Bot address (IP or domain, with port)"), `127.0.0.1:${String(music.port)}`);
    await setField(dialogField("API key"), music.key);
    await dialog()
      .$(".//label[normalize-space(.)='API key']/following::button[normalize-space(.)='Save'][1]")
      .click();
    await browser.waitUntil(async () => (await dialogField("API key").getValue()) === "", {
      timeoutMsg: "the API key was not stored",
    });
    await dialog().$(".//button[normalize-space(.)='Test connection']").click();
    await expect(dialog()).toHaveText(expect.stringContaining("Connection works"), { wait: 20_000 });
    const save = dialog().$(".//button[normalize-space(.)='Save'][not(ancestor::div[contains(@class,'pr-row')])]");
    await save.click();
    await expect(dialog()).toHaveText(expect.stringContaining("Nothing is playing"), { wait: 20_000 });
    await check("light-mumbot-player", [".userpanel-modal__dialog"]);
    await closeDialog();
    expectNoFindings();
  });

  it("dark: the player has no violations", async () => {
    await restartIn(fixtures, "dark");
    await openMusicDialog();
    await expect(dialog()).toHaveText(expect.stringContaining("Nothing is playing"), { wait: 20_000 });
    await check("dark-mumbot-player", [".userpanel-modal__dialog"]);
    await closeDialog();
    expectNoFindings();
  });
});
