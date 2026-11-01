# Permissions explained

Before you install a plugin, Mumcord shows the list of permissions it asks for. Each permission
has a colour that tells you how sensitive it is. The texts you see come from Mumcord itself, not
from the plugin.

| Colour | Meaning |
|---|---|
| :material-circle:{ style="color: #2e9e5b" } **Green** | Harmless: show something, send a notification, roll a random number. |
| :material-circle:{ style="color: #d9a300" } **Yellow** | Learns or shares something about you or your channel, for example who is talking. |
| :material-circle:{ style="color: #d64545" } **Red** | Powerful. Only official plugins may ask for these. |

## Required and optional

- **Required** permissions are what the plugin needs to work at all.
- **Optional** permissions add extras. You can switch them off before installing or at any time
  in **Settings → Plugins**. The plugin keeps working with what is left.
- An **update** that asks for a permission the plugin did not have before waits for your approval.

## What each permission allows

### Showing things

| Permission | Colour | What it allows |
|---|---|---|
| Panel | green | A panel in the plugins strip |
| Member badge | green | A small badge next to user names |
| Profile card | green | A section on profile cards |
| Chat cards | green | Cards and name decorations in the chat |
| Right-click menu entry | green | An entry in a user's right-click menu |
| Window | green | The plugin's own window |
| Notifications | green | Notifications |

### Knowing what happens

| Permission | Colour | What it allows |
|---|---|---|
| Your own status | green | Your mute, deafen and channel status |
| Who is talking | yellow | Who is talking right now |
| Who is in your channel | yellow | Who joins and leaves your channel, and the user list |
| Chat activity | yellow | When someone writes or mentions you. Never the text itself. |

### Talking and acting

| Permission | Colour | What it allows |
|---|---|---|
| Messages to the same plugin | yellow | Exchanging data with the same plugin of other users in your channel |
| Chat commands | green | Chat commands such as `/roll` |
| Post into the chat | yellow | Posting a chat message for you, only right after you click a button or type a command |
| Replace text lines with cards | green | Hiding the plugin's own text line for people who see its card |
| Secure random numbers | green | Fair random numbers, for example for dice |

### Official plugins only

| Permission | Colour | What it allows |
|---|---|---|
| Talk to a server you choose | red | Talking to one server address that you entered yourself |
| Keep a secret | red | Keeping a secret, such as an API key, in your system keychain |
| Sealed web view | yellow | A sealed web view with no internet access |

Plugins from other repositories and developer plugins cannot get these three permissions at all.

## Things a plugin can never do

- Read your chat messages. At most it learns *that* someone wrote, or it receives only lines that
  start with its own trigger word.
- Open files on your computer or connect to the internet on its own.
- Post into the chat without you clicking or typing a command first. Every such message starts
  with the plugin's name, so everyone can see where it came from.

For the technical details, see the author page [Permissions](permissions.md).
