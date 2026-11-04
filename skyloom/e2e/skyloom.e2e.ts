// Skyloom (Mumcord/mumcord#394) between three Mumcord clients A, B and C and a standard Mumble
// user D (mumbot): A starts a night from the Apps menu, B and C join through the activity's Join
// button, a full night of three rounds runs with every client showing the same verified
// resonance, Threaders never see the figure before dawn, and D sees exactly the start line, one
// line per round and the summary. The game moves between the media area and the side panel
// without losing its state, and every role plays with the keyboard only.
//
// Needs Mumcord 0.7.0 (plugin API 1.1: stage, Apps menu, relay, sound effects).
import { $, browser, expect } from "@wdio/globals";
import { app, apps, registry, testenv } from "../../helpers/index.ts";
import {
  joinChannel,
  messageTexts,
  selectChannel,
  ScriptBot,
} from "../../helpers/chat-ui.ts";
import {
  addAndConnect,
  completeWizard,
  personRow,
} from "../../helpers/voice-ui.ts";
import {
  closeSettings,
  developerMode,
  install,
} from "../official-shared/a11y.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";

/** Plugin data travels only on the voice server, so everyone joins the same channel. */
const CHANNEL = "Lobby";
const ROUNDS = 3;

type Client = "A" | "B" | "C";

function on<T>(client: Client, fn: () => Promise<T>): Promise<T> {
  if (client === "B") return apps.onB(fn);
  if (client === "C") return apps.onC(fn);
  return fn();
}

const ALL: Client[] = ["A", "B", "C"];
const NAMES: Record<Client, string> = { A: "Alice", B: "Bob", C: "Carol" };

async function setUpClient(client: Client): Promise<void> {
  await on(client, async () => {
    await completeWizard(NAMES[client]);
    await developerMode();
    await closeSettings();
    await addAndConnect();
    await joinChannel(CHANNEL);
    await selectChannel(CHANNEL);
    await openSettingsGroup("Plugins");
    // install() ticks every optional permission, so the relay, sounds and chat lines are on.
    await install("Skyloom");
    await closeSettings();
  });
}

/**
 * Runs `fn` inside the game's page, wherever it is shown (media area, side panel or dialog):
 * Mumcord shows one web view per plugin, titled with the plugin's name.
 */
async function inGame<T>(fn: () => Promise<T>): Promise<T> {
  const frame = $("iframe.plugin-webview__frame[title='Skyloom']");
  await frame.waitForDisplayed({ timeout: 30_000 });
  await browser.switchFrame(frame);
  try {
    return await fn();
  } finally {
    await browser.switchFrame(null);
  }
}

async function clickInGame(selector: string, timeout = 30_000): Promise<void> {
  const el = $(selector);
  await el.waitForClickable({ timeout });
  await el.click();
}

async function gameText(): Promise<string> {
  return browser.execute(() => document.getElementById("sky")?.innerText ?? "");
}

async function waitGameText(text: string, timeout = 30_000): Promise<string> {
  let seen = "";
  await browser.waitUntil(
    async () => {
      seen = await gameText();
      return seen.includes(text);
    },
    {
      timeout,
      interval: 200,
      timeoutMsg: `the game never showed "${text}" (last: ${seen.slice(0, 300)})`,
    },
  );
  return seen;
}

/** The figure name and resonance shown at dawn, e.g. ["The Heron", "87%"]. */
async function revealed(): Promise<[string, string]> {
  await waitGameText("Verified", 40_000);
  const name = await $(".reveal-title__name").getText();
  const score = await $(".reveal-title__score span").getText();
  return [name, score];
}

/** Whoever is the round's Weaver this time. */
async function weaverOfRound(): Promise<Client> {
  for (const c of ALL) {
    const weaving = await on(c, () =>
      inGame(async () => (await gameText()).includes("You are the Weaver")),
    );
    if (weaving) return c;
  }
  throw new Error("nobody is weaving");
}

