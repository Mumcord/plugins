# Whiteboard

Official Mumcord plugin: one shared, endless board for everyone in a voice channel. Sketch,
add sticky notes, draw arrows and boxes, present with a laser pointer, and export the board as
PNG, SVG, PDF or a board file (Mumcord/mumcord#397).

## Using it

- **Start:** pick **Start whiteboard** in the Apps menu, or type `/whiteboard` in the chat box.
  The board opens on the stage (the media area) and an invite card appears in the chat.
- **Join:** everyone else in the channel sees the invite card with **Join** (and the stage's own
  Join button). People who join late get the whole board from the others.
- **Draw:** select and move, pen, highlighter, eraser, shapes, arrow, sticky note, text and laser
  pointer, each with a shortcut letter. Undo and redo only touch your own changes.
- **Together:** everyone's cursor shows their name, colour and shape; with a relay you see
  strokes grow live, otherwise they arrive when the pen lifts and cursors show "2 s ago".
- **Present:** the presenter's view is followed by the others, the laser leaves a fading trail,
  and anyone can stop following. Esc ends presenting.
- **Side panel:** while something else (say a screen share) is on the stage, the side panel shows
  a live mini view with a quick sticky note box and "Open on stage".
- **Export:** PNG (1×, 2×, 4×, optionally transparent), SVG, PDF or a board file, for the whole
  board, what you see, or the selection. Mumcord opens its own save window; author names are
  only included when you switch them on.
- **Post to chat:** posts a picture of the board, the view or the selection into the channel
  through Mumcord's normal image sending, so standard Mumble clients see it inline too. It tries
  2× first and 1× when that is too big; above the server's limit Mumcord offers it as a file.
- **Board settings:** the host decides who may draw (everyone, selected people, only the host),
  whether viewers may use the laser while the host presents, can clear the board (with a
  confirmation and 30 seconds of Undo), remove someone's strokes for everyone and hand over the
  host role. Anyone can hide a person's strokes just for themselves and pick their paper.
- **Describe board:** a short spoken summary of what is on the board, for screen readers.
- **Standard Mumble clients** cannot see the board. In Board settings the host can post one
  start line and one summary line as plain chat text (only on a click). Mumcord users with the
  plugin see the invite card in place of those lines.

### Boards live on your devices

Every participant keeps a copy of the board on their own device; it is saved a moment after each
change ("Saved on this device"). Nothing is stored on a server or in a cloud. When everyone has
left, the board is gone from the channel but still on every device that had it open: whoever
starts the whiteboard next sees **Continue "…"** and can open it again, which shares it with the
channel again. People who bring an older copy of the same board merge it automatically.
**Manage saved boards** shows how much of the plugin's 1 MiB storage is used and deletes boards
from this device (only after asking). Export what you want to keep for good.

## Permissions

| Permission | | Why |
|---|---|---|
| `ui.stage` | required | The board in the media area |
| `ui.webview` | required | The board is drawn by the plugin's own web page (`web/board/`, no external addresses) |
| `ui.app-menu` | required | "Start whiteboard" in the Apps menu |
| `ui.dialog` | required | Board settings, Saved boards and Describe board dialogs (Export, Post to chat and Clear are drawn inside the board) |
| `peers.messages` | required | Board changes, joins and snapshots over Mumble plugin data |
| `events.presence` | required | Who is in the channel, joins, leaves and the host hand-over |
| `random.bytes` | required | Random board ids and per-board author keys |
| `peers.relay` | optional | Live strokes, smooth cursors, laser and fast loading over the community relay |
| `ui.side-panel` | optional | The mini view next to a screen share |
| `events.voice` | optional | A speaking ring on people's cursors |
| `chat.send` + `chat.supersedes` | optional | The two plain-text lines for standard Mumble clients, hidden for Mumcord users |
| `ui.chat-card` | optional | The invite card with Join |
| `commands` | optional | `/whiteboard` |
| `files.save` | optional | Saving exports through Mumcord's own save window |
| `chat.post-image` | optional | Post to chat |

Without an optional permission the matching feature is simply missing: no relay means the slower
Mumble connection, no side panel means no mini view, no chat lines, no command, no card.

## How it works

The board is a CRDT (`src/model.rs`): every object field and board setting is a last-writer-wins
register, so all copies end up identical whatever order changes arrive in. `src/sync.rs` keeps
the copies in sync, `src/codec.rs` is the strict wire format, `src/store.rs` saves boards,
`src/export/` writes SVG and PDF, and `src/app.rs` connects all of it to Mumcord's events. The web
page holds no authority: every edit goes to the plugin as a message and comes back as the updated
objects. Who may draw is checked by every participant's Mumcord when a change arrives; a
modified client could ignore the rules, but nobody else would show its changes.

### Wire format

Every payload is a [Bovnar](https://github.com/sothis/bovnar) document made with the SDK's
`bovnar` module: layout version `1` and a message with the kind `k` and the binary body `d`, for
example `.v=1;.msg={.d="Te&q4…";.k=6;};`. Bovnar has no byte-string type, so the compact binary
body travels as base-85 text (Z85 alphabet, 4 bytes become 5 characters). Receivers ignore fields
they do not know. On `peers.messages` the kind also travels as Mumble's plugin data type and the
document is at most 997 bytes; on the relay it is at most 60 KiB. In the body, integers are
LEB128 varints, coordinates are quarter pixels, author keys 4 bytes, board ids 16 bytes.

| Kind | Name | Content |
|---|---|---|
| 0x01 | HELLO | board, author, name and identity, version vector, host flag, joined-at |
| 0x02 | OPS | board, author, a batch of changes (deflated when smaller), last cursor |
| 0x03 | NEED | board, author, missing sequence range |
| 0x04 | DIGEST | board, hash of the version vector, total changes |
| 0x05 | SNAP | board, part / parts, version vector (part 0), deflated state |
| 0x06 | BOARD | board, title, host, started-at (announce and re-share) |
| 0x10–0x13 | CURSOR, LIVE, LASER, VIEW | relay only (VIEW also once over Mumble) |
| 0x20 | LINE | which plain-text line was posted and its chat line hash |

The plugin and its page talk in JSON (`src/bridge.rs`, `web/board/bridge.js`); every page message
is validated (type, ranges, sizes) before it is used.

### Limits

- 5,000 objects and about 4 MiB per board; 2,000 points per stroke; 500 characters per note or
  text, 200 per description, 64 per board title.
- Without a relay: one board message per 2 seconds (bursts of 3, and short game bursts for
  snapshots and the laser); with a relay changes go out several times a second within
  256 kbit/s.
- Flood caps per sender: 1,200 messages and 1,000 changes per 10 seconds, the rest is dropped.
- Saved boards share the plugin's 1 MiB storage; posted pictures and exports are made only after
  a click on the Save or Post button in the board's own Export dialog. Mumcord counts a genuine
  click inside the plugin's page as the user's gesture (spec FR-9.45); a page that sends the
  message without a click gets `not-permitted` from Mumcord.
- Up to three web views per plugin (stage, side panel, dialog): the stage page and the side
  panel's mini view tag every message with their place (`p` from the page, `to` to the page).

## Privacy

- No network access at all: no `net.*` permission, no external fonts, scripts or images.
- With a relay the board is end-to-end encrypted; without one it travels as Mumble plugin data
  through the server, which its operator can read, like text chat.
- Names are Mumble names; exports leave author names out unless you choose them. Cursors are
  only shared while your pointer is over the board.
- Saving and posting go through Mumcord's own windows: the plugin hands over bytes and never sees
  a folder, a path or the chat.

## Languages

All texts live in `i18n/<tag>.json`; the web page's texts are in the same file under the `page.`
prefix and reach the page with its `init` message. A native test checks that `en.json` holds every
page key from `web/board/strings-keys.json`.

## Build and test

```sh
cargo test
node --test web-tests/
mumcord-plugin build
mumcord-plugin check dist/whiteboard-1.0.0.mcplugin
mumcord-plugin test dist/whiteboard-1.0.0.mcplugin scenarios/*.yaml
```

The glue flows (start, join, sync between two users, page validation, dialogs) run as native
tests against a fake host in `src/tests.rs`; the scenarios cover what the mock host can drive.

## Licence

AGPL-3.0-or-later, see [LICENSE](LICENSE). The hand-written Caveat font in `web/board/fonts/` is
under the SIL Open Font License 1.1; its licence text is at the end of `LICENSE`.
