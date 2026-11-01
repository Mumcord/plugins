---
title: Mumcord Plugins
hide:
  - navigation
  - toc
---

<div class="mp-hero" markdown>

# Mumcord Plugins

**Your Mumble server. Finally cosy.** Plugins are small, safe extras on top of voice chat: polls,
dice, pets, a remote for your music bot and, soon, EVE Online fits. They run inside Mumcord, need
no server changes and work on any standard Mumble server.

[Install a plugin](install.md){ .md-button .md-button--primary }
[Build your own](getting-started.md){ .md-button }

</div>

## Official plugins

<div class="grid cards" markdown>

-   <img src="../poll/assets/icon-128.png" alt="" width="48" height="48"> **[Poll](plugins/poll.md)**

    ---

    Ask the channel a question with up to ten options and watch the votes come in live.

-   <img src="../dice/assets/icon-128.png" alt="" width="48" height="48"> **[Dice roller](plugins/dice.md)**

    ---

    `/roll d20` or `2d6+3` with fair rolls that others can witness and verify.

-   <img src="../echolings/assets/icon-128.png" alt="" width="48" height="48"> **[Echolings](plugins/echolings.md)**

    ---

    A little creature made of sound that grows while you hang out with friends.

-   <img src="../mumbot-music/assets/icon-128.png" alt="" width="48" height="48"> **[Mumbot Music](plugins/mumbot-music.md)**

    ---

    A remote for your mumbot music bot: now playing, skip, volume, queue and adding tracks.

-   **[EVE Fit](plugins/eve-fit.md)** <span class="mp-soon">Coming soon</span>

    ---

    Paste an EVE Online fitting in chat and everyone sees a ship card.

</div>

## Why it feels safe

<div class="grid cards" markdown>

-   :material-shield-check: **Safe sandbox**

    ---

    Each plugin is a small WebAssembly program in a strict sandbox. No raw network or file
    access. [Safety model](safety.md)

-   :material-eye-check: **Permissions you can see**

    ---

    Listed before you install. Optional ones can be turned off any time.
    [Permissions explained](permissions-explained.md)

-   :material-draw-pen: **Signed**

    ---

    The registry and every package are signed, and Mumcord checks them before installing.

-   :material-server-off: **No server changes**

    ---

    Works against an unmodified Mumble server. People on the official Mumble client see a plain
    text line where it matters and nothing else.

</div>

## For developers

Plugins are written in **Rust** and compiled to **WebAssembly**. Start with
[Getting started](getting-started.md), then read the [API reference](api.md) and the
[review policy](../policy/REVIEW.md) before you submit your plugin.
