"""Give the silhouette a soft edge.

The art has binary alpha — every pixel is either fully there or fully gone —
because that is all the Watcher's blitter could do and all the old webview
companion could composite. Against a dark desktop it passes. Against a light
one, a hard cut on a curved shoulder is a staircase, and it reads as a bad
cutout no matter how clean the mask underneath is.

The native renderer composites true per-pixel alpha now, so that constraint is
gone. This adds one ring of partial coverage just outside the silhouette:
coverage from how much of the pixel's neighbourhood is solid, colour from the
character rather than from black — so the feather is *his* edge colour fading
out, not a grey halo.

Deliberately one ring and no more. Two would visibly bloat him, and the point
is to take the staircase off the edge, not to blur the drawing.

Idempotent: it only feathers fully-transparent pixels that touch fully-opaque
ones, so the ring it just created is never itself a seed for another ring.

Usage:
  python tools/feather_edges.py --check
  python tools/feather_edges.py --preview
  python tools/feather_edges.py
"""
import sys
from collections import Counter
from pathlib import Path

from PIL import Image

SPRITES = Path(__file__).resolve().parents[1] / "characters" / "yasser" / "sprites"
PREVIEW = Path(__file__).resolve().parents[1] / "feather.png"
YELLOW = (255, 255, 82)

N8 = ((1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1))
# Straight edges expose 3 of 8 neighbours, outer corners 1, inner corners 5.
# Scaling that fraction gives a ramp that tracks the local shape without
# needing a distance field.
STRENGTH = 0.95


def feather(im: Image.Image):
    im = im.convert("RGBA").copy()
    w, h = im.size
    px = im.load()
    added = []
    for y in range(h):
        for x in range(w):
            if px[x, y][3] != 0:
                continue
            solid = []
            for dx, dy in N8:
                nx, ny = x + dx, y + dy
                if 0 <= nx < w and 0 <= ny < h and px[nx, ny][3] == 255:
                    solid.append(px[nx, ny])
            if not solid:
                continue
            coverage = min(1.0, len(solid) / 8.0 * STRENGTH)
            colour = Counter(s[:3] for s in solid).most_common(1)[0][0]
            added.append((x, y, colour, round(coverage * 255)))
    for x, y, colour, a in added:
        px[x, y] = (*colour, a)
    return im, len(added)


def over(im, bg=YELLOW):
    flat = Image.new("RGB", im.size, bg)
    flat.paste(im, (0, 0), im)
    return flat


def preview():
    src = Image.open(SPRITES / "img_y_sitmug.png").convert("RGBA")
    soft, _ = feather(src)
    box = (150, 20, 260, 110)  # his head and the curve of the beanie
    z = 6
    pairs = []
    for bg in (YELLOW, (24, 24, 28)):
        a = over(src, bg).crop(box)
        b = over(soft, bg).crop(box)
        pairs.append(
            (
                a.resize((a.width * z, a.height * z), Image.NEAREST),
                b.resize((b.width * z, b.height * z), Image.NEAREST),
            )
        )
    cw = pairs[0][0].width * 2 + 24
    sheet = Image.new("RGB", (cw, pairs[0][0].height * 2 + 24), (90, 90, 90))
    for row, (a, b) in enumerate(pairs):
        sheet.paste(a, (8, 8 + row * (a.height + 8)))
        sheet.paste(b, (a.width + 16, 8 + row * (a.height + 8)))
    sheet.save(PREVIEW)
    print(f"wrote {PREVIEW}  (left = hard, right = feathered; light row, dark row)")


def main() -> int:
    if not SPRITES.is_dir():
        print(f"no sprites at {SPRITES}")
        return 1
    args = sys.argv[1:]
    if "--preview" in args:
        preview()
        return 0

    check = "--check" in args
    total = touched = 0
    for path in sorted(SPRITES.glob("*.png")):
        soft, n = feather(Image.open(path))
        if n:
            touched += 1
            total += n
            if not check:
                soft.save(path)
    verb = "would soften" if check else "softened"
    print(f"{verb} {total} edge pixel(s) across {touched} sprite(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
