/** The plugin's message to the page: the fit as pasted, and the page's texts in the user's language. */

export interface Strings {
  readonly lang: string;
  readonly loading: string;
  readonly error: string;
  readonly licences: string;
  readonly wheel: string;
  readonly stats: string;
}

export interface ViewMessage {
  readonly fit: string;
  readonly strings: Strings;
}

const STRING_KEYS = ["lang", "loading", "error", "licences", "wheel", "stats"] as const;

/** The message, or undefined for anything that is not one (bridge data is untrusted). */
export function parseMessage(data: unknown): ViewMessage | undefined {
  if (typeof data !== "string") return undefined;
  let value: unknown;
  try {
    value = JSON.parse(data);
  } catch {
    return undefined;
  }
  if (typeof value !== "object" || value === null) return undefined;
  const { fit, strings } = value as { fit?: unknown; strings?: unknown };
  if (typeof fit !== "string" || typeof strings !== "object" || strings === null) return undefined;
  const texts = strings as Record<string, unknown>;
  if (!STRING_KEYS.every((key) => typeof texts[key] === "string")) return undefined;
  return { fit, strings: strings as Strings };
}
