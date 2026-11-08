// Whiteboard, issue #397: starting a board and its sealed page. A starts the board from the Apps
// menu: it opens on the stage and posts an invite card; B joins from the card. The entry is also
// offered as /whiteboard. The board page makes no network requests: a `fetch` and an `<img>` to a
// local address are blocked and never reach it. The board travels only over a relay that allows
// plugin data: on a server without one, the Apps entry is disabled with the reason and /whiteboard
// explains it.
//
// Not automated: the channel menu entry of #397. Mumcord shows plugin app menu items only in the
// composer's Apps menu and the plugins palette, not in the channel context menu, so there is no
// host place to test it yet.
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { $, browser, expect } from "@wdio/globals";
import { app, apps, registry, testenv } from "../../helpers/index.ts";
import {
  inBoard,
  inviteCards,
  joinFromCard,
  onA,
  pluginDialog,
  setUpClient,
  startItem,
  startFromAppsMenu,
  startRelay,
  stroke,
  typeInComposer,
  waitSameBoard,
} from "./board.ts";

const NO_RELAY = "Whiteboard needs a relay on this server.";
const REFUSED = "Couldn't start the whiteboard";

/** A local web server that counts every request (it must stay at zero). */
async function countingServer(): Promise<{ url: string; hits: () => number; stop: () => void }> {
  let hits = 0;
  const server: Server = createServer((_req, res) => {
    hits += 1;
    res.writeHead(200, { "content-type": "text/plain", "access-control-allow-origin": "*" });
    res.end("reached");
  });
  await new Promise<void>((done) => server.listen(0, "127.0.0.1", done));
  const { port } = server.address() as AddressInfo;
  return { url: `http://127.0.0.1:${String(port)}/`, hits: () => hits, stop: () => server.close() };
}

describe("Whiteboard: start, invite card and a sealed page", () => {
  let fixtures: registry.FixtureRegistry;
  /** The Mumble server of the relay environment. */
  let port = 0;

  before(async function () {
    this.timeout(20 * 60_000);
    fixtures = await registry.start();
    port = await startRelay();
  });

  after(async () => {
    await apps.stopB();
    await fixtures.stop();
  });

  it("without a relay, the Apps entry is disabled with the reason and /whiteboard explains it", async () => {
    await testenv.up("v15");
    await app.restart({ env: fixtures.env() });
    await setUpClient("Alice");
    await $("button.chat-apps__button").click();
    const item = startItem();
    await item.waitForDisplayed({ timeout: 10_000 });
    await expect(item).toHaveAttribute("aria-disabled", "true");
    await expect(item.$(".chat-apps__reason")).toHaveText(NO_RELAY);
    await browser.keys("Escape");

    await typeInComposer("/whiteboard ");
    await browser.keys("Enter");
    const dialog = pluginDialog(REFUSED);
    await dialog.waitForDisplayed({ timeout: 15_000 });
    await expect(dialog).toHaveText(expect.stringContaining(NO_RELAY));
    await browser.keys("Escape");
    await dialog.waitForDisplayed({ reverse: true });
  });

  it("A and B install Whiteboard in the same channel", async () => {
    await app.restart({ freshProfile: true, env: fixtures.env() });
    await setUpClient("Alice", port);
    await apps.startB({ env: fixtures.env() });
    await apps.onB(() => setUpClient("Bob", port));
  });

  it("offers /whiteboard in the chat box", async () => {
    await typeInComposer("/whiteboard");
    const option = $("//div[@role='listbox'][@aria-label='Plugin commands']//*[@role='option']");
    await option.waitForDisplayed({ timeout: 10_000 });
    await expect(option).toHaveText(expect.stringContaining("/whiteboard"));
    await typeInComposer("");
  });

  it("starts from the Apps menu on the stage with an invite card, B joins from it", async () => {
    await startFromAppsMenu();
    await browser.waitUntil(async () => (await inviteCards().length) > 0, {
      timeout: 15_000,
      timeoutMsg: "A got no invite card",
    });
    await expect(inviteCards()[0]).toHaveText(expect.stringContaining("You started a whiteboard"));

    await apps.onB(async () => {
      await browser.waitUntil(async () => (await inviteCards().length) > 0, {
        timeout: 30_000,
        timeoutMsg: "B got no invite card",
      });
      await expect(inviteCards()[0]).toHaveText(
        expect.stringContaining("Alice started a whiteboard"),
      );
      await joinFromCard();
    });

    await inBoard(() => stroke(-100, 0));
    await waitSameBoard([onA, apps.onB], 1);
  });

  it("blocks fetch and img requests from the board page", async () => {
    const server = await countingServer();
    try {
      const results = await inBoard(() =>
        browser.execute(
          (url: string) =>
            new Promise<{ fetch?: string; img?: string }>((done) => {
              const out: { fetch?: string; img?: string } = {};
              const finish = () => {
                if (out.fetch !== undefined && out.img !== undefined) done(out);
              };
              fetch(url)
                .then(
                  () => (out.fetch = "reachable"),
                  (e: unknown) => (out.fetch = `blocked ${String(e)}`),
                )
                .finally(finish);
              const img = new Image();
              img.onload = () => {
                out.img = "reachable";
                finish();
              };
              img.onerror = () => {
                out.img = "blocked";
                finish();
              };
              img.src = `${url}pixel.png`;
              document.body.append(img);
            }),
          server.url,
        ),
      );
      expect(results.fetch).toMatch(/^blocked/);
      expect(results.img).toBe("blocked");
      expect(server.hits()).toBe(0);
    } finally {
      server.stop();
    }
  });
});
