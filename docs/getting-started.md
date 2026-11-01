# Getting started

## You need

- Rust (stable, 1.90 or newer) with the WebAssembly target:
  `rustup target add wasm32-unknown-unknown`
- The `mumcord-plugin` command line tool (see [Building](building.md#installing-the-cli))
- Mumcord with **Developer mode** switched on (Settings > Advanced & Developer)

## Create, build, check

```sh
mumcord-plugin new my-plugin
cd my-plugin
cargo generate-lockfile        # needed once; commit Cargo.lock
mumcord-plugin build           # writes dist/my-plugin-0.1.0.mcplugin and prints its sha256
mumcord-plugin check dist/my-plugin-0.1.0.mcplugin
```

`new` copies the same files as the [`template/`](../template/) folder of this repository. The
result is a "Hello" plugin: a panel with a **Wave** button that counts waves in plugin storage and
sends a wave message to other users of the plugin in your channel.

Ids are 3 to 32 characters of `a-z`, `0-9` and `-`, start with a letter, and may not start with
`mumcord-` or `core-`.

## Try it in Mumcord

Use Developer → load on the plugins page. It needs an **unpacked folder** (`manifest.json`,
`plugin.wasm` and `i18n/en.json`), not a `.mcplugin` package. A plugin loaded this way is marked as a developer plugin and only exchanges messages with other
developer copies of the same id. See [Testing](testing.md) for loading packages and for serving a
local registry.

## Next steps

1. Rename the plugin: edit `name` and `id` in `manifest.json` and the package name in
   `Cargo.toml` (the id and the crate name must match).
2. Ask only for the permissions you use ([Permissions](permissions.md)).
3. Write the logic against the [API](api.md).
4. Add a `LICENSE` file before you publish.
