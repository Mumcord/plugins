// Pure state for laser trails: timed points that fade out and get pruned.

export const TRAIL_MS = 900;
export const TRAIL_MAX = 240;

/** Adds flat [x0, y0, x1, y1, …] points stamped with `now`. */
export function addTrail(trail, flat, now) {
  for (let i = 0; i + 1 < flat.length; i += 2) {
    const x = flat[i];
    const y = flat[i + 1];
    if (Number.isFinite(x) && Number.isFinite(y)) trail.push({ x, y, t: now });
  }
  if (trail.length > TRAIL_MAX) trail.splice(0, trail.length - TRAIL_MAX);
  return trail;
}

/** Drops points older than `ttl`; the newest point stays as the dot until it too expires. */
export function pruneTrail(trail, now, ttl = TRAIL_MS) {
  let i = 0;
  while (i < trail.length && now - trail[i].t > ttl) i++;
  if (i > 0) trail.splice(0, i);
  return trail;
}

/** Opacity of a trail segment of age `age` ms: 1 when new, 0 at `ttl`. */
export function trailAlpha(age, ttl = TRAIL_MS) {
  if (age <= 0) return 1;
  if (age >= ttl) return 0;
  return 1 - age / ttl;
}
