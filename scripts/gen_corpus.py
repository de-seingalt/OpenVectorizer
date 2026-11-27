"""Generate the project's own ground-truth SVGs (MIT, like the code):
lettering with the OFL fonts of testdata/fonts, pixel-art sprites, and a chart.

  python3 -I scripts/gen_corpus.py testdata
"""

import pathlib
import sys

FONT_NOTE = "<!-- Rendered with the fonts in testdata/fonts (OFL). -->"


def svg(w: int, h: int, body: str) -> str:
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">{FONT_NOTE}\n{body}\n</svg>\n'


LETTERING = {
    # Script logo on a colored background (café / bakery signage).
    "script-cafe": svg(
        640,
        220,
        '<rect width="640" height="220" fill="#f6efe2"/>'
        '<text x="320" y="135" text-anchor="middle" font-family="Pacifico" font-size="84" fill="#5b3a29">Café Lumière</text>'
        '<path d="M170 168 Q320 196 470 168" fill="none" stroke="#c0843d" stroke-width="5" stroke-linecap="round"/>',
    ),
    # Handwritten note: thin, irregular strokes.
    "handwritten-note": svg(
        560,
        300,
        '<rect width="560" height="300" fill="#ffffff"/>'
        '<text font-family="Caveat" font-size="58" fill="#1d3f8f">'
        '<tspan x="36" y="96">Merci pour tout !</tspan><tspan x="36" y="170">On se voit jeudi,</tspan>'
        '<tspan x="200" y="244">— Camille</tspan></text>',
    ),
    # Condensed capitals on a sign with a frame.
    "condensed-sign": svg(
        640,
        200,
        '<rect width="640" height="200" fill="#ffffff"/>'
        '<rect x="12" y="12" width="616" height="176" rx="14" fill="#1f4d3a"/>'
        '<rect x="26" y="26" width="588" height="148" rx="8" fill="none" stroke="#e8d9a8" stroke-width="4"/>'
        '<text x="320" y="132" text-anchor="middle" font-family="Bebas Neue" font-size="104" letter-spacing="6" fill="#e8d9a8">BOULANGERIE</text>',
    ),
    # High-contrast serif headline: hairlines next to heavy stems.
    "serif-headline": svg(
        640,
        180,
        '<rect width="640" height="180" fill="#ffffff"/>'
        '<text x="320" y="118" text-anchor="middle" font-family="Abril Fatface" font-size="86" fill="#111111">Le Grand Hôtel</text>',
    ),
    # Small text: the hardest case for tracers.
    "small-text": svg(
        480,
        160,
        '<rect width="480" height="160" fill="#ffffff"/>'
        '<text x="16" y="40" font-family="Abril Fatface" font-size="22" fill="#222">Établi en 1923 · Lyon</text>'
        '<text x="16" y="80" font-family="Bebas Neue" font-size="18" letter-spacing="2" fill="#a3342b">QUALITÉ ARTISANALE — DEPUIS TOUJOURS</text>'
        '<text x="16" y="124" font-family="Caveat" font-size="26" fill="#1d3f8f">fait main, avec amour</text>',
    ),
    # Emblem: icon + curved and straight text, two colors.
    "emblem-badge": svg(
        360,
        360,
        '<rect width="360" height="360" fill="#ffffff"/>'
        '<circle cx="180" cy="180" r="160" fill="#0e3b5c"/><circle cx="180" cy="180" r="140" fill="none" stroke="#f2b134" stroke-width="4"/>'
        '<path id="arc" d="M60 190 A120 120 0 0 1 300 190" fill="none"/>'
        '<text font-family="Bebas Neue" font-size="40" letter-spacing="4" fill="#f2b134"><textPath href="#arc" startOffset="50%" text-anchor="middle">ATELIER NAVAL</textPath></text>'
        '<path d="M130 230 L180 150 L230 230 Z" fill="#f2b134"/><path d="M120 240 H240 L225 262 H135 Z" fill="#ffffff"/>'
        '<text x="180" y="300" text-anchor="middle" font-family="Abril Fatface" font-size="26" fill="#ffffff">1898</text>',
    ),
}

