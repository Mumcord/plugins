# Poll

<img class="mp-plugin-icon" src="../../poll/assets/icon-128.png" alt="" width="96" height="96">

Ask the channel a question with up to ten options and watch the votes come in live.

## Start a poll

1. Open **Poll** from the plugins strip and choose **New poll**.
2. Enter a question and 2 to 10 options.
3. Start the poll. Everyone in the channel sees it.

## Vote

- **With Mumcord and Poll:** a poll card appears in the chat with a live bar per option. Vote with
  one click. You can change your vote until the poll ends.
- **Everyone else** (official Mumble client, or Mumcord without Poll): type `!poll 1`, `!poll 2` …
  in the channel chat. These votes count like any other vote: one per person, and a later one
  replaces the earlier one.

The Poll panel shows the current poll of your channel. **All options** opens the large view with
every option and the voters.

## End a poll

The creator ends the poll with **End poll**. The results are posted to the channel together with
the list of voters.

When a poll starts or ends, a plain text line goes to the channel, so people on the official Mumble
client can follow along, for example:

```text
[Poll] Pizza or pasta? 1) Pizza 2) Pasta · Vote with !poll 1 to !poll 2
```

Mumcord users with Poll see the card instead of that line.

## Good to know

- People who join the channel later still get the current poll.
- If the creator leaves, the poll stays visible as frozen.

## Permissions

| Permission | Why |
|---|---|
| Panel (required) | The current poll, voting and the form to create one |
| Chat cards (required) | The poll card in the chat |
| Messages to the same plugin (required) | Send polls, votes and results |
| Post into the chat (required) | The text line for people without Poll |
| Replace text lines with cards (required) | Show the card instead of the text line |
| Read trigger words in the chat (optional) | Count `!poll` votes. Poll only ever sees lines that start with `!poll`. |
| Who is in your channel (optional) | Send your poll to people who join later and notice when a creator leaves |
| Notifications (optional) | Tell you when someone else starts a poll |

[Source code](https://github.com/Mumcord/plugins/tree/main/poll)
