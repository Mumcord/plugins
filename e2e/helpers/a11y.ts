// Shared helpers of the official plugins' accessibility specs (AC-6.29, spec 06 C14): axe (WCAG
// 2.2 AA, colour contrast included) on a plugin's screens, its install prompt and its progress bar.
// e2e/run.sh copies this file to <mumcord>/e2e/specs/official-shared/a11y.ts, next to Mumcord's own
// helpers, so the imports below resolve there.
import { $, $$, browser, expect } from "@wdio/globals";
import type { Result } from "axe-core";
import { app, axe, registry, testenv } from "../../helpers/index.ts";
import { joinChannel, selectChannel } from "../../helpers/chat-ui.ts";
import { openSettingsGroup, settingsCard } from "../../helpers/settings-ui.ts";
import { addAndConnect, completeWizard, reconnect } from "../../helpers/voice-ui.ts";

type Element = ReturnType<typeof $>;

export const CHANNEL = "Lobby";

export const prompt = "//div[contains(concat(' ',@class,' '),' plugin-prompt ')]";
export const pluginTab = (name: string) =>
  $(`//button[@role='tab'][contains(@class,'plugins-tab')][starts-with(normalize-space(.),'${name}')]`);
export const sheet = () => $(".pstrip-sheet");
export const dialog = () => $("//div[@role='dialog' and contains(@class,'userpanel-modal__dialog')]");
export const dialogField = (label: string) =>
  dialog().$(`.//label[normalize-space(.)='${label}']/following::input[1]`);
export const cardItem = (plugin: string) =>
  $(`(//li[.//section[contains(@class,'plugin-chat-card')][@aria-label='Card from ${plugin}']])[last()]`);

/** Every violation of one screen, collected so a run reports all screens at once. */
export const findings: string[] = [];

export async function check(label: string, include: string[]): Promise<void> {
  const violations: Result[] = await axe.scan(label, {}, { include: include.map((s) => [s]) });
  for (const v of violations) {
    const targets = v.nodes.map((n) => n.target.join(" ")).slice(0, 3).join(" | ");
    findings.push(`${label}: ${v.impact ?? "?"} ${v.id} (${v.help}) → ${targets}`);
  }
}

export function expectNoFindings(): void {
  const all = findings.splice(0);
  expect(all.join("\n")).toBe("");
}

export async function waitText(el: () => Element, text: string, timeout = 20_000): Promise<void> {
  await browser.waitUntil(
    async () => (await el().getProperty("textContent").then(String)).includes(text),
    { timeout, timeoutMsg: `${JSON.stringify(text)} never showed` },
  );
}

/** Puts `value` into a React text field (typed keys lose Shift in some WebKitWebDriver runs). */
export async function setField(input: Element, value: string): Promise<void> {
  await input.waitForDisplayed({ timeout: 10_000 });
  await browser.execute(
    (el: HTMLInputElement, text: string) => {
      el.focus();
      Reflect.set(HTMLInputElement.prototype, "value", text, el);
      el.dispatchEvent(new Event("input", { bubbles: true }));
      // Some plugin fields commit on leaving the field.
      el.blur();
    },
    (await input.getElement()) as unknown as HTMLInputElement,
    value,
  );
}

export async function closeSettings(): Promise<void> {
  await browser.keys("Escape");
  await $(".settings__nav").waitForExist({ reverse: true, timeout: 10_000 });
}

export async function developerMode(): Promise<void> {
  await openSettingsGroup("Advanced & Developer");
  const developer = settingsCard("Developer mode").$("input[role='switch']");
  await developer.waitForExist({ timeout: 10_000 });
  if (!(await developer.isSelected())) await developer.click();
  await expect(developer).toBeSelected({ wait: 5_000 });
}

/**
 * Runs axe on the progress bar the moment it shows (it lasts well under a second for a local
 * registry, too short for a WebDriver round trip) and keeps the result in the page.
 */
export async function armProgressScan(): Promise<void> {
  await axe.inject();
  await browser.execute((tags: string[]) => {
    const w = window as unknown as Record<string, unknown>;
    w.__c14Progress = null;
    const observer = new MutationObserver(() => {
      if (document.querySelector(".plugin-progress") === null) return;
      observer.disconnect();
      const run = (w.axe as { run: (c: unknown, o: unknown) => Promise<{ violations: unknown[] }> }).run;
      run({ include: [[".plugin-progress"]] }, { runOnly: { type: "tag", values: tags } })
        .then((r) => (w.__c14Progress = { violations: r.violations }))
        .catch((e: unknown) => (w.__c14Progress = { error: String(e) }));
    });
    observer.observe(document.body, { childList: true, subtree: true });
  }, axe.wcag22aa);
}

export async function progressFindings(label: string): Promise<void> {
  const result = await browser.waitUntil(
    () => browser.execute(() => (window as unknown as Record<string, unknown>).__c14Progress),
    { timeout: 30_000, timeoutMsg: "the progress bar never showed" },
  );
  const r = result as { error?: string; violations?: { id: string; impact: string }[] };
  if (r.error !== undefined) findings.push(`${label}: axe failed on the progress bar: ${r.error}`);
  for (const v of r.violations ?? []) findings.push(`${label}: ${v.impact} ${v.id}`);
}

