# Echolings

Official Mumcord plugin: a little creature made of sound that grows while you hang out with friends. It never dies, never gets sick, never leaves and never nags. Design: Mumcord/mumcord#264, delivered as a plugin per Mumcord/mumcord#279 (spec FR-6.83).

Licence: AGPL-3.0-or-later (see `LICENSE`).

## What it does

- **Five species**, each with three colours (the third unlocks at Bond 10): Fizzle (energetic), Burrble (chatty), Mossmo (cosy), Zapling (competitive), Hushmoth (shy).
- **Four stages**: Hum (a glowing orb) → Sprout (Bond 3) → Buddy (Bond 10) → Echo (Bond 25, with a sparkle aura).
- **Meters**: Cheer rises with company and falls slowly while you are away (never below 20: the pet "misses you"); Energy falls while the pet is active and refills with naps, Do Not Disturb or quiet time and time away; Bond only ever goes up.
- **Care actions** in the panel: Pat (free), Treat (costs Notes), Play (starts Echo Echo), Snack Race, Nap.
- **Notes** (♪) are earned by using Mumcord with others, with daily caps.
- **Streaks** count days together; one missed day per week is bridged, and a broken streak never lowers Bond.
- **Do Not Disturb and quiet time**: the pet sleeps, plays no reactions and earns nothing from voice.
- **Your pet** shows in the Plugins panel, as your member badge (it bobs while you talk) and on your own profile card.
- **Settings**: animation Full / Subtle / Still. The system's reduced-motion setting always wins.
- **Storage**: one pet per identity in plugin storage (scope `identity`). Export and "start over" are the host's Export / Reset data in Settings → Plugins.

- **Other people's pets**: Mumcord users in your channel who run Echolings see your pet as a member badge (it bobs while you talk) and on your profile card, and you see theirs. Who receives your pet card follows the plugin's visibility (default Friends); with Nobody nothing is sent and others cannot tell you have a pet.
- **Visits**: open someone's profile card and pat their pet, once per pet per day. Both pets earn Notes; the owner gets a quiet notification (never during Do Not Disturb).
- **Reactions**, computed locally without traffic: the pet waves when someone joins your channel, sparkles when someone shares their screen and hops when you are mentioned. None while it sleeps.
- **Echo Echo**: Play shows a chat card (or the panel without the chat-card permission) with a five-note tune on four pads. Everyone in the channel can join for 30 seconds (a "Starts in" countdown shows on every card); then the round starts on its own with whoever joined, or earlier when the host presses Start now. The host's start message starts the tune on every joined client at once, so nobody's timer can drift. If nobody joined, the host plays alone. Players listen to the tune and repeat it; 45 seconds after the start every card shows the scores. Latecomers can still join a running round. If the host leaves before the start, the waiting players' cards say the round did not start.

- **Snack Race** (needs the optional web view permission; without it the race is hidden): the race button next to Play invites the channel with a chat card (or the panel without the chat-card permission) and opens the race in a window. The lobby works like Echo Echo's: 30 seconds or Start now, and the host's start message starts everyone's run. A marker sweeps over a timing bar; each tap (Space, Enter, the Hop! button or the bar) while it is on the snack makes your pet hop, a miss makes it stumble for a moment. Ten hops reach the snack; each run lasts at most 15 seconds and every client times only its own run, so message delays never decide the winner. While racing, hops are shared a few times per second inside a game burst so the other pets move on everyone's track. 20 seconds after the start the card and the window rank everyone: most hops first, then the fastest finish. The animation setting applies (Full hops, Subtle slides, Still jumps; the timing marker always moves because it is the game). A sleeping pet (Do Not Disturb) neither races nor receives invites. The race window shows only files of this plugin and gets everything, including all texts, from the plugin through the web-view bridge.

## Messages

Binary, version byte first, all sent through the plugin messaging API (at most 997 bytes, rate limited by Mumcord). Everything received is untrusted and validated; unknown species, colours, moods or notes drop the message, and pet names are cleaned like your own.

