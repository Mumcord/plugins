// Pure helpers for sending rendered PNGs over the bridge in chunks.

/**
 * Base64 chunk size. The host's UI takes at most 16 KiB per page -> plugin message from every
 * web view, the stage included (only plugin -> stage messages may reach 64 KiB), so the chunk
 * leaves room for the envelope.
 */
export const PNG_CHUNK = 12 * 1024;

const ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

export function bytesToBase64(bytes) {
  let out = "";
  let i = 0;
  for (; i + 2 < bytes.length; i += 3) {
    const n = (bytes[i] << 16) | (bytes[i + 1] << 8) | bytes[i + 2];
    out += ALPHABET[n >> 18] + ALPHABET[(n >> 12) & 63] + ALPHABET[(n >> 6) & 63] + ALPHABET[n & 63];
  }
  const rest = bytes.length - i;
  if (rest === 1) {
    const n = bytes[i] << 16;
    out += ALPHABET[n >> 18] + ALPHABET[(n >> 12) & 63] + "==";
  } else if (rest === 2) {
    const n = (bytes[i] << 16) | (bytes[i + 1] << 8);
    out += ALPHABET[n >> 18] + ALPHABET[(n >> 12) & 63] + ALPHABET[(n >> 6) & 63] + "=";
  }
  return out;
}

/** Splits a string into parts of at most `max` characters (at least one part, maybe empty). */
export function chunkString(s, max) {
  if (!(max > 0)) throw new RangeError("chunk size must be positive");
  const parts = [];
  for (let i = 0; i < s.length; i += max) parts.push(s.slice(i, i + max));
  if (parts.length === 0) parts.push("");
  return parts;
}

/**
 * The `png` messages answering render request `req`. Chunk boundaries fall on multiples of four
 * characters so every part is valid base64 on its own.
 */
export function pngMessages(req, bytes, max = PNG_CHUNK) {
  const size = Math.max(4, max - (max % 4));
  const parts = chunkString(bytesToBase64(bytes), size);
  return parts.map((data, part) => ({ t: "png", req, part, of: parts.length, data }));
}

/**
 * Output size for a render: the requested scale unless the bitmap would get too large for a
 * canvas, in which case the scale shrinks so the picture still fits.
 */
export function renderScale(w, h, scale, maxSide = 16384, maxArea = 64 * 1024 * 1024) {
  let s = scale;
  if (w * s > maxSide) s = maxSide / w;
  if (h * s > maxSide) s = Math.min(s, maxSide / h);
  if (w * h * s * s > maxArea) s = Math.sqrt(maxArea / (w * h));
  return s;
}
