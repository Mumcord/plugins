# Poll

Official Mumcord plugin: ask the channel a question with up to ten options and watch the votes come in live.

## How it works

- Create a poll in the Poll panel of the plugins strip: a question and 2 to 10 options.
- Everyone in the channel who runs Poll gets a poll card in the chat and votes with one click. Votes can be changed until the poll is closed.
- The creator's client counts the votes and sends the tally to the channel, at most once every 2 seconds. Votes count once per person (per user certificate, or per connection for users without one).
- People who join the channel later get the current poll from the creator (needs the optional "who is in your channel" permission on the creator's side).
- If the creator leaves, the poll stays visible as "Creator left, poll is frozen".
- Closing the poll posts the result and sends the final tally and the list of voters.
- When a poll is created or closed, a plain text line goes to the channel so people on the official Mumble client see it too. Mumcord users with Poll see the card instead of that line.

## Permissions

| Permission | Why |
|---|---|
| Panel (required) | The form to create a poll |
| Chat cards (required) | The poll card in the chat |
| Messages to the same plugin (required) | Send polls, votes and tallies |
| Post into the chat (required) | The text line for people without Poll |
| Replace text lines with cards (required) | Show the card instead of the text line |
| Who is in your channel (optional, suggested) | Send your poll to people who join later and notice when a creator leaves |
| Notifications (optional) | Tell you when someone else starts a poll |

## Messages

Plugin data id `mumcord.p.poll`. Payloads are binary: integers big-endian, strings as a one-byte length plus UTF-8.

| Kind | Direction | Payload |
|---|---|---|
| 1 create | creator → channel (or one late joiner) | poll id u32, line hash u32 (0 = none), question (≤ 200 bytes), option count u8 (2–10), options (≤ 40 bytes each) |
| 2 vote | voter → creator only | poll id u32, option u8 |
| 3 tally | creator → channel, ≤ 1 per 2 s | poll id u32, closed u8, closing line hash u32, count u8, counts u16 each |
| 4 voter list | creator → channel at close, or one late joiner | poll id u32, part u8, parts u8, count u8, entries (option u8, name ≤ 16 bytes); ≤ 100 voters in total |

Only messages from the creator's session change a poll; malformed messages are dropped.

The question field in the panel holds 64 bytes (the limit of a plugin text field); the message format accepts up to 200.

## Build and test

```sh
cargo test
cargo build --release --target wasm32-unknown-unknown
```

`mumcord-plugin build` turns `poll.wasm` into the plugin component.

## Licence

AGPL-3.0-or-later, see [LICENSE](LICENSE).

## Scenario tests

`scenarios/*.yaml` run the built package in the `mumcord-plugin` mock host:

```
mumcord-plugin build
mumcord-plugin test dist/poll-1.0.0.mcplugin scenarios/*.yaml
```

They cover creating and closing a poll (card, text line, final tally), resending to a user who joins later, voting and tally updates on someone else's poll, a frozen poll when the creator leaves, and missing permissions. The poll id is the current second, so creator-side steps check message kinds and targets, not exact payloads.
