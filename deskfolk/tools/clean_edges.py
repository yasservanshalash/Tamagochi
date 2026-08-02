"""Give every sprite a clean silhouette outline.

The art was cut out of a black background and the cut is dirty. Measured on
`img_y_expr0`: the outermost opaque ring has a median luma of 12.6, but only
about half of it is actually black — the rest are mid-tone *blends* between the
character and the background behind him, anti-aliased pixels flattened to full
opacity when the alpha was made binary.

On the Watcher's black screen those blends are invisible, because they are the
background. On a yellow wallpaper they are a grey halo, and they are why the
whole thing reads as a bad background removal.

The fix is not to soften the edge — pixel art wants a hard one, and the
renderer scales nearest-neighbour. The fix is to make the edge *deliberate*:
every pixel on the silhouette becomes the outline colour, so the ragged
half-black-half-grey ring becomes the 1px keyline the art should have had.

Nothing inside the silhouette is touched, so the shading, the 3,595 colours and
the character are all exactly as drawn.

Usage:
  python tools/clean_edges.py --check      # report, change nothing
  python tools/clean_edges.py --preview    # write a before/after over yellow
  python tools/clean_edges.py              # apply
"""
import sys
from pathlib import Path

from PIL import Image

SPRITES = Path(__file__).resolve().parents[1] / "characters" / "yasser" / "sprites"
PREVIEW = Path(__file__).resolve().parents[1] / "sprite-edges.png"

# The darkest colour already in the art. Using a colour from the palette rather
# than pure black keeps the outline warm, in line with everything else.
OUTLINE = (16, 12, 8, 255)

# A pixel this close to the outline colour already is left alone, so the count
# reports real changes rather than no-ops.
NEAR = 24


def edge_pixels(px, w, h):
    """Opaque pixels that touch transparency — the silhouette, one deep.

    The canvas border is deliberately *not* an edge. Every sprite is trimmed
    tight to the ink, so the art runs right up to the border on all four sides;
    outlining there draws a straight keyline along the crop and makes the
    bounding box visible — a hard line under his feet and down his side, which
    is worse than the halo it replaced.
    """
    out = []
    for y in range(h):
        for x in range(w):
            if px[x, y][3] == 0:
                continue
            for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                nx, ny = x + dx, y + dy
                if 0 <= nx < w and 0 <= ny < h and px[nx, ny][3] == 0:
                    out.append((x, y))
                    break
    return out


def clean(im: Image.Image) -> tuple[Image.Image, int]:
    im = im.convert("RGBA").copy()
    w, h = im.size
    px = im.load()
    changed = 0
    for x, y in edge_pixels(px, w, h):
        r, g, b, _ = px[x, y]
        if abs(r - OUTLINE[0]) + abs(g - OUTLINE[1]) + abs(b - OUTLINE[2]) > NEAR:
            changed += 1
        px[x, y] = OUTLINE
    return im, changed


def over(im: Image.Image, bg) -> Image.Image:
    flat = Image.new("RGB", im.size, bg)
    flat.paste(im, (0, 0), im)
    return flat


def preview(names: list[str]) -> None:
    """Before and after, composited over a colour the halo cannot hide on."""
    yellow = (255, 255, 82)
    pairs = []
    for name in names:
        src = Image.open(SPRITES / name).convert("RGBA")
        pairs.append((over(src, yellow), over(clean(src)[0], yellow)))

    pad = 12
    w = sum(a.width + b.width + pad * 3 for a, b in pairs)
    h = max(max(a.height, b.height) for a, b in pairs) + pad * 2
    sheet = Image.new("RGB", (w, h), yellow)
    x = pad
    for a, b in pairs:
        sheet.paste(a, (x, pad))
        x += a.width + pad
        sheet.paste(b, (x, pad))
        x += b.width + pad * 2
    sheet = sheet.resize((sheet.width * 2, sheet.height * 2), Image.NEAREST)
    sheet.save(PREVIEW)
    print(f"wrote {PREVIEW}  (left = now, right = cleaned, on wallpaper yellow)")


def main() -> int:
    if not SPRITES.is_dir():
        print(f"no sprites at {SPRITES}")
        return 1

    args = sys.argv[1:]
    if "--preview" in args:
        preview(["img_y_sitmug.png", "img_y_expr0.png", "img_y_act0.png"])
        return 0

    check = "--check" in args
    total = 0
    touched = 0
    for path in sorted(SPRITES.glob("*.png")):
        im = Image.open(path)
        cleaned, changed = clean(im)
        if changed:
            touched += 1
            total += changed
            if not check:
                cleaned.save(path)
    verb = "would repaint" if check else "repainted"
    print(f"{verb} {total} silhouette pixels across {touched} sprite(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
