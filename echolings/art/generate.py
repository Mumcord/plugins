#!/usr/bin/env python3
"""Generates the Echoling pet art: one SVG per species, stage and colour, plus a small variant and closed-eye sleeping variants of both.

Usage: python3 art/generate.py  (run from the echolings folder; writes assets/pets/*.svg and
the small awake pictures again to web/pets/ for Snack Race)

Files:
- `<species>-<stage>-<colour>.svg`: the full picture (pet panel, profile card), 128x128 with a
  soft glow, a ground shadow and, for the Echo stage, a sparkle aura.
- `<species>-<stage>-<colour>-small.svg`: the member badge and channel tree (16 to 24 px). Tightly
  cropped, thicker outlines, no glow, shadow or tiny details, so the pet still reads at that size.

The style follows the agreed mockups (issue Mumcord/mumcord#264): round kawaii bodies with a
dark plum outline, a radial gradient that is light in the middle, big shiny eyes, pink cheeks
and a small "w" mouth. The dark outline keeps every pet readable on light and dark themes.

The SVGs only use elements and attributes that Mumcord's SVG sanitiser keeps (no scripts, no
styles, no external references), so the installed files look exactly like these.
"""

from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "assets" / "pets"
# Snack Race's web view may only load files below `web/`, so it gets its own copy of the small
# awake pictures.
WEB_OUT = Path(__file__).resolve().parent.parent / "web" / "pets"

INK = "#2b1d4f"
GOLD = "#ffd36b"
WHITE = "#ffffff"

# Per colour variant: body gradient (light middle, mid, edge), accent (tuft, fronds, leaves,
# bolt, wings), second accent (belly, tail tip, glow spots, moth ruff) and cheeks.
# The third variant of each species is the unlockable one.
PALETTES = {
    "fizzle": {
        "ember": dict(c0="#ffe08a", c1="#ffaa76", c2="#ff8573", a="#ff6fa8", a2="#ffd36b", ear="#ffc7a8", cheek="#ff7f9a"),
        "rose": dict(c0="#fff1d6", c1="#ffc4dc", c2="#f79ac4", a="#b57cf2", a2="#ffe08a", ear="#ffe0ee", cheek="#ff7fb0"),
        "frost": dict(c0="#c9d8f2", c1="#8fa3cf", c2="#6b7fb0", a="#3c5bb8", a2="#b8cdf0", ear="#b2c4e8", cheek="#7f8fe0"),
    },
    "burrble": {
        "lagoon": dict(c0="#e6f6ff", c1="#9fd6ff", c2="#5eaee8", a="#ff8fc4", a2="#d4f0ff", fin="#4f9be0", cheek="#c99ade"),
        "kelp": dict(c0="#eefbe2", c1="#a8e3a6", c2="#5fb47a", a="#ffc35c", a2="#e2f7cf", fin="#3f9a68", cheek="#f2a07a"),
        "deep": dict(c0="#c7ccff", c1="#7f88ec", c2="#4a50c4", a="#6ff0d8", a2="#bfc6ff", fin="#3a3fa8", cheek="#d48ae8"),
    },
    "mossmo": {
        "meadow": dict(c0="#e2f9cf", c1="#b2e696", c2="#7cc463", a="#a6e38c", a2="#5fa64a", cheek="#e8b071"),
        "autumn": dict(c0="#fff1bf", c1="#ffd36e", c2="#eaa140", a="#9ccc6a", a2="#d58630", cheek="#f08a6a"),
        "blossom": dict(c0="#fff0f6", c1="#ffc7de", c2="#f294bd", a="#a6e38c", a2="#e57aa8", cheek="#ff7fa0"),
    },
    "zapling": {
        "thunder": dict(c0="#fffbea", c1="#e6e0ff", c2="#b6acf2", a=INK, a2="#fff0a0", cheek="#f2c46e"),
        "sunset": dict(c0="#fff6e0", c1="#ffd2bc", c2="#ff9f94", a="#7a2e5a", a2="#fff0a0", cheek="#ff8f8f"),
        "midnight": dict(c0="#c8d4ff", c1="#7f8be0", c2="#4c50a8", a="#1a1238", a2="#a8c0ff", cheek="#b48af0"),
    },
    "hushmoth": {
        "dusk": dict(c0="#fff3e6", c1="#ddccff", c2="#ae96ee", a="#3d2f8f", a2="#fff6df", cheek="#f09ad0"),
        "dawn": dict(c0="#fffaf0", c1="#ffd9c4", c2="#f5a98f", a="#b8507a", a2="#fff6df", cheek="#ff8f9a"),
        "aurora": dict(c0="#f4fff8", c1="#bff0dc", c2="#6fcfb2", a="#1f6878", a2="#f1e6ff", cheek="#c99af0"),
    },
}

