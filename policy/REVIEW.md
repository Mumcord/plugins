# Review policy

## What needs review
A new plugin, a new author key or a new permission needs one maintainer approval and green CI.
Other changes (a new version with the same permissions and keys, media, texts) need green CI and
are merged by a maintainer as usual.

## Rules
- OSI-approved licence
- Reproducible build
- No ads, tracking, payments or loot boxes

## CI checks
The `check` job (`.github/workflows/check.yml`) runs on every pull request and must be green
before merging. It checks every version file the pull request adds or changes.

- **Schema validation:** `plugin.toml`, `versions/<ver>.toml` and `revoked.toml` are validated
  against `schemas/`, then `mumcord-plugin site-data` generates the index the way the publish
  workflow does and refuses any entry Mumcord would ignore.
- **Reproducible rebuild:** the source named in `[source]` (repository, full commit hash, folder)
  is fetched and built with `mumcord-plugin build --reproducible`. Size and SHA-256 must equal the
  version file. Pin the Rust toolchain with a `rust-toolchain.toml` in the plugin source, otherwise
  CI builds with the current stable Rust and the hash may differ from the author's build.
- **Author signature:** the `signature` of the version file must verify against one of the
  plugin's `author_keys`, and the package must pass `mumcord-plugin check --official`.
- **Permission-diff comment:** CI posts one comment on the pull request listing new plugins, new
  author keys and the permissions each new version adds or removes compared with the previous
  published version. It ends with "Needs one maintainer approval" when the review rule above
  applies. The same text is in the job summary.
- **Mock-host load test:** the package is started in the mock host of `mumcord-plugin test`.
- **Licence scan:** the manifest `license` must be on the OSI allow-list in
  `.github/scripts/registry.py`, the source folder must have a `LICENSE` file, and the Rust
  dependencies are checked with `cargo deny check licenses` (`.github/deny.toml`).

## Publishing
After a merge to `main`, `.github/workflows/publish.yml`:

1. rebuilds and checks every version that has no GitHub release `<id>-v<ver>` yet and attaches
   `<id>-<ver>.mcplugin` and its `.minisig` to that release;
2. generates `v1/index.json`, `v1/revoked.json`, `v1/permissions.json` and the media with
   `mumcord-plugin site-data`, using a `sequence` above the published one;
3. signs `index.json` and `revoked.json` in the protected `publish` environment and checks the
   signatures against `.github/registry-key.pub`;
4. deploys GitHub Pages (`https://mumcord.github.io/plugins/v1/`);
5. sends the `repository_dispatch` event `plugins-published` to the repository in the variable
   `SITE_DISPATCH_REPO` with the token `SITE_DISPATCH_TOKEN` (skipped while either is unset).

To revoke a plugin, add an entry to `revoked.toml`; the merge publishes a new signed
`revoked.json`. A run can also be started by hand from the Actions tab (`workflow_dispatch`).
