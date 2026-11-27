#!/usr/bin/env python3
"""Build assets/AppIcon.icns from the icon artwork assets/icon/AppIcon-art.png.

Needs: pip install pillow numpy
The artwork is a full-bleed 1024 square: the cream keycap cat on the ink
ground. This script fits it to the macOS icon grid (an 824 px rounded square
inside the 1024 canvas, keycap scaled down so the ears clear the edge), clips
it to the squircle, adds the standard drop shadow and writes every size
straight into the .icns, so it runs on Linux too (no `iconutil`).

    python3 scripts/make-icon.py               # write assets/AppIcon.icns
    python3 scripts/make-icon.py preview.png   # also save the 1024 render

It also writes the Dock icons per theme (TH15), assets/icon/Dock-{dark,light,
miami}.png: the same keycap on the theme's backdrop, ⌘ in the theme colour.
The app sets one as the Dock icon while it runs (native::set_dock_theme).
"""
import io
import math
import struct
import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
ART = ROOT / "assets/icon/AppIcon-art.png"
OUT = ROOT / "assets/AppIcon.icns"

CANVAS = 1024
BODY = 824            # macOS icon grid: an 824x824 shape centered in 1024
KEYCAP_H = 600        # keycap height inside the shape (in the art: ~780 of 1024)
INK = (13, 14, 21, 255)

# (icns type, pixel size)
ENTRIES = [
    (b"icp4", 16), (b"ic11", 32),
    (b"icp5", 32), (b"ic12", 64),
    (b"ic07", 128), (b"ic13", 256),
    (b"ic08", 256), (b"ic14", 512),
    (b"ic09", 512), (b"ic10", 1024),
]


def squircle_mask(size: int, ss: int = 4) -> Image.Image:
    """Superellipse (n = 5), close to the macOS icon shape; supersampled for smooth edges."""
    big = size * ss
    r = big / 2
    pts = []
    for i in range(1440):
        t = 2 * math.pi * i / 1440
        c, s = math.cos(t), math.sin(t)
        pts.append((r + r * math.copysign(abs(c) ** 0.4, c), r + r * math.copysign(abs(s) ** 0.4, s)))
    m = Image.new("L", (big, big), 0)
    ImageDraw.Draw(m).polygon(pts, fill=255)
    return m.resize((size, size), Image.LANCZOS)


def content_box(art: Image.Image) -> tuple:
    """Bounding box of everything that is not the ink ground."""
    ground = Image.new("RGB", art.size, INK[:3])
    diff = ImageChops.difference(art.convert("RGB"), ground).convert("L").point(lambda v: 255 if v > 40 else 0)
    return diff.getbbox()


def master() -> Image.Image:
    art = Image.open(ART).convert("RGBA")
    x0, y0, x1, y1 = content_box(art)
    scale = KEYCAP_H / (y1 - y0)
    art = art.resize((round(art.width * scale), round(art.height * scale)), Image.LANCZOS)
    cx, cy = (x0 + x1) / 2 * scale, (y0 + y1) / 2 * scale
    shape = Image.new("RGBA", (BODY, BODY), INK)
    shape.alpha_composite(art, (round(BODY / 2 - cx), round(BODY / 2 - cy)))
    shape.putalpha(squircle_mask(BODY))
    out = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    off = (CANVAS - BODY) // 2
    out.alpha_composite(shape, (off, off))
    return out


def render(base: Image.Image, px: int) -> Image.Image:
    art = base.resize((px, px), Image.LANCZOS)
    # macOS template shadow: black 30%, y +10, blur 10 (in 1024 units).
    s = px / 1024
    alpha = art.getchannel("A").point(lambda a: int(a * 0.30))
    shadow = Image.new("RGBA", art.size, (0, 0, 0, 0))
    shadow.putalpha(alpha)
    shadow = shadow.transform(art.size, Image.AFFINE, (1, 0, 0, 0, 1, -10 * s))
    if px >= 64:
        shadow = shadow.filter(ImageFilter.GaussianBlur(10 * s))
    out = Image.new("RGBA", art.size, (0, 0, 0, 0))
    out.alpha_composite(shadow)
    out.alpha_composite(art)
    return out


def png_bytes(img: Image.Image) -> bytes:
    b = io.BytesIO()
    img.save(b, "PNG", optimize=True)
    return b.getvalue()