STAGES = ["hum", "sprout", "buddy", "echo"]
# Sprout is a smaller, simpler baby; Buddy and Echo are the full pet.
SCALE = {"sprout": 0.74, "buddy": 0.9, "echo": 0.9}
# Small files: the crop that fits each stage at full scale (x, y, size).
SMALL_VIEW = {"hum": (16, 18, 96), "sprout": (2, 0, 124), "buddy": (2, 0, 124), "echo": (2, -2, 124)}
SW = 3.4  # Outline width of the full picture.
SW_SMALL = 6.0  # Thicker outline so the badge keeps its shape at 16 px.


class Pen:
    """Drawing settings for one file: palette, outline width and whether it is the small variant."""

    def __init__(self, p, small, sleep=False):
        self.p = p
        self.small = small
        self.sleep = sleep
        self.sw = SW_SMALL if small else SW

    def outline(self, factor=1.0):
        return f'stroke="{INK}" stroke-width="{self.sw * factor:.1f}" stroke-linejoin="round" stroke-linecap="round"'


def sparkle(x, y, r, colour):
    """A four-point star."""
    k = r * 0.28
    return (
        f'<path d="M{x} {y - r} Q{x + k} {y - k} {x + r} {y} Q{x + k} {y + k} {x} {y + r} '
        f'Q{x - k} {y + k} {x - r} {y} Q{x - k} {y - k} {x} {y - r}Z" fill="{colour}"/>'
    )


def eyes(pen, cx, cy, spread, kind):
    big = 1.35 if pen.small else 1.0
    out = ""
    if pen.sleep and kind != "sleepy":
        kind = "sleepy"
    for x in (cx - spread, cx + spread):
        if kind == "open":
            out += f'<ellipse cx="{x}" cy="{cy}" rx="{4.4 * big:.1f}" ry="{5.3 * big:.1f}" fill="{INK}"/>'
            out += f'<circle cx="{x + 1.5 * big:.1f}" cy="{cy - 2 * big:.1f}" r="{1.9 * big:.1f}" fill="{WHITE}"/>'
            if not pen.small:
                out += f'<circle cx="{x - 1.3}" cy="{cy + 1.9}" r="0.8" fill="{WHITE}"/>'
        else:
            # "happy" arcs up like ^, "sleepy" arcs down like a closed lid.
            lift = -4.5 if kind == "happy" else 3.5
            w = 4.4 * big
            out += (
                f'<path d="M{x - w:.1f} {cy} Q{x} {cy + lift * big:.1f} {x + w:.1f} {cy}" fill="none" '
                f'stroke="{INK}" stroke-width="{2.6 * big:.1f}" stroke-linecap="round"/>'
            )
    return out


def cheeks(pen, cx, cy, spread):
    return "".join(
        f'<ellipse cx="{x}" cy="{cy}" rx="5" ry="3" fill="{pen.p["cheek"]}" opacity="0.7"/>'
        for x in (cx - spread, cx + spread)
    )


def w_mouth(pen, cx, cy):
    w = 2.6 if pen.small else 2.2
    return (
        f'<path d="M{cx - 4.6} {cy} Q{cx - 2.3} {cy + 3.2} {cx} {cy} Q{cx + 2.3} {cy + 3.2} {cx + 4.6} {cy}" '
        f'fill="none" stroke="{INK}" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"/>'
    )


def face(pen, cx, cy, kind, spread=13, mouth=True):
    out = cheeks(pen, cx, cy + 8, spread + 5) + eyes(pen, cx, cy, spread, kind)
    if mouth:
        out += w_mouth(pen, cx, cy + 7)
    return out


def body_gradient(p, gid="body", cx=0.42, cy=0.36):
    return (
        f'<radialGradient id="{gid}" cx="{cx}" cy="{cy}" r="0.72">'
        f'<stop offset="0" stop-color="{p["c0"]}"/><stop offset="0.55" stop-color="{p["c1"]}"/>'
        f'<stop offset="1" stop-color="{p["c2"]}"/></radialGradient>'
    )


