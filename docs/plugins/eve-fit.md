# EVE Fit <span class="mp-soon">Coming soon</span>

> [!NOTE]
> EVE Fit is not released yet. This page describes what it will do once it is available in
> **Settings → Plugins → Browse**.

Paste an EVE Online fitting in chat and everyone sees a ship card.

## How it works

- Someone in your channel pastes a fit in **EFT format**, the text that EVE's fitting window and
  tools such as Pyfa export. It starts with a header such as `[Rifter, Tackle]`.
- Mumcord shows a **card** with the ship, the fit name, who pasted it, a slot summary and the most
  common modules.
- **Open** shows the full fitting view, offline inside Mumcord.
- **Copy fitting** copies the fit exactly as it was pasted, ready to import into EVE or Pyfa.
- The pasted text stays in the chat as it is, so people without the plugin see the plain fit.

EVE Fit is free.

## Only what you paste

The plugin only receives chat messages that start with a fit header such as `[Rifter, Tackle]`. It
never sees any other chat.

## Permissions

| Permission | Why |
|---|---|
| Read chat messages that look like an EVE fit (required) | Receive messages that start with a fit header, and nothing else |
| Chat cards (required) | Show the fit card in the chat |
| Window (required) | Show the full fit when you choose **Open** |

## Credits and notices

The full fitting view is based on [eveship.fit](https://eveship.fit) by the EVEShipFit project,
used under its licence.

EVE Online and all related logos and images are trademarks or registered trademarks of CCP hf.
Mumcord is not affiliated with CCP hf.
