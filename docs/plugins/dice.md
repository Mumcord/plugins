# Dice roller

<img class="mp-plugin-icon" src="../../dice/assets/icon-128.png" alt="" width="96" height="96">

`/roll d20` or `2d6+3` with fair rolls that others in the channel can witness and verify.

## Roll

Type `/roll` followed by a roll in the chat box:

| Roll | Meaning |
|---|---|
| `/roll` or `/roll d20` | One twenty-sided die |
| `/roll 2d6+3`, `/roll 3d8-1` | Several dice plus or minus a modifier |
| `/roll d20 adv`, `/roll d20 dis` | Roll twice, keep the higher (advantage) or the lower (disadvantage) |
| `/roll 4d6 dl`, `/roll 4d6 dh` | Drop the lowest or the highest die |

You can roll up to 20 dice at once.

The result appears as a card in the chat. People without the plugin see one text line such as
`🎲 rolled 2d6+3 = 11 (4, 4)`. With the panel, you can expand the history of your rolls in this
session.

## Fair rolls

There is no server that rolls for you, so other Mumcord users with the plugin in your channel
**witness** each roll: they add their own random values before the result is revealed, and every
client checks the result by itself. Nobody can pick their own result. The card shows:

- **Verified:** at least one other person witnessed the roll and every check passed.
- **Unverified (rolled alone):** nobody else with the plugin could witness the roll.
- **Not revealed:** the roller never revealed the result. The roll does not count.
- **Verification failed:** the result does not match what was sent. The roll does not count.

## Permissions

| Permission | Why |
|---|---|
| Chat cards (required) | The roll card in the chat |
| Messages to the same plugin (required) | Witness and check rolls together |
| Post into the chat (required) | The text line for people without the plugin |
| Replace text lines with cards (required) | Show the card instead of the text line |
| Secure random numbers (required) | Fair dice |
| Chat commands (required) | The `/roll` command |
| Panel (optional) | Your roll history |

[Source code](https://github.com/Mumcord/plugins/tree/main/dice)