# --- Species. Each draws the pet at full scale, standing on y = 110, centred on x = 64. ---


def fizzle_tuft(pen):
    return (
        f'<path d="M64 42 C55 40 53 30 58 23 C60 27 62 27 63 22 C64 17 66 14 68 11 '
        f'C70 19 76 26 74 34 C73 39 69 42 64 42Z" fill="{pen.p["a"]}" {pen.outline()}/>'
    )


def fizzle(pen, full, kind):
    p = pen.p
    tail = (
        f'<linearGradient id="tail" x1="0" y1="1" x2="0" y2="0"><stop offset="0" stop-color="{p["c2"]}"/>'
        f'<stop offset="0.45" stop-color="{p["a2"]}"/><stop offset="1" stop-color="{p["a"]}"/></linearGradient>'
        f'<path d="M92 96 C106 94 114 82 112 62 C108 68 104 70 101 68 C105 60 103 54 98 50 '
        f'C98 64 93 72 90 80Z" fill="url(#tail)" {pen.outline()}/>'
        if full
        else ""
    )
    ears = (
        f'<path d="M38 62 L30 22 L60 44Z" fill="url(#body)" {pen.outline()}/>'
        f'<path d="M90 62 L98 22 L68 44Z" fill="url(#body)" {pen.outline()}/>'
    )
    if not pen.small:
        ears += f'<path d="M39 50 L35 31 L51 44Z" fill="{p["ear"]}"/><path d="M89 50 L93 31 L77 44Z" fill="{p["ear"]}"/>'
    feet = "".join(f'<ellipse cx="{x}" cy="106" rx="8" ry="5" fill="{p["c2"]}" {pen.outline(0.8)}/>' for x in (48, 80))
    body = (
        f'<path d="M64 36 C84 36 98 56 99 78 C100 96 88 107 64 107 C40 107 28 96 29 78 C30 56 44 36 64 36Z" '
        f'fill="url(#body)" {pen.outline()}/>'
    )
    belly = f'<path d="M44 88 Q64 110 84 88 Q64 98 44 88Z" fill="{p["a2"]}" opacity="0.85"/>'
    shine = "" if pen.small else sparkle(81, 64, 3.2, GOLD)
    return (
        tail + feet + ears + fizzle_tuft(pen) + body + belly
        + face(pen, 64, 72, "happy" if kind == "echo" else "open") + shine
    )


def burrble_fin(pen):
    return (
        f'<path d="M53 42 C55 34 59 35 61 38 C62 31 67 31 68 38 C70 34 75 35 75 42Z" '
        f'fill="{pen.p["fin"]}" {pen.outline()}/>'
    )


def burrble(pen, full, kind):
    p = pen.p
    lobes = [(37, 60, 30), (40, 72, 0), (37, 84, -30)] if full else [(37, 66, 20), (37, 80, -20)]
    fronds = "".join(
        f'<ellipse cx="{64 + side * dx}" cy="{y}" rx="10" ry="4.5" fill="{p["a"]}" '
        f'transform="rotate({side * a} {64 + side * dx} {y})" {pen.outline(0.75)}/>'
        for side in (1, -1)
        for dx, y, a in lobes
    )
    feet = "".join(f'<ellipse cx="{x}" cy="106" rx="6" ry="4" fill="{p["c2"]}" {pen.outline(0.8)}/>' for x in (52, 76))
    body = f'<circle cx="64" cy="73" r="35" fill="url(#body)" {pen.outline()}/>'
    shine = f'<ellipse cx="50" cy="53" rx="9" ry="5.5" fill="{WHITE}" opacity="0.7" transform="rotate(-28 50 53)"/>'
    if not pen.small:
        shine += f'<circle cx="41" cy="64" r="2.2" fill="{WHITE}" opacity="0.7"/>'
    belly = f'<ellipse cx="64" cy="95" rx="15" ry="7" fill="{p["a2"]}" opacity="0.9"/>'
    return fronds + feet + burrble_fin(pen) + body + shine + belly + face(pen, 64, 76, "happy")


