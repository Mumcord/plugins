# Poll

Official Mumcord plugin: ask the channel a question with up to ten options and watch the votes come in live.

## How it works

- The Poll panel in the plugins strip shows the current poll of your channel: the question, a live bar per option and Vote buttons. The small panel shows the first two options; "All options" opens the large view with every option and the voters. Without a poll it offers "New poll".
- "New poll" opens the large panel view with the form: a question and 2 to 10 options. "End poll" (only for the creator) closes it.
- Everyone in the channel who runs Poll gets a compact poll card in the chat: the question, the three options with the most votes and **Open**, which shows the whole poll in the popover, where people vote. Votes can be changed until the poll is ended.
- People without Poll (official Mumble client, or Mumcord without the plugin) vote by writing `!poll 1`, `!poll 2`, … in the channel chat. The creator's Poll counts these like any other vote: one per person, and a later one replaces the earlier one. This needs the optional "read trigger words in the chat" permission on the creator's side; without it the text line says to vote in Mumcord instead.
- The creator's client counts the votes and sends the tally to the channel, at most once every 2 seconds. Votes count once per person (per user certificate, or per connection for users without one).
- People who join the channel later get the current poll from the creator (needs the optional "who is in your channel" permission on the creator's side).
- If the creator leaves, the poll stays visible as "Creator left, poll is frozen".
- Closing the poll posts the result and sends the final tally and the list of voters.
- When a poll is created or ended, a plain text line goes to the channel so people on the official Mumble client see it too, e.g. `[Poll] Pizza or pasta? 1) Pizza 2) Pasta · Vote with !poll 1 to !poll 2`. Mumcord users with Poll see the card instead of that line.
- Poll remembers the last 20 polls (question, options, counts, who created it; no voter names) in its plugin storage, so after Mumcord restarts their text lines in the chat history still show as cards and old `!poll` votes stay hidden. A poll that was still open becomes frozen: its votes were only kept in memory.
- If several people in the channel run a poll at the same time, a `!poll 2` counts for each of their polls.

## Permissions

| Permission | Why |
|---|---|
| Panel (required) | The current poll, voting, and the form to create one |
| Chat cards (required) | The poll card in the chat |
| Messages to the same plugin (required) | Send polls, votes and tallies |
| Post into the chat (required) | The text line for people without Poll |
| Replace text lines with cards (required) | Show the card instead of the text line |
| Read trigger words in the chat (optional, suggested) | Count `!poll <number>` votes from people without Poll; Poll only ever sees lines that start with `!poll` |
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

The question field in the panel holds 64 bytes (the limit of a plugin text field, shown to the user as a character counter); the message format accepts up to 200. Votes typed as `!poll <number>` never travel as plugin messages: the creator reads them from the chat.

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

They cover creating and ending a poll (popover, card, text line, final tally), resending to a user who joins later, voting and tally updates on someone else's poll, `!poll` votes from the chat, a frozen poll when the creator leaves, and missing permissions. The poll id is the current second, so creator-side steps check message kinds and targets, not exact payloads.
