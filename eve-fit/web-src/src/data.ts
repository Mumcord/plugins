/**
 * EVE's data and the calculator, loaded from the classic scripts in `data/`.
 *
 * The sealed frame cannot fetch files, so scripts/build.mjs wraps each binary file as base64 in
 * one or more scripts. `data/parts.js` sets how many parts each file has; every part calls
 * `eveFitData(name, index, base64)` when it runs.
 */

export type DataName = "sde" | "wasm" | "images" | "texts";

declare global {
  interface Window {
    eveFitParts?: Partial<Record<DataName, number>>;
    eveFitData?: (name: string, index: number, base64: string) => void;
  }
}

const received = new Map<string, string>();

window.eveFitData = (name, index, base64) => {
  received.set(`${name}/${index}`, base64);
};

function loadScript(src: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const script = document.createElement("script");
    script.src = src;
    script.onload = () => resolve();
    script.onerror = () => reject(new Error(`could not load ${src}`));
    document.head.append(script);
  });
}

export function decodeBase64(parts: readonly string[]): Uint8Array {
  const total = parts.reduce((sum, part) => sum + (part.length / 4) * 3, 0);
  const bytes = new Uint8Array(total);
  let at = 0;
  for (const part of parts) {
    const binary = atob(part);
    for (let i = 0; i < binary.length; i++) bytes[at++] = binary.charCodeAt(i);
  }
  // Padding of the last part makes the estimate a few bytes too long.
  return bytes.subarray(0, at);
}

export async function loadData(name: DataName): Promise<Uint8Array> {
  const count = window.eveFitParts?.[name];
  if (count === undefined) throw new Error(`data/parts.js lists no ${name}`);
  const indexes = Array.from({ length: count }, (_, i) => i);
  await Promise.all(indexes.map((i) => loadScript(`data/${name}-${i}.js`)));
  const parts = indexes.map((i) => {
    const part = received.get(`${name}/${i}`);
    if (part === undefined) throw new Error(`data/${name}-${i}.js did not deliver its part`);
    received.delete(`${name}/${i}`);
    return part;
  });
  return decodeBase64(parts);
}
