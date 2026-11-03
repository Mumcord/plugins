# skyloom

Official Mumcord plugin: **Skyloom**, a voice-first party game for 2–12 people. Each round one
player, the *Weaver*, sees a hidden constellation and describes it out loud, while everyone else
moves a single glowing star on a shared night-sky loom. At dawn the threads are pulled tight and
the group sees how close their sky came to the real figure.

Needs Mumcord 0.7 or newer (plugin API 1.1: stage, Apps menu, relay, sound effects).

## Playing

- **Start:** *Start Skyloom* in the composer's Apps menu (or `/skyloom`) opens a lobby in the media
  area. Others join with the Join button Mumcord shows for the activity, or from the invite card in
  the chat. Standard Mumble users see one `[Skyloom] …` line and can talk and guess along.
- **Lobby:** the opener is the *Lamplighter* and picks the weave time (60, 90 or 120 s) and the
  sky: *Calm* or *Mixed* (twists from round 3). Anyone can choose to play or just watch.
- **A round (about 2 minutes):**
  1. *Seal:* the Weaver's client commits to a figure; the Threaders' clients add randomness.
  2. *Weave:* the Weaver talks; Threaders drag their star or move it with the keyboard. The Weaver
     can *pluck* a star so everyone sees which one is meant, and can *tie off* early for a small
     bonus (one point per 4 spare seconds, at most 5).
  3. *First light:* the last 10 seconds use the fastest sync.
  4. *Dawn:* positions lock, the figure is revealed with its threads and every star's score.
  5. *Next:* the next Weaver presses "I'm ready to weave". That click also posts the round's line
     for standard Mumble clients.
- **Roles:** with 2–3 players the Threaders hold several stars so a figure has at least 4 points.
  Late joiners watch and get a star next round. If a Threader leaves, their star fades and is not
  scored; if the Weaver leaves, clouds roll in and the next Weaver starts. If the Lamplighter
  leaves, the longest-present player takes over.