# Dock icon per theme (TH15): the ground's gradient stops (0 = top centre),
# and the colour of ⌘. Light's ground is deeper than the window's so the
# cream keycap reads, and its ⌘ darker than ACTION for contrast on cream.
DOCK = {
    "dark": ([(0, 0x22401D), (.35, 0x132610), (.7, 0x0A150A), (1, 0x070D07)], 0xC8F05A),
    "light": ([(0, 0xE6F0D8), (.55, 0xD3E4BE), (1, 0xBCD3A0)], 0x7DB519),
    "miami": ([(0, 0x0B1240), (.45, 0x2A1452), (.78, 0x5E1D57), (1, 0x70283C)], 0xFF5FA2),
}
DOCK_PX = 512
# A thin light rim inside the edge, so a dark tile still shows its outline
# on a dark Dock: (width, white alpha) in 1024 units. Dark Classic only.
DOCK_RIM = {"dark": (5, 0.22)}
CREAM = (242, 236, 229)


def dock_icon(theme: str) -> Image.Image:
    import numpy as np

    def rgb(h):
        return np.array([(h >> 16) & 255, (h >> 8) & 255, h & 255], float)

    stops, cmd = DOCK[theme]
    art = Image.open(ART).convert("RGBA")
    x0, y0, x1, y1 = content_box(art)
    scale = KEYCAP_H / (y1 - y0)
    art = art.resize((round(art.width * scale), round(art.height * scale)), Image.LANCZOS)
    cx, cy = (x0 + x1) / 2 * scale, (y0 + y1) / 2 * scale
    shape = Image.new("RGBA", (BODY, BODY), INK)
    shape.alpha_composite(art, (round(BODY / 2 - cx), round(BODY / 2 - cy)))
    base = np.asarray(shape.convert("RGB"), float)
    ink = np.array(INK[:3], float)
    # keycap coverage from lightness (the ground is near black), then the
    # keycap's own colour with the ground taken out of its edge
    a = np.clip((base.mean(axis=2) - 20) / 130, 0, 1)[..., None]
    key = np.where(a > 1e-3, (base - (1 - a) * ink) / np.maximum(a, 1e-3), np.array(CREAM, float))
    # ⌘ coverage: the orange is the only saturated part of the keycap
    chroma = key.max(axis=2) - key.min(axis=2)
    t = np.clip((chroma - 40) / 132, 0, 1)[..., None]
    key = key * (1 - t) + rgb(cmd) * t
    # the ground: Miami's dusk sky runs top to bottom, the others glow from the top centre
    y, x = np.mgrid[0:BODY, 0:BODY] / (BODY - 1)
    d = y if theme == "miami" else np.minimum(np.sqrt(((x - .5) / .95) ** 2 + (y / 1.15) ** 2), 1)
    ground = np.zeros((BODY, BODY, 3))
    for (t0, c0), (t1, c1) in zip(stops, stops[1:]):
        m = (d >= t0) & (d <= t1)
        k = ((d - t0) / (t1 - t0))[..., None]
        ground[m] = (rgb(c0) * (1 - k) + rgb(c1) * k)[m]
    img = Image.fromarray(np.clip(ground * (1 - a) + key * a, 0, 255).astype("uint8")).convert("RGBA")
    mask = squircle_mask(BODY)
    img.putalpha(mask)
    if theme in DOCK_RIM:
        width, alpha = DOCK_RIM[theme]
        inner = mask.filter(ImageFilter.MinFilter(2 * width + 1)).filter(ImageFilter.GaussianBlur(0.6))
        rim = ImageChops.subtract(mask, inner).point(lambda v: round(v * alpha))
        white = Image.new("RGBA", (BODY, BODY), (255, 255, 255, 0))
        white.putalpha(rim)
        img.alpha_composite(white)
    out = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    out.alpha_composite(img, ((CANVAS - BODY) // 2,) * 2)
    return render(out, CANVAS).resize((DOCK_PX, DOCK_PX), Image.LANCZOS)


def main() -> None:
    base = master()
    cache = {px: png_bytes(render(base, px)) for px in sorted({px for _, px in ENTRIES})}
    body = b"".join(t + struct.pack(">I", len(cache[px]) + 8) + cache[px] for t, px in ENTRIES)
    OUT.write_bytes(b"icns" + struct.pack(">I", len(body) + 8) + body)
    print(f"wrote {OUT.relative_to(ROOT)} ({OUT.stat().st_size // 1024} KB)")
    for theme in DOCK:
        path = ART.parent / f"Dock-{theme}.png"
        dock_icon(theme).save(path, optimize=True)
        print(f"wrote {path.relative_to(ROOT)} ({path.stat().st_size // 1024} KB)")
    if len(sys.argv) > 1:
        render(base, 1024).save(sys.argv[1])


if __name__ == "__main__":
    main()