| Kind | Message | Sent |
|---|---|---|
| 1 | Pet card: species, colour, mood, Bond, days together, streak, name (at most 75 bytes) | to the channel when you join one, when someone joins and when it changes; at most once per minute |
| 2 | Pat | directly to the pet's owner, once per pet per day |
| 3 | Echo Echo invite: game id, tune and lobby seconds (older clients ignore the last byte) | to the channel when you press Play |
| 4 | Echo Echo score: game id and correct notes | to the channel when you finish |
| 5 | Echo Echo start: game id | to the channel when your lobby closes (after 30 seconds or Start now) |
| 6 | Snack Race invite: race id and lobby seconds | to the channel when you press the race button |
| 7 | Snack Race start: race id | to the channel when your lobby closes |
| 8 | Snack Race progress: race id and hops | to the channel while you race, at most every 350 ms and only inside a game burst |
| 9 | Snack Race score: race id, hops and finish time in ms | to the channel when your run ends |

Visits are recorded per peer key (stable per person, different in every plugin) in plugin storage under `visits`, for both given and received pats, so a modified client cannot pat more than once a day either.

## Balance

All numbers live in `src/pet/balance.rs`. Days are the user's local days.

| What | Effect | Daily cap |
|---|---|---|
| Bond | 100 Bond points = +1 Bond | 100 points per day (at most +1 Bond per day, so Echo takes at least 25 days) |
| Voice with others | per 5 minutes: +1 ♪, +5 points, +3 Cheer, −2 Energy. Counts only while connected, not deafened, not in DND, with someone else in the channel, and only if you talked in the last 5 minutes (idling does not count) | 24 ♪ |
| Own chat message | +1 ♪, +1 point | 10 ♪ |
| Someone else's chat message | +1 Cheer | none |
| Helped a friend (you write within 2 minutes after being mentioned) | +3 ♪, +3 points | 3 times (9 ♪) |
| Greeted someone (you talk within 60 s after someone joined your channel) | +2 ♪, +2 points | 5 times (10 ♪) |
| Pat | free, +5 Cheer, +2 points | 10 points |
| Treat | costs 5 ♪, +10 Cheer, +15 Energy, +5 points | none |
| Play (start or join Echo Echo or Snack Race) | +10 Cheer, −10 Energy, +3 points; needs 10 Energy and an awake pet | none |
| Finished an Echo Echo round or a Snack Race run | +3 ♪, +3 points | 9 ♪ |
| Patted someone's pet (visit) | +2 ♪, +2 points | 10 ♪ (one pat per pet per day) |
| Your pet got a visit | +2 ♪, +5 Cheer, +2 points | 10 ♪ (one pat per visitor per day) |
| Nap | 30 minutes asleep, +2 Energy per minute (+60) | none |
| Do Not Disturb / quiet time | pet sleeps, +1 Energy per 3 minutes | none |
| Away (plugin not running, or computer asleep) | −2 Cheer per hour (not below 20), +10 Energy per hour, counted up to 30 days | none |
| Alone with Mumcord open | −2 Cheer per hour (not below 20) | none |
| Streak | +1 per day spent together; one missed day per 7 days is bridged | — |

Start values: Cheer 70, Energy 80. Moods: sleeping (nap or DND), sleepy (Energy below 20), misses you (Cheer below 35), cheerful (Cheer 70 or more), otherwise content.

## Build

```sh
cargo test                                                # pet logic, views and storage on the host
cargo build --release --target wasm32-unknown-unknown     # the plugin module
```

`mumcord-plugin build` turns `target/wasm32-unknown-unknown/release/echolings.wasm` into the component that goes into the package as `plugin.wasm`, together with `manifest.json`, `i18n/`, `assets/`, `README.md` and `LICENSE`.

## Art

`assets/pets/<species>-<stage>-<colour>.svg` (60 files, the full picture) and `assets/pets/<species>-<stage>-<colour>-small.svg` (60 files, a tighter crop with thicker outlines for 16 to 24 px badges) are generated by `python3 art/generate.py`. Sleeping pets (Do Not Disturb, quiet time, naps) use the closed-eye `-sleep.svg` and `-sleep-small.svg` twins (60 files each), which carry their own "z z". The style follows the agreed mockups in Mumcord/mumcord#264. Edit the script, not the SVGs. They use only SVG features Mumcord's sanitiser keeps.

## Translations

UI strings live in `i18n/<lang>.json` (English is required). The plugin API cannot read package files, so the plugin compiles the same files in (`src/ui/i18n.rs`); add new languages to the `FILES` list there too.