/** Installs `name` from Browse with every optional permission on; scans the prompt once. */
export async function install(name: string, scanAs?: string): Promise<void> {
  await pluginTab("Browse").click();
  const button = $(`button[aria-label='Install ${name}']`);
  await button.waitForExist({ timeout: 30_000 });
  await button.scrollIntoView({ block: "center" });
  await button.waitForClickable({ timeout: 30_000 });
  if (scanAs !== undefined) await armProgressScan();
  await button.click();
  const confirm = $(`${prompt}//button[normalize-space(.)='Install']`);
  await confirm.waitForClickable({ timeout: 60_000 });
  if (scanAs !== undefined) {
    await progressFindings(`${scanAs}-progress`);
    await check(`${scanAs}-prompt`, [".plugin-prompt"]);
  }
  for (const box of await $$(`${prompt}//label[contains(@class,'perm-label')]//input[@type='checkbox']`)) {
    if (!(await box.isSelected())) await box.click();
  }
  await confirm.click();
  await expect($(".plugin-toast")).toHaveText(expect.stringContaining(`${name} installed`), {
    wait: 60_000,
  });
}

/** Opens a plugin's inline panel in the strip (no-op when it is open). */
export async function openPanel(plugin: string): Promise<void> {
  const icon = $(`.pstrip-icons button[aria-label='${plugin}']`);
  await icon.waitForClickable({ timeout: 30_000 });
  // The strip remembers its open panel across restarts, so its state decides the clicks needed.
  for (let i = 0; i < 3; i++) {
    const open = (await icon.getAttribute("aria-pressed")) === "true";
    if (open && (await $(".pstrip-panel").isDisplayed())) return;
    await icon.click();
    await $(".pstrip-panel").waitForDisplayed({ timeout: 3_000 }).catch(() => undefined);
  }
  const state = await browser.execute(() => document.querySelector(".pstrip")?.outerHTML.replace(/<svg.*?<\/svg>|<img[^>]*>/g, "").slice(0, 1500));
  throw new Error(`the ${plugin} panel did not open: ${String(state)}`);
}

export async function closePanel(plugin: string): Promise<void> {
  const icon = $(`.pstrip-icons button[aria-label='${plugin}']`);
  if ((await icon.getAttribute("aria-pressed")) === "true") await icon.click();
}

/** Records every text the polite and assertive live regions announce. */
export async function recordAnnouncements(): Promise<void> {
  await browser.execute(() => {
    const w = window as unknown as { __c14Said: string[] };
    w.__c14Said = [];
    for (const id of ["live-polite", "live-assertive"]) {
      const region = document.querySelector(`[data-testid='${id}']`);
      if (region === null) continue;
      new MutationObserver(() => {
        const text = region.textContent;
        if (text !== "") w.__c14Said.push(text);
      }).observe(region, { childList: true, subtree: true, characterData: true });
    }
  });
}

export async function announcements(): Promise<string> {
  return browser.execute(() => (window as unknown as { __c14Said: string[] }).__c14Said.join("\n"));
}

/** The app's environment for `theme` against the run's fixture registry. */
export function themeEnv(fixtures: registry.FixtureRegistry, theme: "light" | "dark"): Record<string, string> {
  return { ...fixtures.env(), GTK_THEME: theme === "dark" ? "Adwaita:dark" : "Adwaita" };
}

/**
 * Starts the server, the app in the light theme and a connected Alice in the channel with
 * Developer mode on (the fixture registry is plain http on 127.0.0.1, FR-6.74), then installs
 * `plugin` from Browse with its prompt and progress bar scanned.
 */
export async function setUp(fixtures: registry.FixtureRegistry, plugin: string): Promise<void> {
  await testenv.up("v15");
  await app.restart({ env: themeEnv(fixtures, "light") });
  await completeWizard("Alice");
  await developerMode();
  await closeSettings();
  await addAndConnect();
  await joinChannel(CHANNEL);
  await selectChannel(CHANNEL);
  await openSettingsGroup("Plugins");
  await install(plugin, "light-install");
  await closeSettings();
  expectNoFindings();
}

/** Restarts the app (same profile) in `theme`, reconnects and opens the channel chat. */
export async function restartIn(fixtures: registry.FixtureRegistry, theme: "light" | "dark"): Promise<void> {
  await app.restart({ env: themeEnv(fixtures, theme) });
  await reconnect();
  await joinChannel(CHANNEL);
  await selectChannel(CHANNEL);
  await browser.waitUntil(
    async () => (await browser.execute(() => document.documentElement.dataset.theme)) === theme,
    { timeoutMsg: `the app did not show the ${theme} theme` },
  );
}

/** F44: at a 150 % font size every clipped plugin card offers Show more. */
export async function checkCardsAtLargeFont(label: string): Promise<void> {
  await browser.execute(() => (document.documentElement.style.fontSize = "150%"));
  await browser.pause(1_000);
  const clipped = await browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>(".plugin-chat-card")).map((card) => {
      const body = card.querySelector<HTMLElement>(".plugin-chat-card__body");
      const overflows = body !== null && body.scrollHeight > body.clientHeight + 1;
      const more = Array.from(card.querySelectorAll("button")).some((b) => b.textContent === "Show more");
      return { label: card.getAttribute("aria-label") ?? "", overflows, more };
    }),
  );
  expect(clipped.length).toBeGreaterThan(0);
  const missing = clipped.filter((c) => c.overflows && !c.more).map((c) => c.label);
  expect(missing).toEqual([]);
  await check(label, [".plugin-chat-card"]);
  await browser.execute(() => (document.documentElement.style.fontSize = ""));
}

/** Reduced motion (GTK animations off): no animation preset and nothing running in plugin UI. */
export async function expectNoPluginMotion(): Promise<void> {
  await browser.pause(1_000);
  const moving = await browser.execute(() => ({
    presets: document.querySelectorAll(".pr-anim").length,
    running: document.getAnimations().filter((a) => {
      const target = (a.effect as KeyframeEffect | null)?.target;
      return target instanceof Element && target.closest(".plugin-badge, .pstrip, .pstrip-sheet") !== null;
    }).length,
  }));
  expect(moving).toEqual({ presets: 0, running: 0 });
}
