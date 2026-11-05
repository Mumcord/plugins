// Theme, paper and motion settings on <html>. The plugin may send `theme` ("dark"/"light") and
// `reducedMotion` in `init` (it knows Mumcord's theme); otherwise the frame's media queries decide,
// which the sealed frame inherits from Mumcord's window.

const PAPERS = ["auto", "light", "dark", "dots", "grid", "plain"];

export function createTheme(onChange) {
  const root = document.documentElement;
  const darkQuery = matchMedia("(prefers-color-scheme: dark)");
  const motionQuery = matchMedia("(prefers-reduced-motion: reduce)");
  const contrastQuery = matchMedia("(prefers-contrast: more)");
  const state = { theme: null, paper: "auto", reduced: null };

  const apply = () => {
    const theme = state.theme ?? (darkQuery.matches ? "dark" : "light");
    root.dataset.theme = theme;
    // light/dark force the paper colour; dots/grid/plain only change the pattern.
    root.dataset.paper = state.paper === "light" || state.paper === "dark" ? state.paper : theme;
    root.dataset.pattern = ["grid", "plain"].includes(state.paper) ? state.paper : "dots";
    root.dataset.motion = api.reducedMotion() ? "reduce" : "full";
    onChange();
  };

  const api = {
    set(msg) {
      if (msg.theme === "dark" || msg.theme === "light") state.theme = msg.theme;
      if (typeof msg.reducedMotion === "boolean") state.reduced = msg.reducedMotion;
      if (PAPERS.includes(msg.paper)) state.paper = msg.paper;
      apply();
    },
    reducedMotion() {
      return state.reduced ?? motionQuery.matches;
    },
    pattern() {
      return root.dataset.pattern;
    },
  };
  for (const q of [darkQuery, motionQuery, contrastQuery]) q.addEventListener("change", apply);
  apply();
  return api;
}
