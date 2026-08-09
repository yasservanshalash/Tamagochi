"""Cut a two-row instant-transmission effect sheet into frames.

`slice_sheet.py` handles one animation per row; these teleport sheets are two
rows of a single sequence (vanish, then reappear), with frames of wildly
different widths — a full-body pose next to a thin column of energy. So this
reuses that module's green-key/despill/gap-cutting primitives but lays both rows
onto one shared, feet-aligned canvas and numbers the frames straight through.

It also scales the whole set so his *body* matches a target height (the walk
sprite's), because the aura makes the raw frames much taller than he is and a
naive import would have him balloon the instant he teleports.

    python slice_it.py <sheet.png> <outdir> <prefix> <target_body_h> \
        <y0:y1:n> <y0:y1:n> [--min-frame=18]
"""
import os
import sys

import numpy as np
from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import slice_sheet as ss


def main(sheet, outdir, prefix, target_body_h, rows, min_frame):
    ss.MIN_FRAME_W = min_frame  # the energy-column frames are narrow
    rgb = np.asarray(Image.open(sheet).convert("RGB"))
    fg = ~ss.background_mask(rgb, ss.TOLERANCE)
    if ss.looks_greenscreen(rgb):
        print("background: chroma-key green")
        rgb = ss.despill(rgb, fg)

    specs = []  # (index, x0, x1, y0, y1)
    idx = 0
    for (y0, y1, want) in rows:
        cuts, how = ss.cut_group(fg, y0, y1, 0, fg.shape[1], want)
        print(f"row {y0}-{y1}: {len(cuts)} frames [{how}]")
        for (a, b) in cuts:
            specs.append((idx, a, b, y0, y1))
            idx += 1

    boxes = {}
    for (i, a, b, y0, y1) in specs:
        col = fg[y0:y1, a:b]
        ys = np.where(col.any(axis=1))[0]
        xs = np.where(col.any(axis=0))[0]
        if not len(ys) or not len(xs):
            continue
        boxes[i] = (a + int(xs.min()), a + int(xs.max()) + 1,
                    y0 + int(ys.min()), y0 + int(ys.max()) + 1)

    cw = max(b - a for a, b, _, _ in boxes.values()) + 6
    ch = max(d - c for _, _, c, d in boxes.values())
    feet = ch
    # His body height is the *first* frame (a clean idle, no aura). Scale the
    # whole set so that matches the target, so he is the same size teleporting
    # as walking.
    idle_h = boxes[0][3] - boxes[0][2]
    scale = target_body_h / idle_h
    print(f"idle body {idle_h}px -> target {target_body_h}px (x{scale:.3f}); "
          f"canvas {cw}x{ch}")

    os.makedirs(outdir, exist_ok=True)
    tiles = []
    for i, (a, b, c, d) in sorted(boxes.items()):
        rgba = np.dstack([rgb[c:d, a:b], np.where(fg[c:d, a:b], 255, 0)]).astype(np.uint8)
        canvas = np.zeros((ch, cw, 4), dtype=np.uint8)
        ox, oy = (cw - (b - a)) // 2, feet - (d - c)
        canvas[oy:oy + (d - c), ox:ox + (b - a)] = rgba
        img = Image.fromarray(canvas, "RGBA")
        if abs(scale - 1.0) > 0.01:
            img = img.resize((max(1, round(cw * scale)), max(1, round(ch * scale))),
                             Image.LANCZOS)
        img.save(os.path.join(outdir, f"img_y_{prefix}{i}.png"))
        tiles.append(img)

    per = 8
    rows_n = (len(tiles) + per - 1) // per
    tw, th = tiles[0].size
    contact = Image.new("RGBA", (per * tw, rows_n * th), (24, 24, 28, 255))
    for n, img in enumerate(tiles):
        contact.paste(img, ((n % per) * tw, (n // per) * th), img)
    contact.save(os.path.join(outdir, "_contact.png"))
    print(f"wrote {len(tiles)} frames ({tiles[0].size[0]}x{tiles[0].size[1]}) to {outdir}")


if __name__ == "__main__":
    args = [a for a in sys.argv[1:] if not a.startswith("-")]
    min_frame = 18
    for a in sys.argv[1:]:
        if a.startswith("--min-frame="):
            min_frame = int(a.split("=")[1])
    sheet, outdir, prefix, target = args[0], args[1], args[2], int(args[3])
    rows = []
    for spec in args[4:]:
        y0, y1, n = spec.split(":")
        rows.append((int(y0), int(y1), int(n)))
    main(sheet, outdir, prefix, target, rows, min_frame)
