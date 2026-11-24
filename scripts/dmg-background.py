#!/usr/bin/env python3
"""Build assets/dmg/background{,@2x}.png from the DMG artwork.

    python3 scripts/dmg-background.py path/to/art.png   (3:2, e.g. 3072x2048)

Finder draws the icon labels in black on any picture background, in light
and dark mode alike, so the app and the Applications alias each sit on a
light card that holds the icon and its label. Card centres follow the icon
places in scripts/dmg-settings.py.
"""
import sys
from pathlib import Path
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
W, H = 660, 440
ICONS = [(150, 180), (505, 180)]   # icon centres, points (dmg-settings.py)
CARD_W, CARD_TOP, CARD_BOTTOM = 160, 96, 286   # card spans icon + label
RADIUS = 22
CARD = (0xEE, 0xF3, 0xE6, 230)     # light moss ink, near opaque
EDGE = (0xFF, 0xFF, 0xFF, 150)


def build(art, scale):
    img = art.resize((W * scale, H * scale), Image.LANCZOS).convert("RGBA")
    ss = 4
    layer = Image.new("RGBA", (W * scale * ss, H * scale * ss), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    k = scale * ss
    for cx, cy in ICONS:
        box = [(cx - CARD_W / 2) * k, (cy - 180 + CARD_TOP) * k, (cx + CARD_W / 2) * k, (cy - 180 + CARD_BOTTOM) * k]
        d.rounded_rectangle(box, radius=RADIUS * k, fill=CARD, outline=EDGE, width=k)
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
