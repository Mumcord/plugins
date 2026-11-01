# Publishing

The official registry lives in this repository (`Mumcord/plugins`). Publishing means: sign your
package, and open a pull request that adds a small text file describing the version. Maintainers
review it, CI rebuilds your package from your source, and the publish workflow signs and releases the
registry. Nothing in the registry is a binary you uploaded.

## Rules

- OSI-approved licence, with a `LICENSE` file in the source and the package.
- A reproducible build from a public Git repository ([Building](building.md#reproducible-builds)).
- No ads, tracking, payments or loot boxes.
- Ask only for the permissions you need. `net.http`, `secrets` and `ui.webview` are reserved for
  official plugins; use a [third-party repository](third-party-repositories.md) for anything that
  needs them.
- Ids with the prefixes `mumcord-` and `core-` are reserved.

A new plugin, a new author key or a new permission needs one maintainer approval and green CI (see
[`policy/REVIEW.md`](../policy/REVIEW.md)). CI checks the schema, rebuilds the package and compares the
hash, verifies your signature, comments the permission differences and runs a load test and a
licence scan.

## 1. Create an author key

```sh
mumcord-plugin keygen --name my-author --out ~/keys
```

Choose a strong password. Keep the `.key` file private and backed up: it is your identity as an
author. Publish only the `.pub` file. Never commit a key or a password. If a key is lost or leaked,
tell the maintainers so it can be replaced and, if needed, revoked.

## 2. Build and sign

```sh
mumcord-plugin build --reproducible
mumcord-plugin sign dist/my-plugin-1.0.0.mcplugin --key ~/keys/my-author.key
mumcord-plugin check dist/my-plugin-1.0.0.mcplugin --key "$(cat ~/keys/my-author.pub)"
```

Your plugin source must be pushed to a public repository, and the commit you publish must contain the
`Cargo.lock`. The registry rebuilds from that commit.

## 3. Open the pull request

Fork and clone `Mumcord/plugins`, then from your plugin's source checkout:

```sh
mumcord-plugin publish-pr dist/my-plugin-1.0.0.mcplugin \
  --plugins-repo ../plugins --author-key ~/keys/my-author.pub --open
```

This writes `plugins/my-plugin/versions/1.0.0.toml` (version, size, sha256, permissions, your
signature and the source commit), creates `plugins/my-plugin/plugin.toml` for a new plugin (name,
summary, categories, authors and `author_keys`) and, with `--open`, commits, pushes and opens the
pull request (needs `git` and `gh`). Without `--open` it only writes the files. The file formats are
described in the [registry format](https://github.com/Mumcord/mumcord/blob/main/docs/plugin-registry-format.md#registry-source).

Add a description (`README.md`), an `icon-128.png` and optional screenshots with alt text to
`plugins/my-plugin/` so the browse view looks good. Paths in `plugin.toml` are relative to that
folder.

## 4. After the merge

The publish workflow rebuilds the package, attaches it to the release `my-plugin-v1.0.0`, signs
`index.json` and updates the website data at `https://mumcord.github.io/plugins/`. Users see the
plugin in Settings > Plugins > Browse.

## Updates

Publish a new version the same way with a higher version number.

- Users get updates automatically when the new version asks for **no new permissions**. New
  permissions wait for the user's approval, so keep them stable and make new ones optional.
- Old version folders stay on disk for rollback.
- Migrate your own data in `start`, using `StartInfo.previous_version`.
- Never reuse a version number. A published version is immutable.

## Problems after release

If a version is dangerous, open a pull request or contact the maintainers to add it to
`revoked.toml`: severity `warn` shows users a warning, `disable` turns the version off everywhere.
Publish a fixed version afterwards.
