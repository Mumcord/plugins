# My plugin

A Mumcord plugin.

## Build

```sh
rustup target add wasm32-unknown-unknown
cargo generate-lockfile         # once; commit Cargo.lock
mumcord-plugin build            # writes dist/my-plugin-0.1.0.mcplugin
mumcord-plugin check dist/my-plugin-0.1.0.mcplugin
```

Commit `Cargo.lock`: `mumcord-plugin build --reproducible` and `mumcord-plugin test` need it.

## Try it in Mumcord

Switch on Developer mode, then use Developer → load on the plugins page. It needs an unpacked
folder (`manifest.json`, `plugin.wasm` and `i18n/en.json`), not a `.mcplugin` package.

## Licence

Add a `LICENSE` file with an OSI-approved licence before publishing.
