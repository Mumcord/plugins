# Whiteboard

<img class="mp-plugin-icon" src="../../whiteboard/assets/icon-128.png" alt="" width="96" height="96">

One shared, endless board for everyone in your voice channel. Sketch, add sticky notes, draw
arrows and boxes, present with a laser pointer and export the result.

## Start a whiteboard

- Pick **Start whiteboard** in the Apps menu, or type `/whiteboard` in the chat box.
- The board opens on the stage (the media area) and an invite card appears in the chat.
- Everyone else in the channel clicks **Join** on the card (or on the stage). People who join late
  get the whole board from the others.

## Tools

| Tool | Key |
|---|---|
| Select and move | `V` |
| Pen | `P` |
| Highlighter | `H` |
| Eraser | `E` |
| Shapes (rectangle, ellipse, diamond, line; press again for the next one) | `S` |
| Arrow | `A` |
| Sticky note | `N` |
| Text | `T` |
| Laser pointer | `L` |

- Hold `Space` or drag with the middle mouse button to move around the board.
- Zoom from 10 % to 400 %, or fit the whole board to the window.
- Undo and redo only touch your own changes.

## Together

Everyone's cursor shows their name, colour and shape, and you see strokes grow live while someone
draws. Cursors are only shared while your pointer is over the board.

## Present

**Present** shares your view: the others follow it, and your laser leaves a fading trail. Anyone
can **Stop following** at any time, and `Esc` ends presenting.

## Save, export and share

- **Saved on your device:** every participant keeps a copy that is saved a moment after each
  change. Nothing is stored on a server or in the cloud.
- **Saved boards:** when everyone has left, the board is still on every device that had it open.
  Whoever starts the whiteboard next sees **Continue "…"** and can share it with the channel again.
  **Manage saved boards** shows your storage and deletes boards from this device.
- **Export:** PNG (1×, 2×, 4×, optionally transparent), SVG, PDF or a board file, for the whole
  board, what you see or your selection. Mumcord opens its own save window. Names of who drew what
  are left out unless you switch them on.
- **Post to chat:** posts a picture of the board, your view or your selection into the channel.
  People on the standard Mumble client see it inline too. If it is too big for the server, Mumcord
  offers it as a file instead.

## Host controls

Whoever starts the board is the host. Open **Board settings** to:

- Choose **who may draw**: everyone, selected people or only the host. A selected person keeps the
  permission while on the board, but has to be allowed again after leaving and rejoining.
- Let viewers use the **laser** while you present.
- **Clear the board** (you confirm first, and **Undo** works for 30 seconds).
- **Remove** someone's strokes for everyone, or **hand over** the host role.

Anyone can **hide** a person's strokes just for themselves and pick their own paper (auto, light,
dark, dots, grid or plain).

## Side panel

While something else is on the stage, for example a screen share, the side panel shows a live mini
view of the board with a quick sticky note box and **Open on stage**.

## Keyboard and screen readers

Everything can be done without a mouse: press `F1` for the list of shortcuts, use the keyboard
cursor to draw and place things, and open the board outline to move through the objects.
**Describe board** reads out a short summary of what is on the board.

## Good to know

- The board travels through a relay on the server that allows plugin data (mumpeer 0.2.1 and later
  allows it by default) or through direct connections. Without one, the board shows the reason and
  cannot start.
- The board is encrypted on the relay and nothing is stored on the server.
- Standard Mumble clients cannot see the board. The host can post one start line and one summary
  line as plain chat text, but only when they click for it. Mumcord users with Whiteboard see the
  invite card instead.
- A board holds up to 5,000 objects.

## Permissions

| Permission | Why |
|---|---|
| Stage (required) | The board in the media area |
| Sealed web view (required) | The board is drawn by the plugin's own page, with no internet access |
| Apps menu (required) | **Start whiteboard** |
| Dialogs (required) | Board settings, saved boards and Describe board |
| Who is in your channel (required) | Joins, leaves and the host hand-over |
| Random numbers (required) | Board ids and author keys |
| Relay (required) | Send the board to the others |
| Side panel (optional) | The mini view next to a screen share |
| Who is talking (optional) | A speaking ring on people's cursors |
| Post into the chat and replace text lines with cards (optional) | The two text lines for standard Mumble clients, hidden for Mumcord users |
| Chat cards (optional) | The invite card with **Join** |
| Commands (optional) | `/whiteboard` |
| Save files (optional) | Save exports through Mumcord's save window |
| Post pictures to chat (optional) | **Post to chat** |

[Source code](https://github.com/Mumcord/plugins/tree/main/whiteboard)
