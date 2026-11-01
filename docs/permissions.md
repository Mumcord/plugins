# Permissions

Declare permissions in the manifest under `permissions.required` and `permissions.optional`. The
user sees them, with the risk colour, before installing and can switch optional ones off at any
time. An update that asks for a permission it did not have before waits for the user's approval.
Ask for as few as possible; a plugin that works without a permission should list it as optional.

Calls without the needed permission return `NotPermitted`. The permission texts users see come from
the Mumcord catalogue, not from your plugin.

| Permission | Risk | What it unlocks |
|---|---|---|
| `ui.panel` | green | A panel in the Plugins strip |
| `ui.member-badge` | green | A small badge next to user names |
| `ui.profile-card` | green | A section on profile cards |
| `ui.chat-card` | green | Cards and name decorations in the chat |
| `ui.user-menu` | green | An entry in a user's right-click menu |
| `ui.dialog` | green | The plugin's own window |
| `events.self` | green | Your own status (mute, deafen, channel) |
| `events.voice` | yellow | Who is talking |
| `events.presence` | yellow | Who joins and leaves your channel, and the user list |
| `events.chat-activity` | yellow | When someone writes or mentions you (never the text) |
| `peers.messages` | yellow | Exchanging data with the same plugin of other users in your channel |
| `notify` | green | Notifications |
| `random.bytes` | green | Secure random numbers |
| `commands` | green | Chat commands such as `/roll` (declared in the manifest) |
| `chat.send` | yellow | Posting a chat message for you after a click or a command |
| `chat.supersedes` | green | Hiding the plugin's own text line for people who see its card |
| `chat.triggers` | yellow | Chat lines that start with one of the plugin's trigger words (on every connected server) |
| `chat.patterns` | yellow | Chat lines that match one of the plugin's declared patterns (on every connected server) |
| `net.http` | red | Talking to a server the user chose (official plugins only) |
| `secrets` | red | A secret in the system keychain (official plugins only) |
| `ui.webview` | yellow | A sealed web view with no internet (official plugins only) |

The three official-only permissions are refused for packages from third-party repositories and
developer folders with: "This permission is only available to official plugins". Plugins from
third-party repositories show as "Unofficial" in the browse view.

The live catalogue is `https://mumcord.github.io/plugins/v1/permissions.json`.

## Permission design tips

- **Prefer the smallest permission.** Showing a card needs `ui.chat-card`; reading chat text is
  never possible, and `events.chat-activity` only tells you that someone wrote.
- **`chat.send` needs a gesture.** A message is only sent within 2 seconds after a click on your
  button or a slash command, one message of at most 500 characters per gesture. The host prefixes
  the plugin name, so people on other Mumble clients can see where it came from.
- **Make peers optional-safe.** Messages only reach users who also run your plugin. Send something
  readable with `chat.send` where it matters, and use `chat.supersedes` so Mumcord users see your
  card instead of the plain text line.
- **Handle revocation.** The user can revoke an optional permission while the plugin runs. React to
  `SettingsChanged` and to `NotPermitted`, and keep working with what is left.

## New optional permissions in an update

When an update adds optional permissions, Mumcord does not leave them silently off. The Updates tab
(and auto-update) shows "New optional permission: ..." for each one with a toggle, switched on for
permissions you marked `suggested: true`. Permissions the user already decided about are not asked
again, and an update never turns on a required permission without the usual consent step.

Even so, a user can say no. If a feature depends on an optional permission, show a hint when it is
missing and call `ui::open_permissions()` from its button, so the user lands on the switch.
