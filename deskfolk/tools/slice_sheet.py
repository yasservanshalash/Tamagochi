"""Cut a generated sprite sheet into the individual frames a package needs.

The sheets come back as one flat RGB image: no alpha, captioned in orange, with
the animations laid out in labelled bands. Three things have to happen before
the frames are usable, and each has a trap in it.

**Alpha.** The background is a near-black grey, and so are parts of him — the
jacket, the trousers, the outlines. A global colour key eats those. A flood
fill from the borders does not, because his dark areas are enclosed by lighter
outline; but the *tolerance* still matters enormously. At 34 the fill leaks
through the outline and strips his jacket and legs; at 16 he comes out whole.

**Finding the frames.** Detecting them purely from gaps does not work: the
orange captions merge into the band above the poses, and poses that share a
prop — the three at a laptop desk — touch each other. The caption colour is no
help either, being the same family as his skin. So the *layout* is declared
below and gap detection only runs inside one group's column range, where it
has an expected frame count to check itself against.

**Baseline.** Frames are written onto a shared canvas with his feet on a fixed
line, because the renderer anchors him by the feet. Cropping each frame tight
would make him jump every time the animation changed.

Verify with `--contact`, which writes one image of every frame it found.
"""
import json
import os
import sys
from collections import deque

try:
    from PIL import Image
    import numpy as np
except ImportError:
    sys.exit("needs pillow and numpy: pip install pillow numpy")

# Leaks through his outline above this and takes the jacket with it.
TOLERANCE = 16
# The art is drawn at roughly half the size the companion is displayed at, and
# 2 is the only upscale that keeps every source pixel square.
UPSCALE = 2

# (y_top, y_bottom, [(name, x_start, expected_frames)]) — x_start is where the
# group's poses begin; the group runs to the next group's start.
LAYOUT = [
    (38, 190, [("walk", 14, 6), ("run", 516, 5), ("stand", 968, 6)]),
    (238, 382, [("turn", 14, 6), ("jump", 462, 5), ("sit_down", 920, 6)]),
    (434, 576, [("take_coffee", 14, 4), ("drink", 446, 4),
                ("yawn", 806, 4), ("thinking", 1216, 3)]),
    (634, 778, [("wave", 14, 3), ("point", 344, 3), ("shrug", 676, 3),
                ("facepalm", 1004, 2), ("arms_crossed", 1246, 2)]),
    (831, 984, [("laptop", 14, 3), ("phone", 426, 3), ("cheer", 701, 2),
                ("dance", 956, 3), ("sleep", 1264, 2)]),
]

# Poses are separated by only a couple of background columns, so the gap has to
# be small; the width floor is what then keeps stray sparks and music notes
# from being mistaken for frames.
MIN_FRAME_W = 40
FRAME_GAP = 2


def looks_checkered(rgb):
    """Is the 'transparency' a painted checkerboard rather than real alpha?

    Generators often hand back an opaque image with the checkerboard *drawn
    on*, which looks transparent and is not. It shows up as a large share of
    near-neutral mid greys, which no part of this character wears — he is all
    browns and tans, his trousers are darker than the band and his headphones
    lighter.
    """
    return checker_mask(rgb).mean() > 0.25


def checker_mask(rgb):
    """True on the painted checkerboard.

    Deliberately not flood-filled from the border. The squares are separated
    by anti-aliased seams that a fill cannot cross, and the gaps *between his
    legs* are enclosed anyway — a border fill leaves a grey strip standing
    there. Classifying by colour alone is safe precisely because the band is
    one he never occupies.
    """
    a = rgb.astype(int)
    spread = a.max(axis=2) - a.min(axis=2)
    value = a.mean(axis=2)
    return (spread <= 14) & (value >= 116) & (value <= 208)


def background_mask(rgb, tol):
    """True where the pixel is background reachable from the border."""
    if looks_checkered(rgb):
        return checker_mask(rgb)
    h, w, _ = rgb.shape
    bg = rgb[0, 0].astype(int)
    near = np.abs(rgb.astype(int) - bg).max(axis=2) <= tol
    seen = np.zeros((h, w), dtype=bool)
    q = deque()
    for x in range(w):
        for y in (0, h - 1):
            if near[y, x] and not seen[y, x]:
                seen[y, x] = True
                q.append((y, x))
    for y in range(h):
        for x in (0, w - 1):
            if near[y, x] and not seen[y, x]:
                seen[y, x] = True
                q.append((y, x))
    while q:
        y, x = q.popleft()
        for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            ny, nx = y + dy, x + dx
            if 0 <= ny < h and 0 <= nx < w and not seen[ny, nx] and near[ny, nx]:
                seen[ny, nx] = True
                q.append((ny, nx))
    return seen


def runs(occupied, gap):
    """Runs of True, merging anything separated by fewer than `gap` columns."""
    out, start, empty = [], None, 0
    for i, v in enumerate(occupied):
        if v:
            if start is None:
                start = i
            empty = 0
        elif start is not None:
            empty += 1
            if empty >= gap:
                out.append((start, i - empty + 1))
                start = None
    if start is not None:
        out.append((start, len(occupied)))
    return [(a, b) for a, b in out if b - a >= MIN_FRAME_W]