# Pixel art: '.' = transparent, other chars index PALETTE. Exact squares are the ground truth.
SPRITES = {
    "heart": (
        {"k": "#1a1a1a", "r": "#e03131", "p": "#ff8787", "w": "#ffffff"},
        [
            "...kk...kk...",
            "..krrk.krrk..",
            ".krwprkrrrrk.",
            ".kwprrrrrrrk.",
            ".kprrrrrrrrk.",
            "..krrrrrrrk..",
            "...krrrrrk...",
            "....krrrk....",
            ".....krk.....",
            "......k......",
        ],
    ),
    "mushroom": (
        {"k": "#2b2b2b", "r": "#c92a2a", "w": "#f8f9fa", "s": "#f1d9b5", "e": "#2b2b2b"},
        [
            "....kkkkk....",
            "..kkrrwrrkk..",
            ".krrrwwwrrrk.",
            ".kwrrrwrrrwk.",
            "kwwwrrrrrwwwk",
            "krrrrrwrrrrrk",
            "kkkkkkkkkkkkk",
            "...ksesesk...",
            "...kssssk....",
            "....kkkk.....",
        ],
    ),
    "potion": (
        {"k": "#212529", "g": "#adb5bd", "b": "#4dabf7", "d": "#1c7ed6", "w": "#e7f5ff", "c": "#8d6e63"},
        [
            "....kkk....",
            "....kck....",
            "....kck....",
            "...kgggk...",
            "..kgwbbgk..",
            ".kgwbbbbgk.",
            ".kgbbbbdgk.",
            ".kgbbbddgk.",
            ".kgbdddddgk",
            "..kgggggk..",
            "...kkkkk...",
        ],
    ),
}


def sprite_svg(palette: dict, rows: list[str], cell: int = 20, bg: str = "#ffffff") -> str:
    w = max(len(r) for r in rows) * cell + 2 * cell
    h = len(rows) * cell + 2 * cell
    rects = [f'<rect width="{w}" height="{h}" fill="{bg}"/>']
    for y, row in enumerate(rows):
        for x, ch in enumerate(row):
            if ch != ".":
                rects.append(f'<rect x="{(x + 1) * cell}" y="{(y + 1) * cell}" width="{cell}" height="{cell}" fill="{palette[ch]}"/>')
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" shape-rendering="crispEdges">\n' + "\n".join(rects) + "\n</svg>\n"


CHART = svg(
    480,
    320,
    '<rect width="480" height="320" fill="#ffffff"/>'
    '<path d="M40 20 V280 H300" fill="none" stroke="#495057" stroke-width="2"/>'
    + "".join(f'<path d="M40 {y} H300" stroke="#dee2e6" stroke-width="1"/>' for y in (80, 140, 200))
    + '<rect x="60" y="150" width="40" height="130" fill="#4263eb"/><rect x="120" y="90" width="40" height="190" fill="#4263eb"/>'
    '<rect x="180" y="200" width="40" height="80" fill="#4263eb"/><rect x="240" y="60" width="40" height="220" fill="#f76707"/>'
    '<circle cx="390" cy="150" r="70" fill="#4263eb"/>'
    '<path d="M390 150 L390 80 A70 70 0 0 1 456.6 171.6 Z" fill="#f76707"/>'
    '<path d="M390 150 L456.6 171.6 A70 70 0 0 1 362 214.2 Z" fill="#37b24d"/>',
).replace(FONT_NOTE, "")


def main() -> None:
    root = pathlib.Path(sys.argv[1])
    out = root / "corpus" / "lettering"
    out.mkdir(parents=True, exist_ok=True)
    for name, content in LETTERING.items():
        (out / f"{name}.svg").write_text(content, encoding="utf-8")
    out = root / "corpus" / "pixelart"
    out.mkdir(parents=True, exist_ok=True)
    for name, (palette, rows) in SPRITES.items():
        (out / f"{name}.svg").write_text(sprite_svg(palette, rows), encoding="utf-8")
    (root / "synthetic" / "07-chart.svg").write_text(CHART, encoding="utf-8")
    print(f"generated {len(LETTERING)} lettering, {len(SPRITES)} pixel-art, 1 chart")


if __name__ == "__main__":
    main()
