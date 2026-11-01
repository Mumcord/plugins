#!/usr/bin/env python3
"""Generates the Echoling pet art: one SVG per species, stage and colour.

Usage: python3 art/generate.py  (run from the echolings folder; writes assets/pets/*.svg)

The SVGs only use elements and attributes that Mumcord's SVG sanitiser keeps (no scripts, no
styles, no external references), so the installed files look exactly like these.
"""

from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "assets" / "pets"

# (body, accent) per colour variant. The third variant of each species is the unlockable one.
PALETTES = {
    "fizzle": {"ember": ("#ff7a3d", "#ffd23f"), "rose": ("#ff6f91", "#ffc2d1"), "frost": ("#6ec6ff", "#e0f4ff")},
    "burrble": {"lagoon": ("#3cc8c8", "#a8f0e8"), "kelp": ("#5aa05a", "#c8e6a0"), "deep": ("#3a5bd9", "#9fb4ff")},
    "mossmo": {"meadow": ("#7cc46a", "#f2e394"), "autumn": ("#d98b3a", "#f6d28b"), "blossom": ("#f29ac0", "#fde2ef")},
    "zapling": {"thunder": ("#8a94a8", "#ffe14d"), "sunset": ("#ff9a6b", "#ffe0a3"), "midnight": ("#5a5ea8", "#a7b7ff")},
    "hushmoth": {"dusk": ("#9b7ad8", "#f5e6a8"), "dawn": ("#ffb38a", "#fff1c9"), "aurora": ("#58d6b0", "#d3b8ff")},
}

STAGES = ["hum", "sprout", "buddy", "echo"]
SCALE = {"sprout": 0.72, "buddy": 0.9, "echo": 1.0}
INK = "#2b2140"


def face(cx, cy, spread=12):
    return (
        f'<ellipse cx="{cx - spread}" cy="{cy}" rx="4.5" ry="6" fill="{INK}"/>'
        f'<ellipse cx="{cx + spread}" cy="{cy}" rx="4.5" ry="6" fill="{INK}"/>'
        f'<circle cx="{cx - spread + 1.5}" cy="{cy - 2.5}" r="1.6" fill="#ffffff"/>'
        f'<circle cx="{cx + spread + 1.5}" cy="{cy - 2.5}" r="1.6" fill="#ffffff"/>'
        f'<ellipse cx="{cx - spread - 6}" cy="{cy + 8}" rx="4" ry="2.5" fill="#ff8fa3" opacity="0.6"/>'
        f'<ellipse cx="{cx + spread + 6}" cy="{cy + 8}" rx="4" ry="2.5" fill="#ff8fa3" opacity="0.6"/>'
        f'<path d="M{cx - 4} {cy + 8} Q{cx} {cy + 12} {cx + 4} {cy + 8}" fill="none" stroke="{INK}" stroke-width="2" stroke-linecap="round"/>'
    )


def fizzle(body, accent, full):
    tail = (
        f'<path d="M92 92 Q116 84 108 62 Q104 74 96 78" fill="{accent}" stroke="{body}" stroke-width="3"/>'
        if full
        else ""
    )
    return (
        tail
        + f'<polygon points="36,52 30,20 56,42" fill="{body}"/>'
        + f'<polygon points="92,52 98,20 72,42" fill="{body}"/>'
        + f'<path d="M64 30 Q52 14 62 4 Q62 16 72 12 Q78 24 64 30Z" fill="{accent}"/>'
        + f'<ellipse cx="64" cy="74" rx="36" ry="32" fill="{body}"/>'
        + f'<ellipse cx="64" cy="86" rx="20" ry="14" fill="{accent}" opacity="0.7"/>'
        + face(64, 70)
    )


def burrble(body, accent, full):
    fronds = "".join(
        f'<ellipse cx="{x}" cy="{y}" rx="9" ry="5" fill="{accent}" transform="rotate({a} {x} {y})"/>'
        for x, y, a in ([(28, 60, -30), (24, 76, 0), (100, 60, 30), (104, 76, 0)] + ([(48, 34, -60), (80, 34, 60)] if full else []))
    )
    bubbles = (
        f'<circle cx="104" cy="30" r="6" fill="none" stroke="{accent}" stroke-width="2"/>'
        f'<circle cx="114" cy="16" r="3.5" fill="none" stroke="{accent}" stroke-width="2"/>'
        if full
        else ""
    )
    return (
        fronds
        + bubbles
        + f'<circle cx="64" cy="70" r="36" fill="{body}"/>'
        + f'<ellipse cx="52" cy="52" rx="10" ry="6" fill="#ffffff" opacity="0.35"/>'
        + face(64, 72)
    )


