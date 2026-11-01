# API reference

Plugin API version: **1.0.0**. The contract is the WIT package `mumcord:plugin@1.0.0`
([`crates/plugins/wit`](https://github.com/Mumcord/mumcord/tree/main/crates/plugins/wit) in the
Mumcord repository; the doc comments there are the authoritative reference for every call). Changes
inside 1.x are additive only. A new major version keeps the previous one working for two Mumcord
releases.

The Rust crate `mumcord-plugin-sdk` wraps the interfaces. Add it to `Cargo.toml` (the template does
this for you):

```toml
[dependencies]
mumcord-plugin-sdk = { git = "https://github.com/Mumcord/mumcord", branch = "main" }
```

## Lifecycle

```rust
use mumcord_plugin_sdk::{Plugin, events::Event, types::StartInfo};

struct MyPlugin;

impl Plugin for MyPlugin {
    fn start(info: StartInfo) -> Result<(), String> { Ok(()) }
    fn on_event(event: Event) {}
    fn stop() {}
}

mumcord_plugin_sdk::export!(MyPlugin);
```

- `start` runs once when the plugin is enabled. Return `Err(text)` to refuse to run; the user sees
  that the plugin failed to start. `StartInfo` has `api_version` and `previous_version` (set after an
  update, so you can migrate your own data).
- Right after `start`, the host delivers the current state as ordinary events: `settings-changed`,
  `language-changed`, `theme-changed` and, with `events.self`, `self-changed`. Handle the first
  value like every later one.
- `on_event` receives every event, one at a time. `start` may run for 500 ms and every other call
  for 50 ms; a plugin that exceeds this is stopped.
- `stop` runs when the plugin is disabled or removed. Keep it short.
- The plugin has no threads and no clock of its own: use `timers` and `clock`.

## Modules

All interfaces are modules of the SDK crate root.

| Module | Permission | Purpose |
|---|---|---|
| `log` | none | `debug`, `info`, `warn`, `error`. Shown in the plugin's diagnostics. Never log chat text, names or secrets. |
| `storage` | none | Key-value storage in two scopes: `Identity` (per client certificate) and `Global`. `get_json` / `set_json` helpers, raw `get`, `set`, `delete`, `list_keys`. Quota from the manifest (default 256 KiB, at most 1 MiB). There is no server scope: prefix keys with the server key. |
| `timers` | none | `start(interval_ms, repeat)`, `cancel`. At least 250 ms, at most 32 active timers. Fires the `timer` event. |
| `clock` | none | `now()`. |
| `theme` | none | `current()`: colour tokens, mode, reduced motion. Changes arrive as `theme-changed`. |
| `notify` | `notify` | `send(title, body)`. |
| `random` | `random.bytes` | `bytes(len)`, at most 256 per call. |
| `peers` | `peers.messages`, `events.presence` for `presence` | `send` / `send_json` to `Target::Channel` or one user, `decode_json`, `start_game_burst`, `presence`. |
| `ui` | per slot, see below | Build and show interface trees. |
| `chat` | `chat.send`, `chat.supersedes` | `send` (after a click or command only), `line_hash`, `supersede`. Plugins never read chat text. |
| `secrets` | `secrets` (official only) | Reference-only keychain secrets. |
| `http` | `net.http` (official only) | Requests to a host the user typed into the plugin's form. |
| `webview` | `ui.webview` (official only) | A sealed web page from the package's `web/` folder. |

Every call that can fail returns a `types::Error`: `NotPermitted`, `TooLarge`, `RateLimited`,
`NoRecipients`, `Hidden`, `QuotaExceeded` or `Invalid`. Handle them: a missing permission is a normal
state for optional permissions. Never ignore errors silently, except cases that are normal (for
example `NoRecipients` when nobody else runs the plugin).

## Events

`on_event` gets one `Event` variant at a time.

| Event | Needs | Meaning |
|---|---|---|
| `Talking((peer, bool))` | `events.voice` | A user started or stopped talking. |
| `Joined(peer)`, `Left(peer)`, `ScreenShareStarted(peer)` | `events.presence` | Channel presence. `Joined` is also sent for everyone already there when you move into a channel. |
| `ChatActivity((peer, channel))`, `Mentioned(peer)` | `events.chat-activity` | Someone wrote or mentioned you. Never the text. |
| `SelfChanged(state)` | `events.self` | Your mute, deafen, do-not-disturb, channel and server. |
| `ThemeChanged`, `LanguageChanged`, `SettingsChanged` | none | Theme, language, granted permissions and visibility. |
| `UiAction(action)` | none | A click, change or input in your UI, or a slash command (`source = Command`, the value is the rest of the line). |
| `Timer(id)` | none | A timer fired. |
| `PeerMessage(message)` | `peers.messages` | A message from the same plugin of another user. Untrusted: validate. |
| `ProfileCardOpened(peer)` | `ui.profile-card` | Answer by setting the `ProfileCard` slot within 500 ms. |
| `PanelVisibilityChanged`, `UserMenuRequested`, `DialogClosed` | slot permission | UI state. |
| `HttpResponse(result)` | `net.http` | An `http.request` finished. |
| `WebviewMessage(text)` | `ui.webview` | A message from your web page, at most 16 KiB. Untrusted. |

Revoked permissions stop events at once; check `SettingsChanged` or handle `NotPermitted`.

## Peers

A peer has a `session` (valid until they disconnect), a `name` (untrusted text) and an optional
`key`: a stable per-plugin identity (64 hex characters) that is different in every plugin. Plugins
never see certificate hashes, IP addresses or registered user ids. Nothing from or about a blocked
user reaches a plugin.

Messages go only to the same plugin id from the same source in Mumcord clients in the user's current
channel. Limits: at most 997 bytes (or `messaging.max_payload`); per plugin one message per 2 s
sustained (burst of 3); all plugins together one per second (burst of 5). A real-time game can call
`start_game_burst` for 4 messages per second for 10 seconds, at most once per minute. Use the first
byte-sized `kind` of your own protocol to version your messages, and ignore kinds you do not know.

## UI

Build a tree with the helpers in `ui` and show it in a slot:

```rust
let tree = ui::column()
    .child(ui::heading("Hello"))
    .child(ui::button("wave", "Wave").animate(Animation::Wave));
ui::show(Slot::Panel, tree)?;
```

Components: `column`, `row`, `stack`, `text`, `heading`, `caption`, `styled_text`, `image`,
`sprite`, `meter`, `button`, `icon_button`, `chip`, `divider`, `progress`, `list`, `toggle`,
`select`, `slider`, `text_input`, `password`, `file_picker`, `countdown`. Interactive components
need a label for screen readers. Colours are theme tokens, never RGB values.

| Slot | Permission | Size |
|---|---|---|
| `Panel` | `ui.panel` | 260x160 inline in the Plugins strip, 420x560 expanded |
| `MemberBadge(session)` | `ui.member-badge` | 24 px next to a user name (one badge per user) |
| `ProfileCard(session)` | `ui.profile-card` | 300x220 section |
| `ChatCard(id)`, `ChatDecoration(session)` | `ui.chat-card` | 440x160 local chat item, 16 px decoration |
| Dialog | `ui.dialog` | Own window, opened with `ui::open_dialog` |
| Settings form | none | The plugin's form in Settings > Plugins |

User-menu entries use `set_user_menu_entry` and the `ui.user-menu` permission. Limits per slot: 500 nodes, 16 KiB
encoded tree and 20 updates per second; `ui::show` returns `Invalid` for malformed trees. The
component fields and remaining limits are in `ui.wit`.

Plugins do not decide how things look: layout, fonts, colours, focus and keyboard handling come
from Mumcord, so every plugin follows the user's theme, scaling and reduced-motion setting.

## Untrusted input

Peer names, peer message payloads and web view messages come from other people. Check lengths and
ranges, never trust a sender claim in the payload (the sender is the `peer`), and design messages
so that a forged one cannot do harm.
