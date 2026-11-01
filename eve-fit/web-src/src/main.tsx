/**
 * The eve-fit web view: shows one fit on eveship.fit's fitting wheel with its statistics.
 *
 * The page runs in Mumcord's sealed frame: no network, no storage, an opaque origin. EVE's data
 * and the calculator come as classic scripts from `data/` (see scripts/build.mjs), and the fit
 * arrives from the plugin over the message bridge.
 */
import "@eveshipfit/ui-ingame/theme.css";
import "./page.css";

import { StrictMode, useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import { createEngine, type Engine, type FitStore } from "@eveshipfit/fitting";
import { loadImages, type Images } from "@eveshipfit/images";
import { DamageProfiles, EveShipFitProvider, ImagesProvider, LocalFits, TextsProvider } from "@eveshipfit/react-hooks";
import { loadSde, loadTexts, type Texts } from "@eveshipfit/sde-loader";
import { FittingWheel, ShipStatistics } from "@eveshipfit/ui-ingame";

import { loadData } from "./data";
import { parseMessage, type Strings, type ViewMessage } from "./message";
import { licences, NOTICE } from "./licences";

interface Loaded {
  readonly engine: Engine;
  readonly images: Images;
  readonly texts: Texts;
}

async function loadEngine(): Promise<Loaded> {
  const [sde, wasm, imagesData, textsData] = await Promise.all([
    loadData("sde"),
    loadData("wasm"),
    loadData("images"),
    loadData("texts"),
  ]);
  const [engine, images, texts] = await Promise.all([
    loadSde({ bytes: sde }).then((loaded) => createEngine(loaded, { wasm })),
    loadImages({ bytes: imagesData }, { baseUrl: "images/" }),
    loadTexts({ bytes: textsData }),
  ]);
  return { engine, images, texts };
}

/** A storage that forgets everything: the sealed frame has no Web Storage. */
function memoryStorage(): Pick<Storage, "getItem" | "setItem"> {
  const items = new Map<string, string>();
  return {
    getItem: (key) => items.get(key) ?? null,
    setItem: (key, value) => void items.set(key, value),
  };
}

/** An IndexedDB that is never there: the fit is read-only, so nothing needs saving. */
const NO_INDEXED_DB = {
  open() {
    const request = new EventTarget() as EventTarget & { error: Error | null };
    request.error = new Error("the sealed web view has no IndexedDB");
    setTimeout(() => request.dispatchEvent(new Event("error")));
    return request;
  },
  // Only `open` is ever called by LocalFits; the cast documents that narrower contract.
} as unknown as IDBFactory;

function Fit({ loaded, fit, strings }: { loaded: Loaded; fit: FitStore; strings: Strings }) {
  const [localFits] = useState(() => new LocalFits({ factory: NO_INDEXED_DB }));
  const [damageProfiles] = useState(() => new DamageProfiles({ storage: memoryStorage() }));
  return (
    <TextsProvider texts={loaded.texts}>
    <ImagesProvider images={loaded.images}>
      <EveShipFitProvider engine={loaded.engine} fit={fit} localFits={localFits} damageProfiles={damageProfiles}>
        <div className="layout">
          <div className="wheel">
            <FittingWheel readOnly label={strings.wheel} />
          </div>
          <div className="stats">
            <ShipStatistics label={strings.stats} />
          </div>
        </div>
      </EveShipFitProvider>
    </ImagesProvider>
    </TextsProvider>
  );
}

type State =
  | { kind: "loading" }
  | { kind: "fit"; loaded: Loaded; fit: FitStore; strings: Strings; seq: number }
  | { kind: "error"; strings: Strings | undefined };

function Page() {
  const [loaded, setLoaded] = useState<Loaded | "failed" | undefined>();
  const [message, setMessage] = useState<{ value: ViewMessage; seq: number } | undefined>();

  useEffect(() => {
    let seq = 0;
    const onMessage = (event: MessageEvent) => {
      if (event.source !== window.parent) return;
      const value = parseMessage(event.data);
      if (value !== undefined) setMessage({ value, seq: ++seq });
    };
    window.addEventListener("message", onMessage);
    // Ask the plugin for the fit once we listen; anything it sent earlier was lost.
    window.parent.postMessage("ready", "*");
    loadEngine().then(setLoaded, (error: unknown) => {
      console.error("eve-fit: loading EVE's data failed", error);
      setLoaded("failed");
    });
    return () => window.removeEventListener("message", onMessage);
  }, []);

  // Memoised: every new fit store would remount the wheel.
  const state = useMemo(() => pageState(loaded, message), [loaded, message]);
  const strings = state.kind === "loading" ? message?.value.strings : state.strings;
  if (strings !== undefined) document.documentElement.lang = strings.lang;
  return (
    <>
      <main>
        {state.kind === "fit" && <Fit key={state.seq} loaded={state.loaded} fit={state.fit} strings={state.strings} />}
        {state.kind === "loading" && (
          <p className="status" role="status">
            {strings?.loading ?? ""}
          </p>
        )}
        {state.kind === "error" && (
          <p className="status" role="alert">
            {state.strings?.error ?? ""}
          </p>
        )}
      </main>
      <footer>
        <p>{NOTICE}</p>
        <details>
          <summary>{strings?.licences ?? "Licences"}</summary>
          <pre>{licences()}</pre>
        </details>
      </footer>
    </>
  );
}

function pageState(
  loaded: Loaded | "failed" | undefined,
  message: { value: ViewMessage; seq: number } | undefined,
): State {
  if (loaded === "failed") return { kind: "error", strings: message?.value.strings };
  if (loaded === undefined || message === undefined) return { kind: "loading" };
  const { strings, fit: text } = message.value;
  try {
    const fit = loaded.engine.createFit(loaded.engine.loadText(text));
    return { kind: "fit", loaded, fit, strings, seq: message.seq };
  } catch (error) {
    // Not a fit the engine knows, e.g. names from a non-English EVE client.
    console.warn("eve-fit: the fit could not be read", error);
    return { kind: "error", strings };
  }
}

const root = document.getElementById("root");
if (root === null) throw new Error("eve-fit: index.html has no #root");
createRoot(root).render(
  <StrictMode>
    <Page />
  </StrictMode>,
);