def mossmo_sprout(pen):
    leaf = pen.p["a"]
    return (
        f'<path d="M64 53 C64 42 63 34 62 26" fill="none" stroke="{INK}" stroke-width="{pen.sw * 0.8:.1f}" stroke-linecap="round"/>'
        f'<path d="M62 30 C56 22 46 23 43 28 C49 33 57 33 62 30Z" fill="{leaf}"/>'
        f'<path d="M63 27 C68 18 80 16 85 20 C79 27 70 29 63 27Z" fill="{leaf}"/>'
    )


def mossmo(pen, full, kind):
    p = pen.p
    body = (
        f'<path d="M16 100 C16 70 36 52 64 52 C92 52 112 70 112 100 C112 106 107 108 101 108 '
        f'L27 108 C21 108 16 106 16 100Z" fill="url(#body)" {pen.outline()}/>'
    )
    spots = (
        ""
        if pen.small
        else "".join(
            f'<circle cx="{x}" cy="{y}" r="{r}" fill="{p["a2"]}" opacity="0.55"/>'
            for x, y, r in [(36, 72, 3.4), (46, 62, 2), (88, 64, 3), (98, 80, 2.2), (28, 86, 2.2), (78, 58, 1.8)]
        )
    )
    grass = f'<path d="M26 99 Q64 105 102 97" fill="none" stroke="{WHITE}" stroke-width="2.6" stroke-linecap="round" opacity="0.5"/>'
    mushroom = (
        f'<rect x="99" y="96" width="6" height="13" rx="2.5" fill="#fff5ea" {pen.outline(0.55)}/>'
        f'<path d="M92 99 C92 88 112 88 112 99Z" fill="#ff8a65" {pen.outline(0.55)}/>'
        + ("" if pen.small else f'<circle cx="98" cy="94" r="1.5" fill="{WHITE}"/><circle cx="105" cy="93" r="1.3" fill="{WHITE}"/>')
        if full
        else ""
    )
    return mossmo_sprout(pen) + body + spots + grass + face(pen, 64, 80, "open", spread=12) + mushroom


ZAP_PUFFS = [(40, 80, 19), (88, 80, 19), (48, 60, 18), (80, 60, 18), (64, 50, 18), (64, 90, 22), (50, 93, 16), (78, 93, 16)]


def zapling_spark(pen):
    return f'<path d="M66 22 L58 36 L65 35 L61 46 L71 31 L64 32Z" fill="{GOLD}" {pen.outline(0.5)}/>'


def zapling(pen, full, kind):
    p = pen.p
    bolt = (
        f'<path d="M95 72 L113 56 L106 71 L117 71 L99 92 L103 78 L93 79Z" fill="{p["a"]}" {pen.outline(0.7)}/>'
        f'<path d="M108 60 L104 70" stroke="{GOLD}" stroke-width="2" stroke-linecap="round"/>'
        if full
        else ""
    )
    feet = "".join(f'<ellipse cx="{x}" cy="110" rx="6" ry="3.5" fill="{p["c2"]}" {pen.outline(0.8)}/>' for x in (54, 74))
    # The outline is a thick stroke under all puffs, so the cloud gets one clean silhouette.
    under = "".join(f'<circle cx="{x}" cy="{y}" r="{r}" fill="{INK}" stroke="{INK}" stroke-width="{pen.sw * 2:.1f}"/>' for x, y, r in ZAP_PUFFS)
    puffs = "".join(f'<circle cx="{x}" cy="{y}" r="{r}" fill="url(#body)"/>' for x, y, r in ZAP_PUFFS)
    glow = (
        f'<radialGradient id="spot"><stop offset="0" stop-color="{p["a2"]}" stop-opacity="0.95"/>'
        f'<stop offset="1" stop-color="{p["a2"]}" stop-opacity="0"/></radialGradient>'
        + "".join(f'<circle cx="{x}" cy="{y}" r="{r}" fill="url(#spot)"/>' for x, y, r in [(50, 60, 11), (80, 58, 10), (40, 84, 9), (88, 84, 9), (64, 94, 11)])
    )
    shine = f'<ellipse cx="56" cy="44" rx="7" ry="4" fill="{WHITE}" opacity="0.8" transform="rotate(-20 56 44)"/>'
    stars = "" if pen.small else sparkle(47, 66, 3.2, GOLD) + sparkle(81, 66, 3.2, GOLD)
    return bolt + feet + (zapling_spark(pen) if full else "") + under + puffs + glow + shine + face(pen, 64, 74, "open", spread=12) + stars


