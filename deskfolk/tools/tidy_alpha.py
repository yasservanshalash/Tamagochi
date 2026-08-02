"""Repair bites and specks left in the sprites' alpha masks.

The art was keyed off a black background, and his darkest parts — the shoes,
the shadowed side of the beanbag — were close enough to that background that
the key ate into them. The damage is *not* enclosed holes (there are only two
in the whole set); it is notches chewed inward from the silhouette, plus stray
single pixels floating just outside it. On a dark desktop the notches are
filled by the wallpaper and invisible. On a light one, his heel has a bite out
of it and his sole breaks into fragments.

The rule is local and deliberately timid, because the previous attempt at
tidying this art was not: repainting every silhouette pixel destroyed anything
one or two pixels wide — his fingers, the mug handle, the edge of his beard.
So nothing here changes a pixel that is already part of the drawing:

  * A transparent pixel with at least `fill` of its 8 neighbours opaque is a
    notch, not a gap. It is filled with the most common colour among those
    neighbours. At 5/8 the gap between two fingers (~4 opaque neighbours) and
    the gap between the mug and his chest are both left alone — verified: the
    hands and mug come through the pass pixel-identical.
  * An opaque pixel with at most `speck` opaque neighbours is a leftover
    fleck of keyed background. It goes.

Three passes, so a notch three pixels deep closes. Going further (4/8) starts
inventing silhouette rather than restoring it — the beanbag visibly fattens.

Usage:
  python tools/tidy_alpha.py --check
  python tools/tidy_alpha.py --preview
  python tools/tidy_alpha.py
"""
import sys
from collections import Counter
from pathlib import Path

from PIL import Image

SPRITES = Path(__file__).resolve().parents[1] / "characters" / "yasser" / "sprites"
PREVIEW = Path(__file__).resolve().parents[1] / "alpha-tidy.png"
YELLOW = (255, 255, 82)

N8 = ((1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1))


def neighbours(px, w, h, x, y):
    out = []
    for dx, dy in N8:
        nx, ny = x + dx, y + dy
        if 0 <= nx < w and 0 <= ny < h and px[nx, ny][3] > 0:
            out.append(px[nx, ny])
    return out


def tidy(im: Image.Image, passes=3, fill=5, speck=1):
    im = im.convert("RGBA").copy()
    w, h = im.size
    px = im.load()
    filled = removed = 0

    for _ in range(passes):
        # Collect first, then apply, so a pixel changed early in the sweep
        # cannot cascade into its neighbours within the same pass.
        to_fill = []
        to_clear = []
        for y in range(h):
            for x in range(w):
                near = neighbours(px, w, h, x, y)
                if px[x, y][3] == 0:
                    if len(near) >= fill:
                        colour = Counter(n[:3] for n in near).most_common(1)[0][0]
                        to_fill.append((x, y, colour))
                elif len(near) <= speck:
                    to_clear.append((x, y))

        for x, y, colour in to_fill:
            px[x, y] = (*colour, 255)
        for x, y in to_clear:
            px[x, y] = (0, 0, 0, 0)
        filled += len(to_fill)
        removed += len(to_clear)
        if not to_fill and not to_clear:
            break

    return im, filled, removed


def over(im, bg=YELLOW):
    flat = Image.new("RGB", im.size, bg)
    flat.paste(im, (0, 0), im)
    return flat


def preview():
    """His feet, before and after, on a colour the damage cannot hide on."""
    src = Image.open(SPRITES / "img_y_sitb.png").convert("RGBA")
    fixed, _, _ = tidy(src)
    box = (0, 150, 130, 238)
    z = 6
    a = over(src).crop(box)
    b = over(fixed).crop(box)
    a = a.resize((a.width * z, a.height * z), Image.NEAREST)
    b = b.resize((b.width * z, b.height * z), Image.NEAREST)
    sheet = Image.new("RGB", (a.width + b.width + 24, a.height + 16), YELLOW)
    sheet.paste(a, (8, 8))
    sheet.paste(b, (a.width + 16, 8))
    sheet.save(PREVIEW)
    print(f"wrote {PREVIEW}  (left = now, right = tidied)")


def main() -> int:
    if not SPRITES.is_dir():
        print(f"no sprites at {SPRITES}")
        return 1
    args = sys.argv[1:]
    if "--preview" in args:
        preview()
        return 0

    check = "--check" in args
    tf = tr = touched = 0
    for path in sorted(SPRITES.glob("*.png")):
        fixed, filled, removed = tidy(Image.open(path))
        if filled or removed:
            touched += 1
            tf += filled
            tr += removed
            if not check:
                fixed.save(path)
    verb = "would fill" if check else "filled"
    print(f"{verb} {tf} notch pixel(s), removed {tr} speck(s), across {touched} sprite(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
