# Safety model

Plugins should make voice chat more fun without putting you, your friends or your server at risk.
This is how Mumcord makes sure of that.

## A strict sandbox

- Every plugin is a small **WebAssembly** program that runs in a strict sandbox inside Mumcord,
  with a memory limit and a time limit per call.
- It has **no raw network access and no direct file access**. It can only use what its granted
  permissions allow.
- A plugin **never draws its own pixels**. It describes its interface with Mumcord's built-in
  building blocks (text, buttons, meters, lists, inputs), and Mumcord draws them in your theme.

## Permissions you can see

- The permission list is shown **before you install**, with a colour for how sensitive each one is.
- **Optional permissions** can be turned off at any time.
- An **update that needs more permissions waits** for your approval.
- Some powerful permissions are **only available to official plugins**.

More in [Permissions explained](permissions-explained.md).

## Signed and checked

- The registry index and every package are **signed** (minisign).
- Before installing, Mumcord checks the size, the hash, the author's signature and the plugin's
  manifest. Anything that does not match is refused.
- A **kill switch** can turn off a bad plugin version for everyone.

## Reviewed in the open

Every plugin in the official registry is added through a public pull request. A new plugin, a new
author key or a new permission needs one maintainer approval and green automated checks:

- a reproducible rebuild from the published source, so the package matches the code,
- the author's signature,
- a permission comparison with the previous version,
- a load test in a mock host,
- a licence scan.

Plugins must use an OSI-approved open source licence and contain **no ads, tracking, payments or
loot boxes**. Read the full [review policy](../policy/REVIEW.md).

## No server changes

Plugins stay entirely on your side. They talk to the same plugin of other Mumcord users through
standard Mumble plugin data messages, so they work on any unmodified Mumble server. People on the
official Mumble client are not affected; they see a plain text line where it matters.

## You are in control

- Nothing is preinstalled.
- **Turn off all plugins** in **Settings → Plugins**, or from the command palette
  (++ctrl+shift+p++), stops every plugin at once.
- Plugins from other repositories are clearly marked **Unofficial**.
