# Testing

## 1. Unit tests on your own machine

The host interfaces only exist inside Mumcord, so a plugin crate cannot call them in a normal
`cargo test`. Keep the logic in plain Rust and put the host calls behind a small trait:

```rust
trait Host {
    fn get_count(&self) -> u32;
    fn set_count(&self, n: u32);
    fn send_wave(&self);
}

fn handle_wave(host: &impl Host) -> u32 {
    let n = host.get_count().saturating_add(1);
    host.set_count(n);
    host.send_wave();
    n
}
```

The real implementation calls `storage` and `peers`; tests use a fake that records calls. Test the
message encoding, limits, bad input from peers and every error path (`NotPermitted`,
`RateLimited`, `NoRecipients`). The official Poll plugin (`poll/` in this repository) is built this
way and is a good example. Compile the crate for the host target for tests
(`cargo test --lib`), and keep host-only code behind `#[cfg(target_arch = "wasm32")]`.

## 2. Check the package

```sh
mumcord-plugin build
mumcord-plugin check dist/my-plugin-0.1.0.mcplugin
```

`check` reports exactly what the client would reject: manifest errors, unknown or forbidden
permissions, missing English texts, oversized files, wasm that imports anything but
`mumcord:plugin/*`. Fix these first.

## 3. Scenario tests

`mumcord-plugin test` runs your plugin in the same runtime as the client against a mock host. A
scenario is a YAML file that sends events in and checks what comes out: UI trees, messages to
other users, chat lines, notifications and log lines. Nothing reaches the network or the keychain,
and every scenario starts with a fresh storage database.

```sh
mumcord-plugin test dist/my-plugin-0.1.0.mcplugin scenarios/hello.yaml
```

The first argument is a built `.mcplugin` package or an unpacked plugin folder, followed by one or
more scenario files. Each scenario prints `ok <name>` or `FAIL <name>` with the reason, and the
command exits with an error if any scenario failed. `mumcord-plugin new` creates a working
`scenarios/hello.yaml` for you. A short example:

```yaml
name: Waving counts and tells the channel
grant: [ui.panel, peers.messages]
me: { session: 1, name: Alice }
peers:
  - { session: 2, name: Bob }

steps:
  - expect: { tree: { slot: panel, text: "Waves: 0" } }
  - event: { ui-action: { action: wave } }
  - expect: { message: { to: channel, kind: 1, text: "null" } }
  - expect: { tree: { slot: panel, text: "Waves: 1" } }
  - event: { peer-message: { from: 2, kind: 1, text: "null" } }
  - expect: { log: { contains: someone waved } }
  - expect: nothing
```

Top-level keys (unknown keys are rejected):

| Key        | Meaning                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------- |
| `name`     | Shown in the result. Defaults to the file path.                                          |
| `grant`    | Granted permissions. Default: the required ones plus the suggested optional ones.        |
| `me`       | You: `{session, name, anonymous}`. Default: session 1, "Me". `anonymous: true` = no key. |
| `peers`    | Other users in the channel who run the same plugin, same fields as `me`.                 |
| `language` | UI language, default `en`.                                                               |
| `theme`    | `light` (default) or `dark`.                                                             |
| `steps`    | The steps, run in order.                                                                 |

A step is one of:

