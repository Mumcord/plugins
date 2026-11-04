// Whiteboard, issue #397: starting a board and its sealed page. A starts the board from the Apps
// menu: it opens on the stage and posts an invite card; B joins from the card. The entry is also
// offered in the channel menu and as /whiteboard. The board page makes no network requests: a
// `fetch` and an `<img>` to a local address are blocked and never reach it.
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { $, browser, expect } from "@wdio/globals";
import { app, apps, registry, testenv } from "../../helpers/index.ts";
import { channelName } from "../../helpers/voice-ui.ts";
import {
  inBoard,
  inviteCards,
  joinFromCard,
  onA,
  ROOT,
  setUpClient,
  START,
  startFromAppsMenu,
  stroke,
  typeInComposer,
  waitSameBoard,
} from "./board.ts";

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

  before(async function () {
    this.timeout(20 * 60_000);
    fixtures = await registry.start();
    await testenv.up("v15");
  });

  after(async () => {
    await apps.stopB();
    await fixtures.stop();
  });

  it("A and B install Whiteboard in the same channel", async () => {
    await app.restart({ env: fixtures.env() });
    await setUpClient("Alice");
    await apps.startB({ env: fixtures.env() });
    await apps.onB(() => setUpClient("Bob"));
  });

  it("offers Start whiteboard in the channel menu and as /whiteboard", async () => {
    await channelName(ROOT).click({ button: "right" });
    const item = $(`//*[@role='menu']//*[@role='menuitem'][normalize-space(.)='${START}']`);
    await item.waitForDisplayed({ timeout: 10_000 });
    await browser.keys("Escape");

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