- **A night:** one round per player, at most 8. The team score is the average resonance; the
  tapestry at the end names a *Silver tongue* (best Weaver), a *Truest star* (closest star most
  often) and a *Wanderer* (moved furthest). Name the night's sky to keep it among your last 20
  past skies (stored on your device only, without other people's names).
- **Twists (Mixed):** *Fog* (Threaders only see stars near their own), *Mirror* (the Weaver sees the
  figure flipped left to right), *Drift* (one star slowly drifts and must be held), *Twin Weavers*
  (8+ players: two Weavers each see half of the figure's points).

## Scoring: resonance

At dawn the figure is sampled into exactly as many points as there are scored stars, at equal
distances along its lines. Both point sets are moved to their centroid and scaled to the same
RMS radius, so where the group built the figure and how big does not matter. Rotation is not
removed: up means up. Stars are matched to figure points by the assignment with the least total
squared distance (exact, dynamic programming over subsets, at most 11 points). A star scores
`100 − 100 · d / 0.9` percent (at least 0), where `d` is its distance to its point in RMS radii;
the round's resonance is the mean. ≥ 60 % weaves the constellation into the sky, ≥ 90 % weaves it
in gold. Everything is integer maths, so every client gets bit-identical results.

## Fairness: commit, witness, reveal

- The Weaver picks a 32-byte secret `S` and sends `C = SHA-256("skyloom1" ‖ S ‖ round)` together
  with the set of figures already used this night.
- Each Threader then sends a 16-byte witness nonce. Weaving starts once all are in (or after 3 s);
  the start message names the roster slots whose nonces count.
- The figure is `free[k]`, where `free` is the deck without the used figures and `k` comes from the
  stream `HMAC-SHA256(S, "skyloom1" ‖ block ‖ round ‖ (slot ‖ nonce)…)` by rejection sampling. The
  Weaver cannot pick an easy figure, and the Threaders' clients cannot know it before dawn.
- At the deadline every Threader sends the final position of their own stars (*lock*). The Weaver
  then reveals `S` and which locks are scored. Every client checks `C`, draws the figure, scores
  the round and sends a digest of its result; matching digests show *Verified*, a mismatch shows
  *Unverified: results differ*, a broken seal *Unverified: the figure did not match the seal*.
- In a Twin Weavers round the Weaver sends `S` directly to the second Weaver only.
- Senders are the Mumble sessions the server attributes messages to; payloads never claim an
  identity. The Lamplighter seats players and sets the settings; only the Weaver seals and reveals;
  only a Threader moves their own stars.

## Wire format

Messages go over the relay when the channel has one and the user allows it (star movement is then
live at about 12 updates per second), and otherwise as Mumble plugin data with kind 1 (own stars at
most every 2 s, 4 per second in the game burst during first light). Byte 0 is the format (1),
byte 1 the type; integers are big-endian; positions are on a 1600 × 900 loom. Every message is at
most 64 bytes.

| Type | Sender | Payload |
|---|---|---|
| 1 roster | Lamplighter | night u32, rev u8, round u8, weave s u8, mixed u8, woven-slots u16, count u8, sessions u32 × count |
| 2 join | anyone | play u8 |
| 3 leave | anyone | – |
| 4 ready | next Weaver | round u8, roster rev u8 |
| 5 seal | Weaver | round u8, commitment 32 B, used figures u64 |
| 6 witness | Threader | round u8, nonce 16 B |
| 7 weave | Weaver | round u8, witness slots u16 |
| 8 stars | Threader | round u8, count u8, (x u16, y u16) × count |
| 9 lock | Threader | as stars |
| 10 pluck | Weaver | round u8, target session u32, star u8 |
| 11 tie off | Weaver | round u8, spare seconds u8 |
| 12 reveal | Weaver | round u8, secret 32 B, scored slots u16 |
| 13 result | everyone | round u8, digest 8 B |
| 14 share | Weaver → second Weaver | round u8, secret 32 B |
| 15 end | Lamplighter | night u32 |

## Chat lines for standard Mumble clients

Only after a click, always English, each at most 500 characters: the opener's start line, one line
per round (posted by the next Weaver's "I'm ready to weave", or the Lamplighter's "See the
tapestry" after the last round) and the summary ("Post summary to chat"). Mumcord users with
Skyloom see a card instead of each line (`chat.supersedes`).

## Accessibility

Tab reaches your star; arrow keys move it, Shift for fine steps, Enter settles, `L` says where it is
("Your star B: centre, 2 left of Ben's triangle"). The Weaver plucks stars with the number keys.
A live region announces phases, plucks, the timer every 30 s and in the last 10 s, and the
reveal. Every star has a unique shape and initial, so colour is never the only signal. Reduced
motion removes the twinkle, the ripples and the thread run, and makes dawn a stepped bar.

## Layout

- `src/`: the plugin (Rust): `app.rs` state machine, `rules.rs` roles and twists, `deck.rs` the 60
  figures, `score.rs` resonance, `fair.rs` commit–reveal, `wire.rs` messages, `view.rs` page JSON,
  chat cards and lines, `lib.rs` the host glue, `tests.rs` unit and multi-client tests.
- `web-src/`: the page's TypeScript; `npm ci && npm run build` writes `web/app.js` (committed, so
  the plugin builds without Node). `npm run check` verifies it is current, `npm run lint` lints.
- `web/`: the page, its stylesheet and the bundled Fraunces Italic font (SIL OFL 1.1, see
  `web/fonts/OFL-Fraunces.html`).
- `sounds/`: the effects, synthesised by `sounds-src/make.sh`.
- `e2e/`: end-to-end specs for the plugins repository's harness.

## Build and test

```sh
cargo test
cargo clippy --target wasm32-unknown-unknown -- -D warnings
mumcord-plugin build . && mumcord-plugin check --official skyloom-1.0.0.mcplugin
(cd web-src && npm ci && npm run check && npm run lint)
```

## Languages

All texts live in `i18n/<tag>.json` for the 24 Mumcord languages. The plain chat lines for
standard Mumble clients are always English.

## Licence

AGPL-3.0-or-later, see [LICENSE](LICENSE). Fraunces is © The Fraunces Project Authors, SIL Open
Font License 1.1.
