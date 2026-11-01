# EVE fit

Official Mumcord plugin: EVE Online ship fits pasted into the chat become a tidy card, with the full fit one click away.

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
- **Open** shows the full fit in a dialog: every section with its modules and counts, loaded charges, offline modules, empty slots, drones and cargo.
- **Copy fitting**, on the card and in the dialog, copies the fit exactly as it was pasted, ready to import into EVE (Fittings → Import from clipboard) or Pyfa. Mumcord does the copying when you click; the plugin never touches your clipboard.
- The pasted text stays in the chat as it is, so people on the official Mumble client, or on Mumcord without the plugin, see the plain fit.
- The plugin keeps the last 30 fits. An older card stays in the chat but loses its Open button; paste the fit again to open it.

## Good to know

- **Text only.** The plugin ships no EVE data and no EVE images, and it never goes online. Everything on the card comes from the pasted text.
- **Drones or cargo?** Without EVE data, drones and cargo look the same in a fit (both are "name x count"). When a fit has two such blocks, the first is shown as drones and the second as cargo; a single block is shown as "Drones / cargo".
- **Sections** follow the EFT order: low, mid, high, rigs, subsystems. An `[Empty Mid slot]` style line places a section when the fit skips one.
- **Only what you paste.** The plugin only receives chat messages that start with a fit header such as `[Rifter, Tackle]` (the "read chat messages that look like an EVE fit" permission). It never sees any other chat.

## Permissions

| Permission | Why |
|---|---|
| `chat.patterns` | Receive channel messages that start with an EVE fit header, and nothing else. |
| `ui.chat-card` | Show the fit card in the chat. |
| `ui.dialog` | Show the full fit when you choose Open. |

## Build and test

```sh
cargo test                                             # parser and card logic, natively
cargo build --release --target wasm32-unknown-unknown  # the plugin module
mumcord-plugin test                                    # the scenarios in scenarios/
```

## Translations

All texts live in `i18n/<language>.json`, one file for each of Mumcord's 24 languages.

## Trademarks

EVE Online and the EVE logo are the registered trademarks of CCP hf. All rights are reserved worldwide. This plugin is not affiliated with or endorsed by CCP hf. It contains no EVE game data, names or images of its own; it only displays the fit text that users paste into the chat.

## Licence

AGPL-3.0-or-later, see [LICENSE](LICENSE).
