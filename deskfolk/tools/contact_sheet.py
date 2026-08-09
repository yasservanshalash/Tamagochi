#!/usr/bin/env python3
"""Montage every accepted sprite sheet into labelled contact sheets for a
complete visual inventory. Read-only; writes PNGs to the scratchpad."""
import os
from pathlib import Path
from PIL import Image, ImageDraw

ACCEPTED = Path(os.environ.get(
    "DESKFOLK_ACCEPTED",
    r"C:\Users\yasse\Desktop\code\Tamagochi\deskfolk\deskfolk_flux_generation_pack\deskfolk_flux_pack\generated\accepted",
))
OUT = Path(r"C:\Users\yasse\AppData\Local\Temp\claude\C--Users-yasse-Desktop-code-Tamagochi--claude-worktrees-task14-layered-window-tts\581c98a2-dae8-4910-b1f4-c3ab1991da7b\scratchpad")
OUT.mkdir(parents=True, exist_ok=True)

files = sorted(p for p in ACCEPTED.iterdir() if p.suffix.lower() == ".png")
COLS = 3
THUMB_W = 380
LABEL_H = 22
BG = (25, 24, 30)
CHECK = (46, 44, 52)

def thumb(path):
    im = Image.open(path).convert("RGBA")
    w, h = im.size
    tw = THUMB_W
    th = round(h * tw / w)
    im = im.resize((tw, th), Image.NEAREST)
    # flatten onto a checker so transparency is visible
    bg = Image.new("RGBA", (tw, th), (40, 38, 46, 255))
    for y in range(0, th, 16):
        for x in range(0, tw, 16):
            if (x // 16 + y // 16) % 2:
                for yy in range(y, min(y + 16, th)):
                    for xx in range(x, min(x + 16, tw)):
                        bg.putpixel((xx, yy), (52, 50, 60, 255))
    bg.alpha_composite(im)
    return bg.convert("RGB"), (w, h)

# group into batches of ~15 per output image to keep them readable
per_image = 15
groups = [files[i:i + per_image] for i in range(0, len(files), per_image)]
for gi, group in enumerate(groups):
    rows = (len(group) + COLS - 1) // COLS
    # compute a uniform cell height from the tallest thumb in the group
    thumbs = [thumb(p) for p in group]
    cell_h = max(t.size[1] for t, _ in thumbs) + LABEL_H
    cell_w = THUMB_W
    W = COLS * cell_w
    H = rows * cell_h
    canvas = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(canvas)
    for i, (p, (t, dim)) in enumerate(zip(group, thumbs)):
        r, c = divmod(i, COLS)
        x, y = c * cell_w, r * cell_h
        d.text((x + 4, y + 4), f"{p.name}  [{dim[0]}x{dim[1]}]", fill=(240, 200, 120))
        canvas.paste(t, (x, y + LABEL_H))
    out = OUT / f"contact_{gi+1}.png"
    canvas.save(out)
    print(f"wrote {out}  ({len(group)} sheets)")

print(f"total sheets: {len(files)}")
