# Review policy

## What needs review
A new plugin, a new author key or a new permission needs one maintainer approval and green CI.
Other changes (a new version with the same permissions and keys, media, texts) need green CI and
are merged by a maintainer as usual.

## Rules
- OSI-approved licence
- Reproducible build
- No ads, tracking, payments or loot boxes
- **No remote code:** a plugin must not load scripts, modules, WebAssembly or styles from a
  website, and must not fetch code at run time to execute it (no `eval`, no dynamic `import()` of
  a URL, no `<script src="https://…">`). All web content a plugin shows (HTML, scripts, styles,
  images, fonts) ships inside the package, so what was reviewed and signed is what runs. Data
  fetched from a declared origin (JSON, images, text) is fine; code is not.

## Website review
A plugin that declares websites in `net.origins` (API 1.1) gets a manual review of every origin
before a maintainer approves. The permission-diff comment lists the origins of each new version.
New or changed origins need the same one maintainer approval as a new permission.

- **Declared origins:** each origin is an `https://` host the plugin really contacts. A wildcard,
  an IP address, `localhost` or a host the plugin never uses is refused. At most 8 origins.
- **Purpose texts:** every origin has a purpose text in `en.json` that tells a player in plain
  words what is sent to that website and why (for example "Looks up item prices"). The text must
  match what the code does. Missing, vague or misleading texts are refused.
- **Origins are the plugin's own or justified:** the website belongs to the plugin author or
  project, or is a public service the plugin needs for its stated function. The review notes why
  for each third-party origin (who runs it, what is sent, whether it works without it). Origins
  that only serve ads, tracking, analytics or payments are refused, as is any origin that serves
  code (see "No remote code").
- **What is sent:** no player names, chat contents or audio go to a website unless the plugin's
  function is exactly that and the purpose text says so.

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
- **Declared API:** the `api` of the version file must be the one in the package manifest, and the
  package must import only the WIT interfaces of that API (`mumcord-plugin check --official`). A
  `min_mumcord` below the first Mumcord release with the API (`0.6` for API 1.0, `0.7` for API
  1.1) is refused by `site-data`. The generated `v1` index must list API 1.0 versions only.
- **Author signature:** the `signature` of the version file must verify against one of the
  plugin's `author_keys`, and the package must pass `mumcord-plugin check --official`.
- **Permission-diff comment:** CI posts one comment on the pull request listing new plugins, new
  author keys and the permissions each new version adds or removes compared with the previous
  published version. It ends with "Needs one maintainer approval" when the review rule above
  applies. The same text is in the job summary.
- **Official plugin translations:** every official plugin (the folders `poll/`, `dice/`,
  `echolings/`, `mumbot-music/` and any registry plugin whose `[source]` is this repository) must
  ship `i18n/<tag>.json` for all 24 Mumcord languages: `bg cs da de el en es et fi fr ga hr hu it
  lt lv mt nl pl pt ro sk sl sv`. Each file has exactly the keys of `en.json`. Plural variants
  (`_zero`, `_one`, `_two`, `_few`, `_many`) may differ per language, but `_other` must exist for
  every plural key. The job lists the missing languages and keys. Third-party plugins only need
  `en.json`. Run it locally with `python3 .github/scripts/registry.py i18n`.
- **Mock-host load test:** the package is started in the mock host of `mumcord-plugin test`.
- **Licence scan:** the manifest `license` must be on the OSI allow-list in
  `.github/scripts/registry.py`, the source folder must have a `LICENSE` file, and the Rust
  dependencies are checked with `cargo deny check licenses` (`.github/deny.toml`).

## Publishing
After a merge to `main`, `.github/workflows/publish.yml`:

1. rebuilds and checks every version that has no GitHub release `<id>-v<ver>` yet and attaches
   `<id>-<ver>.mcplugin` and its `.minisig` to that release;
2. generates `v1/index.json` (format 1, API 1.0 versions only, read by Mumcord 0.6.x),
   `v1.1/index.json` (format 2, all versions, read by Mumcord 0.7.0 and newer), `v1/revoked.json`
   (the kill switch for both), `v1/permissions.json`, `v1.1/permissions.json` and the media with
   `mumcord-plugin site-data`, using a `sequence` above both published ones;
3. signs both `index.json` files and `revoked.json` in the protected `publish` environment and
   checks the signatures against `.github/registry-key.pub`;
4. deploys GitHub Pages: the registry at `https://mumcord.github.io/plugins/v1/` and
   `https://mumcord.github.io/plugins/v1.1/` and the
   documentation site built from `docs/` at `https://mumcord.github.io/plugins/docs/`;
5. sends the `repository_dispatch` event `plugins-published` to the repository in the variable
   `SITE_DISPATCH_REPO` with the token `SITE_DISPATCH_TOKEN` (skipped while either is unset).

To revoke a plugin, add an entry to `revoked.toml`; the merge publishes a new signed
`revoked.json`. A run can also be started by hand from the Actions tab (`workflow_dispatch`).
