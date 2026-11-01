# Manifest and package

## Package layout

A package is a zip file with the extension `.mcplugin`. `mumcord-plugin build` creates it.

| File | Required | Notes |
|---|---|---|
| `manifest.json` | yes | At most 16 KiB. See below. |
| `plugin.wasm` | yes | A WebAssembly component for the world `mumcord:plugin@1.0.0`. |
| `i18n/en.json` | yes | English texts. Other languages are `i18n/<tag>.json` (BCP 47, for example `de`). At most 64 KiB each. |

**Translations.** Third-party plugins only need `i18n/en.json`. Official plugins (published from the
Mumcord plugins repository) must ship `i18n/<tag>.json` for all 24 Mumcord languages (`bg cs da de
el en es et fi fr ga hr hu it lt lv mt nl pl pt ro sk sl sv`), each with exactly the keys of
`en.json`. Plural keys may use different `_zero`/`_one`/`_two`/`_few`/`_many` variants per language,
but `_other` is required everywhere. CI fails and lists missing languages and keys.
| `assets/**` | no | Images for `ui::image`, the plugin icon. At most 512 KiB each. SVGs are sanitised on install. |
| `README.md`, `LICENSE` | official plugins | Recommended for every plugin. |

Limits: the package is at most 5 MiB, unpacks to at most 8 MiB and holds at most 200 files. Official plugins may be larger because they can bundle a web app: at most 64 MiB packed, 96 MiB unpacked and 10,000 files, with each file under `web/` at most 16 MiB. Files under `web/` may be `.html`, `.css`, `.js`, `.json`, `.wasm`, `.woff2`, `.png`, `.webp` or `.svg`.
Because SVGs are sanitised on install, the installed bytes can differ from the package bytes.

## `manifest.json`

```json
{
  "manifest": 1,
  "id": "my-plugin",
  "name": "My plugin",
  "description": "Wave at the channel.",
  "version": "0.1.0",
  "api": "^1.0",
  "authors": ["Your name"],
  "license": "Apache-2.0",
  "categories": ["fun"],
  "permissions": {
    "required": ["ui.panel"],
    "optional": [{ "id": "peers.messages", "suggested": true }]
  }
}
```

| Field | Required | Meaning |
|---|---|---|
| `manifest` | yes | Format number. Currently `1`. |
| `id` | yes | Unique, permanent id. Must match the registry folder name. |
| `name`, `description` | yes | Fallback texts. Translated by `manifest.name` and `manifest.description` in `i18n/*.json` (name at most 32 characters, description at most 140). |
| `version` | yes | Semantic version. A new release needs a higher version. |
| `api` | yes | Plugin API version range, for example `^1.0`. The current API is `1.0.0`. |
| `mumcord` | no | Version range of Mumcord the plugin needs. |
| `authors` | yes | List of names. |
| `license` | yes | SPDX identifier of an OSI-approved licence. |
| `categories` | yes | One or more of `fun`, `games`, `chat`, `voice`, `accessibility`, `looks`, `tools`. |
| `permissions.required` | yes | Permissions the user must accept to install. |
| `permissions.optional` | no | Permissions the user can switch off. `suggested: true` starts them switched on in the install prompt. The plugin must work without them. |
| `commands` | no | Slash commands: `{ "name", "description", "usage" }`. Needs the `commands` permission. |
| `triggers` | no | Chat trigger words such as `["!poll"]`. A channel message that starts with one arrives as a `ChatTrigger` event. Needs `chat.triggers`. |
| `chat_patterns` | no | At most 2 entries `{ "pattern", "description" }`. A pattern is at most 256 characters and must start with `^` or `\A`. A channel message whose plain text matches arrives as a `ChatMatch` event. Needs `chat.patterns`. Translate the description with the key `chat_patterns.<n>.description`. |
| `messaging.max_payload` | no | Largest peer message the plugin sends, at most 997 bytes (default 997). |
| `messaging.default_visibility` | no | `everyone`, `friends` or `nobody`: preselected in the install prompt. |
| `memory_mib` | no | Memory limit, at most 32 MiB. |
| `storage_quota_kib` | no | Storage quota, default 256 KiB, at most 1024 KiB. |
| `net` | no | Only for official plugins with `net.http`. |
| `icon` | no | Path of an image in `assets/`. |

Unknown fields and unknown permission ids are errors. Ids that start with `mumcord-` or `core-` are
reserved.

## Translations

Every user-visible string of the manifest is looked up in the active language and falls back to
English. Keep keys stable between versions. Strings you draw with `ui::text` are yours to
translate: Mumcord sends the current language (a BCP 47 tag such as `de`) as the `language-changed` event right after `start` and whenever it changes.
