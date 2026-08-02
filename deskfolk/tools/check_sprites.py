"""Check sprites against the rules in SPRITE-BRIEF.md.

Run this on new art before wiring it into a character package. Every rule here
was measured from the existing 54 sprites, so passing means the frame will sit
correctly next to the rest instead of looking subtly wrong on the desktop.

    python tools/check_sprites.py                       # check everything
    python tools/check_sprites.py img_y_look_l.png ...  # check specific files
"""
import sys
from pathlib import Path

try:
    from PIL import Image
except ImportError:
    sys.exit("needs Pillow:  pip install pillow")

SPRITES = Path(__file__).resolve().parent.parent / "characters" / "yasser" / "sprites"

# Heights the existing set uses. A new frame that matches neither will sit at
# the wrong scale beside the others.
FULL_BODY_H = 238
BUST_H = 176
KNOWN_HEIGHTS = {FULL_BODY_H, BUST_H, 182}   # 182 = the sleeping pose


def kind(name: str) -> str:
    """Sprites fall into three classes with genuinely different rules.

    Character frames are the strict ones. FX overlays (hearts, zzz, notes) are
    small and any size. The portal backdrop is meant to be opaque.
    """
    if name.startswith("img_y_"):
        return "character"
    if name.startswith("img_fx_"):
        return "fx"
    return "background"


def check(path: Path):
    """Return (errors, warnings) for one sprite."""
    errors, warnings = [], []
    cls = kind(path.stem)
    try:
        im = Image.open(path)
    except Exception as e:
        return [f"cannot open: {e}"], []

    if im.mode != "RGBA":
        errors.append(f"mode is {im.mode}, must be RGBA "
                      f"(export as PNG-32 — never a solid or green background)")
        im = im.convert("RGBA")

    alpha = im.getchannel("A")
    lo, hi = alpha.getextrema()

    if lo == 255 and cls != "background":
        errors.append("fully opaque — no transparency at all. The background "
                      "must be erased, not filled with a colour to key out")

    # Partial alpha used to be banned outright, because the webview companion
    # could not composite it and a soft edge came out as a halo. The native
    # renderer premultiplies and composites per pixel, so a feathered
    # silhouette is now correct — and on a light wallpaper it is the
    # difference between a curve and a staircase.
    #
    # What is still wrong is partial alpha *away* from the silhouette: a
    # half-transparent layer left switched on, or a matte baked into the
    # middle of him. So the rule is about where it is, not whether it exists.
    hist = alpha.histogram()
    partial = sum(hist[1:255])
    if partial:
        px = im.load()
        w, h = im.size
        stray = 0
        for y in range(h):
            for x in range(w):
                a = px[x, y][3]
                if a in (0, 255):
                    continue
                touches_solid = any(
                    0 <= x + dx < w and 0 <= y + dy < h and px[x + dx, y + dy][3] == 255
                    for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1),
                                   (1, 1), (1, -1), (-1, 1), (-1, -1))
                )
                if not touches_solid:
                    stray += 1
        if stray:
            pct = 100 * stray / (im.width * im.height)
            errors.append(f"{stray} half-transparent pixels ({pct:.2f}%) away from "
                          f"the silhouette — a see-through layer or a baked-in "
                          f"matte, not an edge")

    bbox = alpha.getbbox()
    if bbox is None:
        return errors + ["completely blank"], warnings

    # Tight crop: the bottom row is the contact point the engine anchors to.
    bottom_gap = im.height - bbox[3]
    if bottom_gap and cls == "character":
        errors.append(f"{bottom_gap}px of empty space below the artwork — trim "
                      f"tight; the bottom row must be his contact point or he "
                      f"will float")
    for name, gap in (("left", bbox[0]), ("right", im.width - bbox[2])):
        if gap and cls == "character":
            warnings.append(f"{gap}px empty on the {name} — trim tight so "
                            f"bottom-centre anchoring stays honest")
    if bbox[1] > 0 and cls == "character":
        warnings.append(f"{bbox[1]}px empty on top (fine, but usually trimmed)")

    if cls == "character" and im.height not in KNOWN_HEIGHTS:
        errors.append(f"height {im.height}px — expected {FULL_BODY_H} "
                      f"(full body) or {BUST_H} (bust). A different height "
                      f"means a different scale from the rest of the set")

    if cls == "character" and im.width > 300:
        warnings.append(f"width {im.width}px is wider than anything existing "
                        f"(max 252)")
    return errors, warnings


def main():
    args = sys.argv[1:]
    if args:
        files = [SPRITES / a if not Path(a).exists() else Path(a) for a in args]
    else:
        files = sorted(SPRITES.glob("*.png"))
    if not files:
        sys.exit(f"no sprites found in {SPRITES}")

    bad = 0
    for f in files:
        errors, warnings = check(f)
        if errors or warnings:
            print(f"\n{f.name}")
            for e in errors:
                print(f"  ERROR  {e}")
            for w in warnings:
                print(f"  warn   {w}")
        if errors:
            bad += 1

    print(f"\n{len(files)} sprite(s) checked, {bad} with errors")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