def mossmo(body, accent, full):
    mushroom = (
        f'<rect x="96" y="88" width="6" height="12" rx="2" fill="#f4ead8"/>'
        f'<path d="M90 90 Q99 76 108 90Z" fill="#e0524a"/>'
        f'<circle cx="96" cy="85" r="1.6" fill="#ffffff"/><circle cx="102" cy="86" r="1.4" fill="#ffffff"/>'
        if full
        else ""
    )
    return (
        f'<path d="M64 40 Q60 28 64 20" fill="none" stroke="#4e8a3e" stroke-width="3" stroke-linecap="round"/>'
        + f'<ellipse cx="56" cy="22" rx="8" ry="4" fill="#6fbf5a" transform="rotate(-25 56 22)"/>'
        + f'<ellipse cx="72" cy="20" rx="8" ry="4" fill="#6fbf5a" transform="rotate(25 72 20)"/>'
        + f'<path d="M24 100 Q24 40 64 40 Q104 40 104 100Z" fill="{body}"/>'
        + f'<path d="M34 64 Q44 56 54 62 M74 58 Q86 52 94 62" fill="none" stroke="{accent}" stroke-width="4" stroke-linecap="round" opacity="0.8"/>'
        + mushroom
        + face(64, 76)
    )


def zapling(body, accent, full):
    bolt = (
        f'<polygon points="96,84 116,92 104,96 118,112 94,98 106,96" fill="{accent}" stroke="{INK}" stroke-width="1.5"/>'
        if full
        else ""
    )
    puffs = "".join(
        f'<circle cx="{x}" cy="{y}" r="{r}" fill="{body}"/>'
        for x, y, r in [(40, 70, 22), (64, 56, 26), (88, 70, 22), (52, 86, 20), (78, 86, 20)]
    )
    return (
        bolt
        + puffs
        + f'<ellipse cx="54" cy="46" rx="10" ry="5" fill="#ffffff" opacity="0.35"/>'
        + face(64, 72)
    )


def hushmoth(body, accent, full):
    antennae = (
        f'<path d="M54 40 Q46 22 38 16" fill="none" stroke="{INK}" stroke-width="2"/>'
        f'<path d="M74 40 Q82 22 90 16" fill="none" stroke="{INK}" stroke-width="2"/>'
        f'<circle cx="38" cy="16" r="5" fill="{accent}"/><circle cx="90" cy="16" r="5" fill="{accent}"/>'
    )
    wings = (
        f'<ellipse cx="28" cy="64" rx="22" ry="28" fill="{accent}" opacity="0.85" transform="rotate(-20 28 64)"/>'
        f'<ellipse cx="100" cy="64" rx="22" ry="28" fill="{accent}" opacity="0.85" transform="rotate(20 100 64)"/>'
        if full
        else f'<ellipse cx="36" cy="66" rx="14" ry="18" fill="{accent}" opacity="0.85"/>'
        f'<ellipse cx="92" cy="66" rx="14" ry="18" fill="{accent}" opacity="0.85"/>'
    )
    return (
        antennae
        + wings
        + f'<ellipse cx="64" cy="70" rx="30" ry="34" fill="{body}"/>'
        + f'<path d="M44 46 Q64 36 84 46" fill="none" stroke="#ffffff" stroke-width="4" opacity="0.5" stroke-linecap="round"/>'
        + face(64, 70)
    )


SPECIES = {"fizzle": fizzle, "burrble": burrble, "mossmo": mossmo, "zapling": zapling, "hushmoth": hushmoth}


def hum(body, accent):
    return (
        f'<defs><radialGradient id="g" cx="0.4" cy="0.35" r="0.7">'
        f'<stop offset="0" stop-color="#ffffff"/><stop offset="0.35" stop-color="{accent}"/>'
        f'<stop offset="1" stop-color="{body}"/></radialGradient></defs>'
        f'<circle cx="64" cy="68" r="44" fill="{accent}" opacity="0.25"/>'
        f'<circle cx="64" cy="68" r="30" fill="url(#g)"/>'
        + face(64, 70, spread=8).replace('rx="4.5" ry="6"', 'rx="3" ry="4"')
    )


def aura(accent):
    stars = "".join(
        f'<path d="M{x} {y - 7} L{x + 2} {y - 2} L{x + 7} {y} L{x + 2} {y + 2} L{x} {y + 7} L{x - 2} {y + 2} L{x - 7} {y} L{x - 2} {y - 2}Z" fill="{accent}"/>'
        for x, y in [(14, 22), (114, 36), (18, 108), (110, 112)]
    )
    return f'<circle cx="64" cy="66" r="60" fill="{accent}" opacity="0.18"/>' + stars


def svg(species, stage, body, accent):
    if stage == "hum":
        inner = hum(body, accent)
    else:
        s = SCALE[stage]
        shift = 64 * (1 - s)
        drawing = SPECIES[species](body, accent, stage != "sprout")
        inner = (aura(accent) if stage == "echo" else "") + (
            f'<g transform="translate({shift:.1f} {shift + 8 * (1 - s):.1f}) scale({s})">{drawing}</g>'
        )
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" width="128" height="128">{inner}</svg>\n'


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    for species, colours in PALETTES.items():
        for colour, (body, accent) in colours.items():
            for stage in STAGES:
                (OUT / f"{species}-{stage}-{colour}.svg").write_text(svg(species, stage, body, accent))


if __name__ == "__main__":
    main()
