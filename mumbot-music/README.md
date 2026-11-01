# Mumbot Music

Official Mumcord plugin: control a [mumbot](https://github.com/Mumcord/mumbot) music bot in your channel. See now playing, skip, stop, change the volume, look at the queue and add tracks, without curl or scripts.

## How it works

- Right-click the bot in your channel and choose **Configure bot**. The entry appears for every user with a certificate in your channel, because Mumcord cannot know which user is a bot.
- **First time:** enter the bot's address (`host:port`, or an `https://` URL when the bot sits behind a TLS proxy) and its API key, then **Test connection**. **Save** stays disabled until the test passes.
- **Configured:** the same window shows now playing, Skip, Stop, Volume, the queue, Add track (a file name from the bot's media folder or a URL), Upload an MP3, the "Play next, interrupt" switch, **Edit address / key** and **Remove this bot**.
- **Bot leaves your channel (or you do):** the window shows "Mumbot is not in your channel", hides the controls and stops asking the bot. The controls come back by themselves when the bot returns.
- **Settings > Plugins > Mumbot Music** lists all saved bots with **Edit** and **Remove**. Edit there changes the address or key without testing it.
- Pause, remove one track, reorder, seek and a file list stay hidden until mumbot supports them.

## Permissions

| Permission | Why |
|---|---|
| Right-click menu entry (required) | The **Configure bot** entry |
| Open a window (required) | The setup form and the player |
| Who is in your channel (required) | Show the controls only while the bot is in your channel |
| Your own channel (required) | Notice when you change channel or server |
| Talk to a server you choose (required) | Send requests to the bot's address, and only there |
| Keep a secret (required) | Keep the API key in your system keychain |
| Notifications (optional) | Tell you when the bot you are controlling goes offline |

## Data

- **Which bot:** the bot's certificate (its peer key) plus the server, never its session, so the bot is recognised again after it reconnects or Mumcord restarts. A bot with a new certificate counts as a new bot.
- **Storage** (identity scope): `bots/<server>/<peer>` holds the bot's name, the address and which key slot is active. `<server>` and `<peer>` are the first 32 hex characters of the server key and the peer key, so that secret names stay well under the 128-byte limit.
- **API key:** only in the OS keychain, never in storage, logs or exports. The plugin cannot read it; it names it in the `X-API-Key` header and Mumcord inserts it. Each bot has two slots, `bot/<server>/<peer>/a` and `…/b`. Editing the key writes the inactive slot, the test uses that slot and **Save** switches to it, so a failed or cancelled edit never loses the working key. **Remove** deletes the stored entry and both slots.

## Requests

Mumbot music API (`docs/music.md` of mumbot): `GET /queue` for the connection test and the refresh, `POST /skip`, `POST /stop`, `POST /volume`, `POST /play` with `{"file"}` or `{"url"}` (a text starting with `http://` or `https://` is a URL), and `POST /play?interrupt=` with the picked MP3 as `audio/mpeg` body.

- The window refreshes every **3 seconds**, only while it is open and the bot is in your channel, and never with a refresh already waiting. FR-6.84 asks for 2 seconds, but mumbot allows 30 requests per minute per key, so 2-second refreshes alone would use the whole budget and every click would be refused.
- On "429 Too many requests" the window shows "Too many requests, slowing down" and doubles the refresh interval up to 12 seconds; the next successful answer resets it to 3 seconds.
- A connection failure, a timeout or "503 bot not connected" shows the bot as offline and disables the controls; refreshing goes on and the window recovers by itself.
- Everything the bot returns is untrusted: titles are shown as plain text without control characters, cut to 80 characters.
- The add field holds 64 bytes (the limit of a plugin text field), so longer URLs do not fit.
- All texts are English and compiled in, because plugins cannot read their own `i18n` files yet.

## Build and test

```sh
cargo test
cargo build --release --target wasm32-unknown-unknown
```

`mumcord-plugin build` turns `mumbot_music.wasm` into the plugin component.

## Licence

AGPL-3.0-or-later, see [LICENSE](LICENSE).
