# Plugin author docs

Everything you need to write, test and publish a Mumcord plugin. Licence of these docs and of
[`../template/`](../template/): Apache-2.0.

| Page | What it covers |
|---|---|
| [Getting started](getting-started.md) | Create, build and load your first plugin in five minutes |
| [Manifest and package](manifest.md) | `manifest.json`, translations, assets and the `.mcplugin` package |
| [API reference](api.md) | The `mumcord-plugin-sdk` modules, events and limits |
| [Permissions](permissions.md) | Every permission, its risk level and what it unlocks |
| [Building](building.md) | The `mumcord-plugin` CLI, targets and reproducible builds |
| [Testing](testing.md) | Unit tests, loading in Developer mode, a local test registry |
| [Publishing](publishing.md) | Signing, the pull request to the official registry, updates |
| [Third-party repositories](third-party-repositories.md) | Running your own plugin repository |

The exact registry file formats are specified in
[`plugin-registry-format.md`](https://github.com/Mumcord/mumcord/blob/main/docs/plugin-registry-format.md)
in the Mumcord repository.

## The model in one minute

- A plugin is a small WebAssembly component written in Rust. It runs in a strict sandbox with a
  memory limit and a time limit per call, and it can only use the host interfaces its granted
  permissions allow.
- A plugin never draws pixels. It describes its interface as a tree of built-in building blocks
  (text, buttons, meters, lists, inputs) and Mumcord renders it with the user's theme.
- Plugins talk to the same plugin of other Mumcord users in the channel through standard Mumble
  plugin data messages. No server changes are needed, and people on other Mumble clients are not
  affected.
- Users see the permission list before installing. Updates that ask for more permissions wait for
  approval.

## Licences

- The template, these docs and the `mumcord-plugin-sdk` crate are Apache-2.0: you may use them in
  a plugin under any OSI-approved licence.
- The source of the `mumcord-plugin` command line tool is Apache-2.0, but the **built binary links
  code from Mumcord's AGPL-3.0 crates** (the package and manifest checks are shared with the
  client). Treat the binary as AGPL-3.0 software. This does not affect the plugins you build with
  it.
- Official plugins in this repository are AGPL-3.0. Your own plugin may use any OSI-approved
  licence; the manifest `license` field must be an SPDX identifier.
