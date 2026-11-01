# Third-party repositories

Anyone can run a plugin repository: a static website with a signed `index.json`. Users add it in
Settings > Plugins > Repositories by typing the address of the index. Plugins from a repository are
labelled "Unofficial · <repository name>" and are not reviewed by Mumcord.

Use a third-party repository when your plugin cannot go in the official registry, for example when it
is private to a community or needs a different review process.

## What a repository must serve

| File | Notes |
|---|---|
| `index.json` | The address users type. UTF-8 JSON, at most 2 MiB and 500 plugins. |
| `index.json.minisig` | Detached minisign signature of the index, by the repository key. |
| `<id>-<ver>.mcplugin` | Any URL listed in the index. |
| `<id>-<ver>.mcplugin.minisig` | Detached signature by an author key listed in `author_keys`. |

Use `https`. Plain `http` is accepted only for localhost in Developer mode. The complete field list
for the index is in the
[registry format](https://github.com/Mumcord/mumcord/blob/main/docs/plugin-registry-format.md#indexjson);
the important parts are:

```json
{
  "format": 1,
  "sequence": 12,
  "repository": {
    "name": "Foo Repo",
    "homepage": "https://foo.example",
    "contact": "plugins@foo.example",
    "public_key": "RW..."
  },
  "plugins": [ { "id": "my-plugin", "name": { "en": "My plugin" }, "versions": [ ... ] } ]
}
```

- `sequence` must rise with every change. Mumcord refuses an index with a lower sequence (rollback
  protection).
- Each version lists `url`, `size`, `sha256` and the `permissions` the manifest asks for. Mumcord
  checks size, hash, author signature and manifest, in that order.
- You cannot list official plugin ids or ids starting with `mumcord-` or `core-`.
- Packages from a repository cannot use the official-only permissions `net.http`, `secrets` and
  `ui.webview`.

## Keys

Create two key pairs with `mumcord-plugin keygen`: a **repository key** (signs the index) and an
**author key** per plugin author (signs packages). Keep both secret keys offline and password
protected. Sign with `mumcord-plugin sign <file> --key <secret key>`.

Users pin the repository key the first time they add the repository and see its fingerprint (they
can also type the expected key). If a later index is signed by another key, Mumcord pauses the
repository and asks the user. To change the key without bothering users, add a `key_rotation` block
to the index: the new key's line signed by the old key. The steps are in the
[registry format](https://github.com/Mumcord/mumcord/blob/main/docs/plugin-registry-format.md#key-rotation).

## Try it locally

```sh
mumcord-plugin keygen --unencrypted --out keys --name repo-test
mumcord-plugin serve-registry --packages dist --key keys/repo-test.key \
  --repository "My repo"
```

Add `http://127.0.0.1:<port>/v1/index.json` in Settings > Plugins > Repositories (Developer mode
allows `http` for localhost).

## Responsibility

Mumcord shows users a clear warning that third-party plugins are not reviewed. You are responsible
for what you publish: keep your keys safe, document your plugins' permissions honestly and fix or pull
bad versions quickly. A revoked id, version or author key in the official kill switch (`revoked.json`)
turns that plugin off for every source.