- `event:` sends an event to the plugin. Kinds: `ui-action` (`action`, optional `slot`, `command`
  for a typed slash command, `value`), `peer-message` (`from`, `kind`, `text` or `hex`, optional
  `direct`), `joined`, `left`, `talking` (`session`, `talking`), `screen-share-started`,
  `chat-activity`, `mentioned`, `profile-card-opened`, `user-menu-requested` (each with a session
  number), `chat-trigger` (`sender`, `trigger`, `args`) and `chat-match` (`sender`, `text`), both
  with the chat options described [below](#chat-messages-in-scenarios), `panel-visibility` (`hidden`, `inline` or `expanded`), `dialog-closed` and `language`.
- `wait:` pauses for the given milliseconds, for example to let a timer fire.
- `expect:` checks the next output. It waits up to 1 s for a match after the previous one. Kinds:
  `tree` (`slot`, optional `text`, `nodes`), `cleared` (a slot), `message` (`to`: `channel` or a
  session number, `kind`, `text` or `hex`), `chat` (`text`), `notification` (`title`, `body`),
  `dialog` (`title`), `menu-entry` (`label`), `replaced` and `hidden` (see below), `log` (`contains`), `stopped` (optional `error`) and
  `nothing` (no output for a moment).

Slots are written `panel`, `settings`, `dialog`, or with an argument: `member-badge: 2`,
`profile-card: 2`, `chat-decoration: 2`, `chat-card: <id>`. Text fields match as substrings;
message payloads match exactly.

### Chat messages in scenarios

`chat-trigger` and `chat-match` steps stand in for a user's chat line. The trigger word must be one
of the manifest `triggers`, and for `chat-match` the first manifest pattern that matches `text`
sets the pattern index.

| Option | Meaning |
|---|---|
| `message: N` | Which chat message the line is. Defaults to the next unused number (1, 2, ...). The same number twice is the same message. `expect` steps use it to name the message. |
| `replay: true` | The line comes from stored history, so `chat.is-replay` is true. As in the real host, only plugins that import `is-replay` get it. |
| `own: true` | (`chat-match` only) Your plugin's own `[Name] ` line replayed from history, delivered with pattern index 255. Needs `chat.send` in the granted permissions; the step is refused without it. `sender` is not used. |

Two more `expect` kinds check what the plugin did to a message:

- `replaced: {message: N, card: ID}` the plugin called `chat.replace` for message N (`card` is
  optional: any card matches when left out).
- `hidden: {message: N}` the plugin called `chat.hide` for message N.

```yaml
steps:
  - event: { chat-trigger: { sender: 2, trigger: "!poll", args: "2", message: 4 } }
  - expect: { hidden: { message: 4 } }
  # The same line again, now from history: nothing is counted twice
  - event: { chat-trigger: { sender: 2, trigger: "!poll", args: "2", message: 4, replay: true } }
  - expect: { hidden: { message: 4 } }
  - event: { chat-match: { text: "[Poll] Lunch?", own: true, message: 5, replay: true } }
  - expect: { replaced: { message: 5, card: poll } }
```

### Preview the UI

`mumcord-plugin preview` renders the UI your plugin shows into one static HTML page, so you can
look at it without starting Mumcord:

```sh
mumcord-plugin preview dist/my-plugin-0.1.0.mcplugin --scenario scenarios/hello.yaml --out preview.html
```

`--scenario` plays the events and waits of a scenario first (its `expect` steps are not checked);
without it, you see the UI right after start. `--out` defaults to `preview.html`. The page follows
your system's light or dark theme, loads nothing and contains no scripts. It approximates Mumcord's
components at the real slot sizes, so use it for layout checks and confirm the final look in
Mumcord itself. The command fails if the plugin shows no UI.

## 4. Load it in Mumcord

Switch on Developer mode in Mumcord, then load the project folder or package through the plugin
settings. Developer plugins:

- may use all permissions except the three official-only ones,
- are marked as developer plugins, and exchange messages only with other developer copies of the
  same id,
- show their log lines and call statistics in the plugin's diagnostics.

Test with two Mumcord clients (two copies on the same machine are fine) joined to the same local
test server. **Test only against a local Mumble server**, never a public one.

Things to try by hand:

- Install with only the required permissions, then turn each optional permission on and off.
- Alone in the channel (`NoRecipients` is normal), and with a second client.
- Light and dark theme, large text, reduced motion.
- Disable and enable the plugin; restart Mumcord; reconnect to the server.
- Update from a previous version (set `previous-version` handling).

## 5. A local test registry

To test the install and update flow, serve a registry on your machine:

```sh
mumcord-plugin keygen --unencrypted --out keys --name test     # test key only
mumcord-plugin serve-registry --packages dist --key keys/test.key
```

This serves your packages as the official registry on `http://127.0.0.1:<port>/v1/`, signed with the
test key. Point a development build of Mumcord at it with the registry mirror setting and
`MUMCORD_PLUGIN_REGISTRY_KEY=<public key line>`. Add `--repository <name>` to serve a third-party
repository instead (see [Third-party repositories](third-party-repositories.md)).

`serve-registry` can also produce what the client must refuse, which is useful to see how your
update flow behaves: `tampered-hash:<id>`, `lower-sequence`, `wrong-key`, `extra-permission:<id>`,
`revoke:<id>[@<ver>][:warn]` and `key-change[:rotate]`. Switch them on with `--scenario` or while
running with `POST /_ctl/scenario?set=a,b`, and publish new versions with `POST /_ctl/reload`. The
scenario table is in
[`plugin-registry-format.md`](https://github.com/Mumcord/mumcord/blob/main/docs/plugin-registry-format.md#local-test-registry).

Never use a real signing key with the test registry, and never commit any key.
