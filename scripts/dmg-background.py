#!/usr/bin/env python3
"""Build assets/dmg/background{,@2x}.png from the DMG artwork.

    python3 scripts/dmg-background.py path/to/art.png   (3:2, e.g. 3072x2048)

Finder draws the icon labels in black on any picture background, in light
and dark mode alike, so each label gets a light pill under it. Pill centres
follow the icon places in scripts/dmg-settings.py (label = icon y + 83 pt).
"""
import sys
from pathlib import Path
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
W, H = 660, 440
LABELS = [(150, 263), (505, 263)]  # label centres, points
PILL_W, PILL_H = 116, 22
PILL = (0xEE, 0xF3, 0xE6, 235)     # light moss ink, near opaque


def build(art, scale):
    img = art.resize((W * scale, H * scale), Image.LANCZOS).convert("RGBA")
    ss = 4
    layer = Image.new("RGBA", (W * scale * ss, H * scale * ss), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    k = scale * ss
    for cx, cy in LABELS:
        box = [(cx - PILL_W / 2) * k, (cy - PILL_H / 2) * k, (cx + PILL_W / 2) * k, (cy + PILL_H / 2) * k]
        d.rounded_rectangle(box, radius=PILL_H / 2 * k, fill=PILL)
    layer = layer.resize(img.size, Image.LANCZOS)
    return Image.alpha_composite(img, layer).convert("RGB")


def main():
    art = Image.open(sys.argv[1]).convert("RGB")
    out = ROOT / "assets/dmg"
    for scale, name in ((1, "background.png"), (2, "background@2x.png")):
        build(art, scale).save(out / name, optimize=True)
        print(out / name)


if __name__ == "__main__":
    main()