def cut_group(fg, y0, y1, x0, x1, want):
    """Frame column ranges inside one group.

    Gaps find nearly everything. What they cannot separate is poses that share
    a prop — the three at one laptop desk are a single connected run — so any
    shortfall is made up by repeatedly halving the widest run, which is what a
    merged pair or triple actually is. Splitting only the widest run leaves
    correctly-detected neighbours untouched, where dividing the whole group
    evenly would shift every frame in it.
    """
    found = runs(fg[y0:y1, x0:x1].any(axis=0), FRAME_GAP)
    if not found:
        return [], "empty"
    how = "gaps"
    if len(found) < want:
        how = f"split {len(found)}->{want}"
        while len(found) < want:
            i = max(range(len(found)), key=lambda k: found[k][1] - found[k][0])
            a, b = found.pop(i)
            mid = (a + b) // 2
            found[i:i] = [(a, mid), (mid, b)]
    elif len(found) > want:
        # More runs than poses means something small got through the width
        # floor; the widest `want` are the poses.
        how = f"kept {want} of {len(found)}"
        found = sorted(sorted(found, key=lambda r: r[0] - r[1])[:want])
    return [(x0 + a, x0 + b) for a, b in found], how


def main(sheet, outdir, contact=False, row=None, upscale=UPSCALE, prefix="hd_"):
    rgb = np.asarray(Image.open(sheet).convert("RGB"))
    fg = ~background_mask(rgb, TOLERANCE)
    if looks_checkered(rgb):
        print("background: painted checkerboard (not real alpha)")

    groups = []
    if row:
        # One row, one animation — for a sheet that is a single cycle rather
        # than a page of them, which is the shape worth asking a generator for
        # when the frames need to be big.
        name, y0, y1, want = row
        cuts, how = cut_group(fg, y0, y1, 0, fg.shape[1], want)
        print(f"{name:14s} {len(cuts)} frames  [{how}]")
        groups.append((name, y0, y1, cuts))
    else:
        for y0, y1, specs in LAYOUT:
            for i, (name, xs, want) in enumerate(specs):
                xe = specs[i + 1][1] if i + 1 < len(specs) else fg.shape[1]
                cuts, how = cut_group(fg, y0, y1, xs, xe, want)
                print(f"{name:14s} {len(cuts)} frames  [{how}]")
                groups.append((name, y0, y1, cuts))

    # Measure every frame first: one canvas for all of them, feet on a shared
    # line, so switching animation never shifts him vertically.
    boxes = {}
    for name, y0, y1, cuts in groups:
        for i, (a, b) in enumerate(cuts):
            col = fg[y0:y1, a:b]
            ys = np.where(col.any(axis=1))[0]
            xs = np.where(col.any(axis=0))[0]
            if not len(ys) or not len(xs):
                continue
            boxes[(name, i)] = (a + int(xs.min()), a + int(xs.max()) + 1,
                                y0 + int(ys.min()), y0 + int(ys.max()) + 1)
    cw = max(b - a for a, b, _, _ in boxes.values()) + 6
    # No padding under the feet: the renderer lands a sprite's *bottom edge* on
    # the stage anchor, so any margin there lifts him off the ground by exactly
    # that much — the same mistake that had him hovering over window ledges.
    ch = max(d - c for _, _, c, d in boxes.values())
    feet = ch
    print(f"canvas {cw}x{ch} -> {cw*upscale}x{ch*upscale}, feet on the bottom edge")

    os.makedirs(outdir, exist_ok=True)
    index, tiles = {}, []
    for (name, i), (a, b, c, d) in sorted(boxes.items()):
        rgba = np.dstack([rgb[c:d, a:b], np.where(fg[c:d, a:b], 255, 0)]).astype(np.uint8)
        canvas = np.zeros((ch, cw, 4), dtype=np.uint8)
        ox, oy = (cw - (b - a)) // 2, feet - (d - c)
        canvas[oy:oy + (d - c), ox:ox + (b - a)] = rgba
        img = Image.fromarray(canvas, "RGBA")
        if upscale != 1:
            img = img.resize((cw * upscale, ch * upscale), Image.NEAREST)
        fname = f"img_y_{prefix}{name}{i}"
        img.save(os.path.join(outdir, fname + ".png"))
        index.setdefault(name, []).append(fname)
        tiles.append((fname, img))

    with open(os.path.join(outdir, "_index.json"), "w") as f:
        json.dump({"canvas": [cw * upscale, ch * upscale],
                   "feet": feet * upscale, "groups": index}, f, indent=1)
    print(f"wrote {len(tiles)} frames to {outdir}")

    if contact:
        per = 10
        rows = (len(tiles) + per - 1) // per
        tw, th = tiles[0][1].size
        sheet_img = Image.new("RGBA", (per * tw, rows * th), (24, 24, 28, 255))
        for n, (_, img) in enumerate(tiles):
            sheet_img.paste(img, ((n % per) * tw, (n // per) * th), img)
        sheet_img.save(os.path.join(outdir, "_contact.png"))
        print("contact sheet written")


def _opt(flag, default=None):
    for a in sys.argv[1:]:
        if a.startswith(flag + "="):
            return a[len(flag) + 1:]
    return default


if __name__ == "__main__":
    args = [a for a in sys.argv[1:] if not a.startswith("-")]
    if len(args) < 2:
        sys.exit(
            "usage: slice_sheet.py <sheet.png> <outdir> [--contact]\n"
            "       [--row=name:y0:y1:frames] [--upscale=N] [--prefix=hd_]"
        )
    spec = _opt("--row")
    row = None
    if spec:
        name, y0, y1, want = spec.split(":")
        row = (name, int(y0), int(y1), int(want))
    main(
        args[0],
        args[1],
        "--contact" in sys.argv,
        row,
        int(_opt("--upscale", UPSCALE)),
        _opt("--prefix", "hd_"),
    )
