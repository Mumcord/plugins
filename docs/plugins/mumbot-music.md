# Mumbot Music

<img class="mp-plugin-icon" src="../../mumbot-music/assets/icon-128.png" alt="" width="96" height="96">

A remote for your [mumbot](https://github.com/Mumcord/mumbot) music bot: see what is playing, skip,
change the volume, look at the queue and add tracks.

> [!NOTE]
> You need a mumbot bot on your server. The bot is an ordinary Mumble user, so the server itself
> needs no changes.

## Set up the bot

1. Right-click the bot in your channel and choose **Configure bot**.
2. Enter the bot's address and its API key. Ask whoever runs the bot for both.
3. Choose **Test connection**. **Save** becomes available once the test passes.

The **Configure bot** entry appears for every user with a certificate, because Mumcord cannot know
which user is a bot.

## Control the music

Right-click the bot and choose **Configure bot** again. The window shows:

- **Now playing**
- **Skip** and **Volume**
- the **queue**
- **Add track**: a file name from the bot's media folder or a URL

When the bot leaves your channel (or you do), the controls hide and come back by themselves when
you are together again.

## Manage saved bots

**Settings → Plugins → Mumbot Music** lists all saved bots, where you can edit or remove them.

## Your API key stays safe

The API key is stored only in your system keychain, never in plugin storage, logs or exports. The
plugin itself cannot read it: Mumcord adds it to the requests to the bot's address, and only there.

## Permissions

| Permission | Why |
|---|---|
| Right-click menu entry (required) | The **Configure bot** entry |
| Window (required) | The setup form and the player |
| Who is in your channel (required) | Show the controls only while the bot is in your channel |
| Your own status (required) | Notice when you change channel or server |
| Talk to a server you choose (required) | Send requests to the bot's address, and only there |
| Keep a secret (required) | Keep the API key in your system keychain |
| Notifications (optional) | Tell you when the bot goes offline |

[Source code](https://github.com/Mumcord/plugins/tree/main/mumbot-music)
