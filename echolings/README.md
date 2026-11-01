# Echolings

Official Mumcord plugin: a little creature made of sound that grows while you hang out with friends. It never dies, never gets sick, never leaves and never nags. Design: Mumcord/mumcord#264, delivered as a plugin per Mumcord/mumcord#279 (spec FR-6.83).

Licence: AGPL-3.0-or-later (see `LICENSE`).

## What it does

- **Five species**, each with three colours (the third unlocks at Bond 10): Fizzle (energetic), Burrble (chatty), Mossmo (cosy), Zapling (competitive), Hushmoth (shy).
- **Four stages**: Hum (a glowing orb) → Sprout (Bond 3) → Buddy (Bond 10) → Echo (Bond 25, with a sparkle aura).
- **Meters**: Cheer rises with company and falls slowly while you are away (never below 20: the pet "misses you"); Energy falls while the pet is active and refills with naps, Do Not Disturb or quiet time and time away; Bond only ever goes up.
- **Care actions** in the panel: Pat (free), Treat (costs Notes), Play (starts Echo Echo), Nap.
- **Notes** (♪) are earned by using Mumcord with others, with daily caps.
- **Streaks** count days together; one missed day per week is bridged, and a broken streak never lowers Bond.
- **Do Not Disturb and quiet time**: the pet sleeps, plays no reactions and earns nothing from voice.
- **Your pet** shows in the Plugins panel, as your member badge (it bobs while you talk) and on your own profile card.
- **Settings**: animation Full / Subtle / Still. The system's reduced-motion setting always wins.
- **Storage**: one pet per identity in plugin storage (scope `identity`). Export and "start over" are the host's Export / Reset data in Settings → Plugins.

- **Other people's pets**: Mumcord users in your channel who run Echolings see your pet as a member badge (it bobs while you talk) and on your profile card, and you see theirs. Who receives your pet card follows the plugin's visibility (default Friends); with Nobody nothing is sent and others cannot tell you have a pet.
- **Visits**: open someone's profile card and pat their pet, once per pet per day. Both pets earn Notes; the owner gets a quiet notification (never during Do Not Disturb).
- **Reactions**, computed locally without traffic: the pet waves when someone joins your channel, sparkles when someone shares their screen and hops when you are mentioned. None while it sleeps.
- **Echo Echo**: Play shows a chat card (or the panel without the chat-card permission) with a five-note tune on four pads. Everyone in the channel can join within 45 seconds, listens to the tune and repeats it; then every card shows the scores.

Snack Race (web view) is a separate task (B41c).

## Messages

Binary, version byte first, all sent through the plugin messaging API (at most 997 bytes, rate limited by Mumcord). Everything received is untrusted and validated; unknown species, colours, moods or notes drop the message, and pet names are cleaned like your own.

| Kind | Message | Sent |
|---|---|---|
| 1 | Pet card: species, colour, mood, Bond, days together, streak, name (at most 75 bytes) | to the channel when you join one, when someone joins and when it changes; at most once per minute |
| 2 | Pat | directly to the pet's owner, once per pet per day |
| 3 | Echo Echo invite: game id and tune | to the channel when you press Play |
| 4 | Echo Echo score: game id and correct notes | to the channel when you finish |

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
| Play (start or join Echo Echo) | +10 Cheer, −10 Energy, +3 points; needs 10 Energy and an awake pet | none |
| Finished an Echo Echo round | +3 ♪, +3 points | 9 ♪ |
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

`assets/pets/<species>-<stage>-<colour>.svg` (60 files) are generated by `python3 art/generate.py`. Edit the script, not the SVGs. They use only SVG features Mumcord's sanitiser keeps.

## Translations

UI strings live in `i18n/<lang>.json` (English is required). The plugin API cannot read package files, so the plugin compiles the same files in (`src/ui/i18n.rs`); add new languages to the `FILES` list there too.