def hushmoth_antennae(pen):
    a2 = pen.p["a2"]
    return (
        f'<path d="M56 42 C52 32 46 26 40 24" fill="none" stroke="{INK}" stroke-width="{pen.sw * 0.7:.1f}" stroke-linecap="round"/>'
        f'<path d="M72 42 C76 32 82 26 88 24" fill="none" stroke="{INK}" stroke-width="{pen.sw * 0.7:.1f}" stroke-linecap="round"/>'
        # Moon tips: a crescent made of two arcs.
        f'<path d="M41 17 A7 7 0 1 0 45 29 A5.5 5.5 0 1 1 41 17Z" fill="{a2}" {pen.outline(0.45)}/>'
        f'<path d="M87 17 A7 7 0 1 1 83 29 A5.5 5.5 0 1 0 87 17Z" fill="{a2}" {pen.outline(0.45)}/>'
    )


# The ruff's top edge: little points along a smile-shaped curve.
RUFF = " L".join(
    f"{x} {82 + 7 * (1 - ((x - 64) / 27) ** 2) - (4 if i % 2 else 0):.1f}" for i, x in enumerate(range(37, 92, 6))
)


def hushmoth(pen, full, kind):
    p = pen.p
    if full:
        wings = (
            f'<path d="M50 50 C38 28 8 30 7 56 C6 82 26 96 46 88Z" fill="{p["a"]}" {pen.outline()}/>'
            f'<path d="M78 50 C90 28 120 30 121 56 C122 82 102 96 82 88Z" fill="{p["a"]}" {pen.outline()}/>'
        )
        dots = [(20, 48), (108, 48)]
    else:
        wings = (
            f'<path d="M50 58 C42 46 24 46 24 62 C24 78 36 86 48 82Z" fill="{p["a"]}" {pen.outline()}/>'
            f'<path d="M78 58 C86 46 104 46 104 62 C104 78 92 86 80 82Z" fill="{p["a"]}" {pen.outline()}/>'
        )
        dots = [(34, 60), (94, 60)]
    if not pen.small:
        wings += "".join(f'<circle cx="{x}" cy="{y}" r="2.4" fill="{p["a2"]}"/>' for x, y in dots)
    feet = "".join(f'<ellipse cx="{x}" cy="107" rx="6" ry="4" fill="{p["c2"]}" {pen.outline(0.8)}/>' for x in (54, 74))
    body = (
        f'<path d="M64 40 C86 40 96 58 96 78 C96 98 82 108 64 108 C46 108 32 98 32 78 C32 58 42 40 64 40Z" '
        f'fill="url(#body)" {pen.outline()}/>'
    )
    ruff = (
        f'<path d="M{RUFF} C89 98 78 106 64 106 C50 106 39 98 37 84Z" fill="{p["a2"]}" {pen.outline(0.6)}/>'
    )
    mouth = f'<circle cx="64" cy="78" r="{1.8 if pen.small else 1.4}" fill="{INK}"/>'
    return hushmoth_antennae(pen) + wings + feet + body + ruff + face(pen, 64, 68, "sleepy", spread=12, mouth=False) + mouth


SPECIES = {"fizzle": fizzle, "burrble": burrble, "mossmo": mossmo, "zapling": zapling, "hushmoth": hushmoth}
# The species hint on top of a Hum, anchored at the top of the body (64, 42).
TOPS = {
    "fizzle": fizzle_tuft,
    "burrble": burrble_fin,
    "mossmo": mossmo_sprout,
    "zapling": zapling_spark,
    "hushmoth": hushmoth_antennae,
}


def hum(pen, species):
    """Stage 1: a glowing orb of sound with a hint of the species on top."""
    p = pen.p
    ring = f'<circle cx="64" cy="73" r="40" fill="{INK}" fill-opacity="0.35" stroke="{p["c1"]}" stroke-opacity="0.45" stroke-width="2"/>'
    top = f'<g transform="translate(64 44) scale(0.62) translate(-64 -44)">{TOPS[species](pen)}</g>'
    orb = (
        f'<path d="M64 40 C80 40 88 60 88 78 C88 96 78 106 64 106 C50 106 40 96 40 78 C40 60 48 40 64 40Z" '
        f'fill="url(#body)" {pen.outline()}/>'
    )
    waves = (
        f'<path d="M47 88 Q52.5 82 58 88 T69 88 T80 88" fill="none" stroke="{p["a2"]}" stroke-width="3" stroke-linecap="round" opacity="0.9"/>'
        f'<path d="M52 97 Q57.5 91 63 97 T74 97" fill="none" stroke="{p["a2"]}" stroke-width="3" stroke-linecap="round" opacity="0.9"/>'
    )
    smile = f'<path d="M61 76 Q64 79 67 76" fill="none" stroke="{INK}" stroke-width="2.2" stroke-linecap="round"/>'
    return ring + top + orb + waves + cheeks(pen, 64, 77, 14) + eyes(pen, 64, 68, 9, "sleepy") + smile


