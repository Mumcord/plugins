# Building

## Installing the CLI

`mumcord-plugin` is built from `tools/mumcord-plugin` in the
[Mumcord repository](https://github.com/Mumcord/mumcord):

```sh
cargo install --git https://github.com/Mumcord/mumcord mumcord-plugin
```

The CLI source is Apache-2.0, but the built binary links code from Mumcord's AGPL-3.0 crates (it
runs the same package and manifest checks as the client). Treat the binary as AGPL-3.0 software; this
does not change the licence of the plugins you build with it.

## Commands

| Command | Purpose |
|---|---|
| `new <id> [--dir D] [--sdk-path P]` | Create a project from the built-in template. `--sdk-path` uses a local checkout of the SDK. |
| `build [dir] [--reproducible] [--out D]` | Compile and pack `dist/<id>-<version>.mcplugin`. Prints the sha256. |
| `check <package> [--official] [--key K]` | Run the checks Mumcord runs before installing. With `--key`, also checks `<package>.minisig`. |
| `keygen [--out D] [--name N] [--unencrypted]` | Create a minisign key pair. Protect the secret key with a password. |
| `sign <file> --key K` | Write `<file>.minisig`. |
| `serve-registry` | A local test registry. See [Testing](testing.md). |
| `publish-pr` | Add a version to a checkout of the registry and open the pull request. See [Publishing](publishing.md). |

The key password is read from `MUMCORD_PLUGIN_KEY_PASSWORD` or asked for on the terminal. Never put
a key or a password into a repository.

## The build

Plugins are `cdylib` crates built for **`wasm32-unknown-unknown`**:

```sh
rustup target add wasm32-unknown-unknown
mumcord-plugin build
```

`build` runs `cargo build --release --target wasm32-unknown-unknown`, turns the resulting module into
a WebAssembly component with `wit-component` and packs it with `manifest.json`, `i18n/` and
`assets/` into the `.mcplugin` zip. The target is `wasm32-unknown-unknown` and not
`wasm32-wasip2` because the wasip2 standard library imports `wasi:*` interfaces that the Mumcord
runtime refuses. A plugin may only import `mumcord:plugin/*@1.0.0`, so avoid crates that need
system APIs (files, sockets, threads, wall-clock time).

Keep the plugin small: the package is limited to 5 MiB, and memory to 32 MiB at runtime. The
template's release profile (`opt-level = "s"`, `lto`, `codegen-units = 1`) is a good start.

## Reproducible builds

```sh
cargo generate-lockfile    # once, commit Cargo.lock
mumcord-plugin build --reproducible
```

The same source with the same `Cargo.lock` gives the same package bytes. The official registry
relies on this: it rebuilds your package from the commit you published and compares the sha256, so
nobody has to trust a binary. A reproducible build needs a committed `Cargo.lock` and the build must
not depend on the path it runs from (the CLI maps build paths to fixed names).

Pin your toolchain with a `rust-toolchain.toml` so that CI and your machine agree.
