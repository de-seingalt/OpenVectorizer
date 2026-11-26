"""Normalize downloaded SVGs for the benchmark: set width/height on the root element,
optionally replace currentColor and add a background rectangle.

Usage: normalize_svg.py SRC_DIR DST_DIR WIDTH HEIGHT [--stroke COLOR] [--background COLOR]
"""

import argparse
import pathlib
import re


def normalize(text: str, width: int, height: int, stroke: str | None, background: str | None) -> str:
    match = re.search(r"<svg\b[^>]*>", text)
    if not match:
        raise ValueError("no <svg> root element")
    tag = match.group(0)
    new_tag = re.sub(r'\s(width|height)="[^"]*"', "", tag)
    new_tag = new_tag.replace("<svg", f'<svg width="{width}" height="{height}"', 1)
    if "viewBox" not in new_tag:
        raise ValueError("root element has no viewBox")
    if background:
        vb = re.search(r'viewBox="([^"]+)"', new_tag).group(1).replace(",", " ").split()
        x, y, w, h = vb
        new_tag += f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{background}" stroke="none"/>'
    text = text[: match.start()] + new_tag + text[match.end() :]
    if stroke:
        text = text.replace("currentColor", stroke)
    return text


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("src", type=pathlib.Path)
    p.add_argument("dst", type=pathlib.Path)
    p.add_argument("width", type=int)
    p.add_argument("height", type=int)
    p.add_argument("--stroke")
    p.add_argument("--background")
    a = p.parse_args()
    a.dst.mkdir(parents=True, exist_ok=True)
    for f in sorted(a.src.glob("*.svg")):
        out = normalize(f.read_text(encoding="utf-8"), a.width, a.height, a.stroke, a.background)
        (a.dst / f.name).write_text(out, encoding="utf-8")


if __name__ == "__main__":
    main()