def aura(pen):
    ring = f'<circle cx="64" cy="70" r="55" fill="none" stroke="{GOLD}" stroke-width="{3.5 if pen.small else 2.4}" stroke-dasharray="0.1 {9 if pen.small else 6}" stroke-linecap="round"/>'
    if pen.small:
        return ring
    return ring + sparkle(20, 22, 8, GOLD) + sparkle(108, 18, 5, "#ff8fc4") + sparkle(114, 100, 4.5, GOLD) + sparkle(12, 96, 3.5, "#ff8fc4")


def zzz(pen, stage):
    """The "z z" of a sleeping pet, top right, inside the crop of every stage."""
    x, y, k = (84, 22, 0.85) if pen.small and stage == "hum" else (92, 4, 1.0)
    big = f"M{x} {y + 14 * k:.1f}h{11 * k:.1f}l{-11 * k:.1f} {11 * k:.1f}h{11 * k:.1f}"
    small = f"M{x + 15 * k:.1f} {y:.1f}h{8 * k:.1f}l{-8 * k:.1f} {8 * k:.1f}h{8 * k:.1f}"
    out = ""
    for d, w in ((big, 2.0), (small, 1.6)):
        out += f'<path d="{d}" fill="none" stroke="{INK}" stroke-width="{w * 2.6:.1f}" stroke-linejoin="round" stroke-linecap="round"/>'
        out += f'<path d="{d}" fill="none" stroke="{WHITE}" stroke-width="{w:.1f}" stroke-linejoin="round" stroke-linecap="round"/>'
    return out


def svg(species, stage, p, small, sleep=False):
    pen = Pen(p, small, sleep)
    defs = body_gradient(p)
    if not small:
        defs += (
            f'<radialGradient id="glow"><stop offset="0" stop-color="{p["c1"]}" stop-opacity="0.45"/>'
            f'<stop offset="1" stop-color="{p["c1"]}" stop-opacity="0"/></radialGradient>'
        )
    if stage == "hum":
        inner = hum(pen, species)
        scale = 1.0
    else:
        inner = SPECIES[species](pen, stage != "sprout", stage)
        scale = 1.0 if small else SCALE[stage]
        if scale != 1.0:
            inner = f'<g transform="translate(64 112) scale({scale}) translate(-64 -112)">{inner}</g>'
        if stage == "echo":
            inner = aura(pen) + inner
    if not small:
        back = f'<circle cx="64" cy="70" r="60" fill="url(#glow)"/>'
        back += f'<ellipse cx="64" cy="114" rx="{30 * scale:.1f}" ry="4.5" fill="{INK}" opacity="0.22"/>'
        inner = back + inner
        view = "0 0 128 128"
    else:
        x, y, s = SMALL_VIEW[stage]
        view = f"{x} {y} {s} {s}"
    if sleep:
        inner += zzz(pen, stage)
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{view}" width="128" height="128">'
        f"<defs>{defs}</defs>{inner}</svg>\n"
    )


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    WEB_OUT.mkdir(parents=True, exist_ok=True)
    for species, colours in PALETTES.items():
        for colour, p in colours.items():
            for stage in STAGES:
                (OUT / f"{species}-{stage}-{colour}.svg").write_text(svg(species, stage, p, False))
                small = svg(species, stage, p, True)
                (OUT / f"{species}-{stage}-{colour}-small.svg").write_text(small)
                (WEB_OUT / f"{species}-{stage}-{colour}-small.svg").write_text(small)
                (OUT / f"{species}-{stage}-{colour}-sleep.svg").write_text(svg(species, stage, p, False, True))
                (OUT / f"{species}-{stage}-{colour}-sleep-small.svg").write_text(svg(species, stage, p, True, True))


if __name__ == "__main__":
    main()
