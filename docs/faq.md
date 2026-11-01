# FAQ

## Do I need a special server?

No. Plugins run inside Mumcord and work on any standard, unmodified Mumble server. Your server
admin does not need to install anything.

## What do my friends on the official Mumble client see?

A plain text line where it matters, for example a poll question with the options or a dice result,
and nothing else. Mumcord users with the same plugin see a card instead of that line.

## Do my friends need the plugin too?

To see cards, pets and live updates, yes: plugins talk to the same plugin of other Mumcord users in
your channel. Some plugins also work with everyone else, for example people without Poll can vote
by typing `!poll 1`, `!poll 2` … in the chat.

## Are plugins preinstalled?

No. You choose every plugin yourself in **Settings → Plugins → Browse**.

## Can a plugin read my chat or my files?

No. A plugin cannot read chat text, open files or go online on its own. See the
[safety model](safety.md) and [Permissions explained](permissions-explained.md).

## How do I stop all plugins at once?

Choose **Turn off all plugins** in **Settings → Plugins**, or run it from the command palette
(++ctrl+shift+p++).

## How do updates work?

Updates install by themselves when they need no new permissions. If an update asks for more, it
waits for your approval.

## Do plugins cost anything?

The official plugins are free and open source. Plugins in the official registry may not contain
ads, tracking, payments or loot boxes.

## Are plugins translated?

Yes, into all 24 Mumcord languages.

## What does "Unofficial" mean?

The plugin comes from a plugin repository that someone else runs, not from the official registry.
It still runs in the same sandbox, but it cannot use the permissions reserved for official plugins.

## Can I write my own plugin?

Yes. Plugins are written in Rust and compiled to WebAssembly. Start with
[Getting started](getting-started.md).
