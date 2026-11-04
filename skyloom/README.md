# Skyloom screenshots

Screenshots of the real Skyloom page (`skyloom/web` at e20adad on `feat/skyloom`; weaving,
reveal and results states re-shot at c45bd6f), for
Mumcord/plugins#21. The page runs in headless Chromium inside the mockup's app chrome from
Mumcord/mumcord#394 (branch `mockups/official-game`), so the sidebar, chat and surface header
come from the mockup; everything inside the game frame is the plugin's own page, fed with view
JSON recorded from the plugin's Rust core playing a real night with ten simulated clients.

- `NN-*-dark.png` / `NN-*-light.png`: the states the mockups show, at the mockups' 1440 × 900.
- `xN-*`: states without a mockup (Lamplighter lobby, Weaver at dawn, Lamplighter results,
  spectator at dawn in the side panel).
- `compare/`: the game frame only, mockup on top and the plugin below.
- Weaving (Threader): after the recorded star positions, the harness moves two stars a little
  further so the page draws its dotted trails behind them.
