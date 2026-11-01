# EVE fit

Official Mumcord plugin: EVE Online ship fits pasted into the chat become a tidy card, and **Open** shows the fit on the [EVEShip.fit](https://eveship.fit) fitting wheel with its full statistics.

## How it works

- Someone in your channel pastes a fit in EFT format, the text that EVE's fitting window ("Copy to clipboard") and tools such as Pyfa export:

  ```text
  [Rifter, Tackle]
  Damage Control II
  Gyrostabilizer II
  [Empty Low slot]

  5MN Microwarpdrive II
  Warp Scrambler II

  200mm AutoCannon II, Republic Fleet EMP S
  200mm AutoCannon II, Republic Fleet EMP S
  [Empty High slot]

  Small Projectile Burst Aerator I


  Warrior II x3


  Nanite Repair Paste x50
  ```

- Below the message, Mumcord shows a card with the ship, the fit name and who pasted it, a slot summary (for example "Low 2/3 · Mid 2/2 · High 2/3 · Rigs 1/1 · Drones 3") and the most common modules.
- **Open** shows the fit in a dialog the way EVEShip.fit does: the ship render inside the fitting wheel with every module icon in its slot, hardpoints, CPU and power grid, and the statistics for capacitor, offence, defence, targeting, navigation and drones (calculated with all skills at level V). The view is read-only.
- If Mumcord cannot show the fitting wheel (for example when the web view permission is turned off), **Open** shows the fit as a list instead: every section with its modules and counts, loaded charges, offline modules, empty slots, drones and cargo.
- **Copy fitting**, on the card and in the list, copies the fit exactly as it was pasted, ready to import into EVE (Fittings → Import from clipboard) or Pyfa. Mumcord does the copying when you click; the plugin never touches your clipboard.
- The pasted text stays in the chat as it is, so people on the official Mumble client, or on Mumcord without the plugin, see the plain fit.
- The plugin keeps the last 30 fits. An older card stays in the chat but loses its Open button; paste the fit again to open it.
- **Fit history.** The plugin's panel in the Plugins strip shows the newest fit; **Fit history** opens the full list: every fit seen in the chat or opened, newest first, with ship, fit name, who pasted it and when. The list starts collapsed. Click an entry to open the fit again, or use its own **Copy fitting** button. The same fit pasted again is listed once and moves to the top. The last 50 fits are kept on your device for your identity, so the history survives a restart; long lists are shown in pages (Older fits / Newer fits).

## Good to know

- **Offline.** The plugin never goes online. The card comes from the pasted text and the lists of EVE's item names bundled in the plugin. The fitting wheel runs in Mumcord's sealed web view, which has no internet access: EVE's data, icons and ship renders are bundled in the plugin (`web/`), so a new EVE release needs a plugin update before new ships and modules show up.
- **Any EVE language.** Fits copied from EVE in another language (French, Spanish, Japanese, Chinese, Korean; the German and Russian clients use the English item names) work too: the plugin bundles the item names in every language EVE supports, sorts the items by them and hands the fitting wheel the fit with English names. The card and Copy fitting keep the fit as it was pasted. A fit the fitting wheel still cannot read shows a short message; the card and Copy fitting still work.
- **Sections.** Each item goes where EVE's data says it goes (low, mid, high, rigs, subsystems, drones, cargo, implants), so a fit still comes out right when its blank lines got lost on the way through the chat. Items the data does not know follow the EFT layout: blank-line separated sections in the order low, mid, high, rigs, subsystems, with an `[Empty Mid slot]` style line placing a section when the fit skips one. Of two "name x count" blocks the first is drones and the second cargo; a single block of unknown items is shown as "Drones / cargo".
- **Only what you paste.** The plugin only receives chat messages that start with a fit header such as `[Rifter, Tackle]` (the "read chat messages that look like an EVE fit" permission). It never sees any other chat.

## Permissions

| Permission | Why |
|---|---|
| `chat.patterns` | Receive channel messages that start with an EVE fit header, and nothing else. |
| `ui.chat-card` | Show the fit card in the chat. |
| `ui.dialog` | Show the full fit when you choose Open. |
| `ui.webview` | Show the fitting wheel in the dialog, in a sealed web view without internet access. |
| `ui.panel` (optional) | Show the newest fit and the fit history in the Plugins strip. |

## Build and test

```sh
web-src/build.sh                                       # rebuild web/ (Node 24+, see below)
cargo test                                             # parser and card logic, natively
cargo build --release --target wasm32-unknown-unknown  # the plugin module
mumcord-plugin test                                    # the scenarios in scenarios/
```

## The fitting wheel (`web/`)

`web/` is built by `web-src/build.sh` and committed, so the package builds reproducibly from the source. The script:

- installs the exact npm packages pinned in `web-src/package-lock.json` (npm checks their SHA-512 hashes): EVEShip.fit's `@eveshipfit/ui-ingame`, `react-hooks`, `fitting`, `sde-loader` and `dogma-engine`, EVE's data `@eveshipfit/sde` and icons `@eveshipfit/images`, and React;
- bundles the page into `web/app.js` and `web/app.css` as classic scripts (the sealed frame cannot load modules);
- writes `src/items.txt` (where each module goes) and `src/names.txt` (item names in EVE's other languages) from EVE's data, which the plugin embeds;
- wraps EVE's data and the calculator (WebAssembly) as base64 in `web/data/*.js`, because the page cannot fetch files;
- downloads the 512 px ship renders from CCP's image server, checks them against `web-src/renders.lock` and converts them to WebP in `web/renders/`.

`web-src/build.sh --check` rebuilds into a temporary folder and fails if `web/` differs. To update to a new EVE release, bump `@eveshipfit/sde` and `@eveshipfit/images` in `web-src/package.json`, run `npm install` there, then `web-src/build.sh --update-renders` (it records new or changed renders in `renders.lock`).

The page gets the fit from the plugin over the web view bridge: it sends `ready`, and the plugin answers with `{"fit": "<EFT text>", "strings": {…}}`, the page's texts in the user's language.

## Translations

All texts live in `i18n/<language>.json`, one file for each of Mumcord's 24 languages.

## EVE Online data and trademarks

© CCP hf. All rights reserved. "EVE", "EVE Online", "CCP", and all related logos and images are trademarks or registered trademarks of CCP hf.

The fitting wheel bundles EVE Online game data (`@eveshipfit/sde`), icons (`@eveshipfit/images`) and ship renders from CCP's image server. They belong to CCP hf. and are used under CCP's [Developer License Agreement](https://developers.eveonline.com/license-agreement), which allows non-commercial use only: this plugin and Mumcord are free, without payment or advertising. The plugin is not affiliated with or endorsed by CCP hf. The fitting wheel page shows this notice below every fit.

## Licence

The plugin is AGPL-3.0-or-later, see [LICENSE](LICENSE).

The fitting wheel bundles third-party code: EVEShip.fit's packages (MIT, © EVEShip.fit Team), React (MIT, © Meta Platforms, Inc. and affiliates) and FlatBuffers (Apache-2.0, © Google Inc.). Their licence texts and CCP's licence are in `web/licences.html` and under "Licences" on the page.