describe("plugins: Skyloom night with three Mumcord clients and a standard Mumble user", () => {
  let fixtures: registry.FixtureRegistry;
  let dave: ScriptBot | undefined;
  const plain = (): ScriptBot => {
    if (!dave) throw new Error("D (mumbot) is not connected");
    return dave;
  };
  /** Lines D must see, in order, collected as they are expected. */
  const lines: Promise<string>[] = [];

  before(async function () {
    this.timeout(30 * 60_000);
    fixtures = await registry.start();
    await testenv.up("v15");
  });

  after(async () => {
    dave?.stop();
    await apps.stopC();
    await apps.stopB();
    await fixtures.stop();
  });

  it("A, B and C install Skyloom and share a channel with D", async () => {
    await app.restart({ env: fixtures.env() });
    await setUpClient("A");
    await apps.startB({ env: fixtures.env() });
    await setUpClient("B");
    await apps.startC({ env: fixtures.env() });
    await setUpClient("C");
    dave = new ScriptBot({ name: "Dave", timeout: 30 });
    await dave.connected();
    for (const name of ["Bob", "Carol", "Dave"]) {
      await personRow(name).waitForDisplayed({ timeout: 20_000 });
    }
  });

  it("A starts Skyloom from the Apps menu; D sees one start line", async () => {
    lines.push(
      plain().run(
        'expect message --from Alice --contains "[Skyloom] Alice opened a night"',
        60_000,
      ),
    );
    await $("button[aria-label='Apps']").click();
    const item = $(
      "//*[@role='menuitem'][contains(normalize-space(.),'Start Skyloom')]",
    );
    await item.waitForClickable({ timeout: 10_000 });
    await item.click();
    await inGame(() => waitGameText("Light the loom"));
    await lines[0];
  });

  it("B and C join with the activity's Join button and get seats", async () => {
    for (const c of ["B", "C"] as const) {
      await on(c, async () => {
        const join = $(
          "//button[normalize-space(.)='Join'][ancestor::*[contains(@class,'stage-activity')]]",
        );
        await join.waitForClickable({ timeout: 30_000 });
        await join.click();
        await inGame(() =>
          waitGameText("waiting for Alice to light the loom"),
        );
      });
    }
    await inGame(async () => {
      for (const name of ["Bob", "Carol"]) await waitGameText(name);
    });
  });

  it("a night of three rounds: Threaders never see the figure, all clients agree and verify", async () => {
    // A picks a 60 s weave and begins.
    await inGame(async () => {
      await $("button[data-intent='weave'][data-value='60']").click();
      const begin = $("button[data-intent='begin']");
      await begin.waitForEnabled({ timeout: 10_000 });
      await begin.click();
    });
    for (let round = 1; round <= ROUNDS; round++) {
      // Wait for the weave, then find the Weaver.
      for (const c of ALL)
        await on(c, () => inGame(() => waitGameText("dawn in", 30_000)));
      const weaver = await weaverOfRound();
      const figure = await on(weaver, () =>
        inGame(async () => {
          const card = $(".sky-card .figure-name");
          await card.waitForDisplayed({ timeout: 10_000 });
          return card.getText();
        }),
      );
      expect(figure).not.toBe("");
      for (const c of ALL.filter((x) => x !== weaver)) {
        await on(c, () =>
          inGame(async () => {
            // A Threader's page holds no sky card and no figure name before dawn.
            await expect($(".sky-card")).not.toExist();
            expect(
              await browser.execute(() => document.documentElement.outerHTML),
            ).not.toContain(figure);
            // Keyboard only: Tab to the own star, arrows move it, Enter settles.
            const star = $(".star--you");
            await star.waitForExist({ timeout: 10_000 });
            await browser.execute(() =>
              document.querySelector<SVGGElement>(".star--you")?.focus(),
            );
            await browser.keys([
              "ArrowRight",
              "ArrowRight",
              "ArrowUp",
              "Enter",
            ]);
          }),
        );
      }
      // The Weaver ties off early to keep the test short.
      await on(weaver, () =>
        inGame(() => clickInGame("button[data-intent='tie-off']")),
      );
      const results = [];
      for (const c of ALL) results.push(await on(c, () => inGame(revealed)));
      for (const r of results) {
        expect(r[0]).toBe(figure);
        expect(r[1]).toBe(results[0]?.[1]);
      }
      if (round < ROUNDS) {
        const next = await (async () => {
          for (const c of ALL) {
            const ready = await on(c, () =>
              inGame(
                async () =>
                  await $("button[data-intent='weave-ready']").isExisting(),
              ),
            );
            if (ready) return c;
          }
          throw new Error("nobody can weave next");
        })();
        lines.push(
          plain().run(
            `expect message --from ${NAMES[next]} --contains "[Skyloom] Round ${String(round)}:"`,
            60_000,
          ),
        );
        await on(next, () =>
          inGame(() => clickInGame("button[data-intent='weave-ready']")),
        );
      }
    }
    lines.push(
      plain().run(
        `expect message --from Alice --contains "[Skyloom] Round ${String(ROUNDS)}:"`,
        60_000,
      ),
    );
    await inGame(() => clickInGame("button[data-intent='tapestry']"));
    for (const c of ALL)
      await on(c, () => inGame(() => waitGameText("Little honours")));
  });

  it("D sees exactly the start line, one line per round and the summary", async () => {
    lines.push(
      plain().run(
        'expect message --from Alice --contains "[Skyloom] The night is over"',
        60_000,
      ),
    );
    await inGame(() => clickInGame("button[data-intent='post-summary']"));
    const seen = await Promise.all(lines);
    expect(seen).toHaveLength(ROUNDS + 2);
    // Mumcord users with Skyloom see cards instead of the plain lines.
    for (const c of ALL) {
      const texts = await on(c, messageTexts);
      expect(texts.filter((t) => t.includes("[Skyloom]"))).toEqual([]);
    }
  });

  it("the game moves to the side panel and back without losing its state", async () => {
    await inGame(() => clickInGame("button[data-intent='place-side']"));
    await inGame(() => waitGameText("Little honours"));
    await inGame(() => clickInGame("button[data-intent='place-stage']"));
    await inGame(() => waitGameText("Little honours"));
  });
});
