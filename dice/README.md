# dice

Official Mumcord plugin: roll dice in chat with `/roll`, and everyone with the plugin checks
each roll, so nobody can fake a result (FR-6.82).

## Using it

Type `/roll` followed by a roll in the chat box:

| Roll | Meaning |
|---|---|
| `d20` (or just `/roll`) | one twenty-sided die |
| `2d6+3`, `3d8-1` | several dice plus or minus a modifier |
| `d20 adv`, `d20 dis` | roll twice, keep the higher (advantage) or lower (disadvantage) |
| `4d6 dl`, `4d6 dh` | drop the lowest or highest die |

Limits: 1 to 20 dice, 2 to 1000 sides, one modifier between -1000 and +1000.

The result appears as a card in the chat. People without the plugin see one text line such as
`🎲 rolled 2d6+3 = 11 (4, 4)`; Mumcord users with the plugin see the card instead of that line.
With the optional panel permission, the plugin keeps a history of this session's rolls.

## How a roll is checked

There is no server that rolls for us, so the roll uses commit, witness, reveal:

1. **Commit (kind 1):** the roller picks a random 16-byte secret `R` and sends the roll id, the
   notation and `C = SHA-256("mcdice1" ‖ id ‖ notation ‖ R)`. From now on it cannot change
   the roll or the secret.
2. **Witness (kind 2):** every other client with the plugin waits a random 0–1.5 s and sends a
   random 16-byte value `W`, unless it has already seen two witness values for this roll.
3. **Reveal (kind 3):** after two witness values, 2.5 s after the commit if at least one
   witness value has arrived by then (or as soon as the first one arrives after that), or
   after 5 s, the roller sends `R` and the witness values it used. Everyone computes the seed
   `S = SHA-256("mcdice1" ‖ id ‖ R ‖ W1 ‖ W2)` and the dice from the stream
   `HMAC-SHA256(S, counter)`, four bytes per die with rejection sampling (no modulo bias).

Why the early reveal at 2.5 s is still fair: the roller has fixed `R` in the commitment, so
choosing when to reveal cannot change the dice; it only decides which witness values count, and
those are the first ones sent in the channel, which everyone checks. Honest witnesses answer
within 1.5 s of seeing the commit, so by 2.5 s every witness that is coming has answered. With
only one other player the second witness never comes, and without the early reveal every such
roll would wait the full 5 s.

Each client checks that `R` matches the commitment and that the witness values are exactly the
first two sent in the channel, then computes the dice itself. The card shows:

- **Verified:** at least one witness took part and every check passed.
- **Unverified (rolled alone):** nobody else with the plugin could witness the roll.
- **Not revealed:** the roller never revealed it (5 s after it should have). The roll is void.
- **Verification failed:** the reveal does not match what was sent in the channel. The roll
  does not count, and the roller's text line is not hidden.

Honest limits: a roller who sees the witness values can work out the result first and hold back
a bad roll, which then shows as "Not revealed". If every witness cheats together with the
roller, the check cannot help.

## Wire format

All integers are big-endian.

| Kind | Direction | Payload |
|---|---|---|
| 1 commit | roller → channel | roll id u32, commitment 32 B, canonical notation (rest, ≤ 32 B) |
| 2 witness | witness → channel | roller session u32, roll id u32, value 16 B |
| 3 reveal | roller → channel | roll id u32, secret 16 B, count u8 (0–2), values 16 B each |

The known-answer vectors in `src/tests.rs` were computed independently with Python's
`hashlib` and `hmac`, so other implementations of the same pattern can test against them.

## Languages

All texts the plugin shows (cards, states, errors, history) live in `i18n/<tag>.json` for the 24
Mumcord languages and switch with the app language. The plain-text line posted to Mumble chat for
people without the plugin is always English. A native test checks that every file has the keys of
`en.json`.

## Build and test

```sh
cargo test
cargo build --release --target wasm32-unknown-unknown
```

`mumcord-plugin build` turns `dice.wasm` into the plugin component and package.

## Tests

The mock-host scenarios are in `scenarios/`, the end-to-end specs in `e2e/` (see the repository
README, section Tests).

## Licence

AGPL-3.0-or-later, see [LICENSE](LICENSE).
