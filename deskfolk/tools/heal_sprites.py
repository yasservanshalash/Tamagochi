"""Repair dropped scanlines in a character package's sprites.

`img_y_sitb.png` and `img_y_sitc.png` carry a single fully-transparent row
(y=133) straight through his torso. It is not a rendering bug and never was: the
row is transparent in the original Watcher art (`img_y_sitb.c`), so it has been
there since the firmware. On the Watcher's black screen, and on a dark desktop,
a one-pixel transparent line through a dark hoodie is invisible. Put him on a
yellow wallpaper and it is a bright slash across his chest.

The rule here is deliberately narrow, because "interior transparent row" is a
*legitimate* thing in this package — `img_fx_anger`, `img_fx_bangq`,
`img_fx_spark_l` and the static sheets are all several glyphs stacked with real
gaps between them, and healing those would fuse shapes that are meant to be
separate. So a row is only healed when it is:

  * a run of exactly one row (a dropout, not a gap),
  * bordered above and below by rows that are >=80% opaque (mid-body, not an
    edge), and
  * in a character sprite (`img_y_*`), not an effect sheet.

The healed row is copied from the row above, which for pixel art at this scale
is indistinguishable from what the artist drew.

Usage: python tools/heal_sprites.py [--check]
"""
import sys
from pathlib import Path

from PIL import Image

SPRITES = Path(__file__).resolve().parents[1] / "characters" / "yasser" / "sprites"
SOLID = 0.80


def blank_rows(px, w, h):
    return [y for y in range(h) if all(px[x, y][3] == 0 for x in range(w))]


def opacity(px, w, y):
    return sum(1 for x in range(w) if px[x, y][3] > 0) / w


def heal(path: Path, check: bool) -> list[int]:
    im = Image.open(path).convert("RGBA")
    w, h = im.size
    px = im.load()
    blanks = set(blank_rows(px, w, h))

    healed = []
    for y in sorted(blanks):
        if y == 0 or y == h - 1:
            continue
        # A run of exactly one: a neighbouring blank row means it is a gap
        # between shapes, not a dropped scanline.
        if (y - 1) in blanks or (y + 1) in blanks:
            continue
        if opacity(px, w, y - 1) < SOLID or opacity(px, w, y + 1) < SOLID:
            continue
        healed.append(y)
        if not check:
            for x in range(w):
                px[x, y] = px[x, y - 1]

    if healed and not check:
        im.save(path)
    return healed


def main() -> int:
    check = "--check" in sys.argv
    if not SPRITES.is_dir():
        print(f"no sprites at {SPRITES}")
        return 1

    total = 0
    for path in sorted(SPRITES.glob("img_y_*.png")):
        rows = heal(path, check)
        if rows:
            total += len(rows)
            verb = "would heal" if check else "healed"
            print(f"{verb} {path.name}: row(s) {rows}")

    if total == 0:
        print("no dropped scanlines found")
    elif check:
        print(f"\n{total} dropped scanline(s) — run without --check to repair")
        return 1
    else:
        print(f"\nrepaired {total} scanline(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
