#!/usr/bin/env python3
"""Build the modular Yasser by SLICING the master reference sheet — not by
generating parts. Every part is literally the reference art, so the assembled
character is guaranteed on-model (no image-model drift).

Source: the master reference sheet PNG (FRONT / 3-4 / SIDE / BACK full bodies
plus a 2x7 grid of expression heads). We cut each full body into head / torso /
legs bands at the natural neck + jacket-hem seams, key the near-black
background, and place every band at its true position on the 320 canvas so the
bands reassemble into the original drawing. Front and side share the same feet
line and height, so switching front<->side (idle<->walk) keeps him grounded.

Output: characters/yasser/modular_sheet/{definition.json, manifest.json, sheets/}
Point the app at it with DESKFOLK_MODULAR=1 + DESKFOLK_MODULAR_DIR=<abs path>.
"""
import json, shutil
from pathlib import Path
import numpy as np
from PIL import Image

HERE = Path(__file__).resolve().parent
PKG = HERE.parent / "characters" / "yasser" / "modular"
OUT = HERE.parent / "characters" / "yasser" / "modular_sheet"
SPRITES = HERE.parent / "characters" / "yasser" / "sprites"
SHEET = Path.home() / "Downloads" / "ChatGPT_Image_Aug_2_2026_01_27_43_PM.png"

CANVAS = 320
TARGET_H = 300          # character height on the canvas, in canvas px
TOP_Y = 10              # top margin -> feet land at TOP_Y + TARGET_H = 310
# The sheet background is near-black (lum ~5); his DARKEST pixels — black jeans,
# jacket shadow — are lum ~40+. So anything below this is background... but only
# if it's connected to the border (flood fill), else interior dark clothing
# would punch transparent holes. That hole-punching was the "transparent" bug.
BG_LUM = 24

definition = json.loads((PKG / "definition.json").read_text(encoding="utf-8"))


def flood_bg(dark):
    """Mark background = dark pixels reachable from the border (4-connected).
    Interior dark pixels (jeans, shadow) are enclosed by his lighter outline,
    so the flood never reaches them and they stay opaque."""
    from collections import deque
    h, w = dark.shape
    bg = np.zeros((h, w), bool)
    dq = deque()
    for x in range(w):
        for y in (0, h - 1):
            if dark[y, x] and not bg[y, x]:
                bg[y, x] = True; dq.append((y, x))
    for y in range(h):
        for x in (0, w - 1):
            if dark[y, x] and not bg[y, x]:
                bg[y, x] = True; dq.append((y, x))
    while dq:
        y, x = dq.popleft()
        for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            ny, nx = y + dy, x + dx
            if 0 <= ny < h and 0 <= nx < w and dark[ny, nx] and not bg[ny, nx]:
                bg[ny, nx] = True; dq.append((ny, nx))
    return bg


def key_body(a, x0, x1, y0=140, y1=530):
    """Crop the sheet to a body's column band, flood-fill the near-black
    background to transparent from the edges, tight-trim, and return RGBA."""
    sub = a[y0:y1, x0:x1 + 1]
    lum = sub.max(2)
    bg = flood_bg(lum < BG_LUM)      # only border-connected dark = background
    keep = ~bg
    ys = np.where(keep.any(1))[0]
    xs = np.where(keep.any(0))[0]
    y0t, y1t, x0t, x1t = ys.min(), ys.max() + 1, xs.min(), xs.max() + 1
    c = sub[y0t:y1t, x0t:x1t]
    alpha = np.where(bg[y0t:y1t, x0t:x1t], 0, 255).astype(np.uint8)
    rgba = np.dstack([c.astype(np.uint8), alpha])
    return Image.fromarray(rgba, "RGBA")


def bands(img, seams):
    """Split a full-body RGBA image into horizontal bands at `seams` (fractions
    of art height are pre-resolved to pixel rows by the caller)."""
    W, H = img.width, img.height
    out = []
    rows = [0] + seams + [H]
    for i in range(len(rows) - 1):
        out.append(img.crop((0, rows[i], W, rows[i + 1])))
    return out


def place(img_w, img_h, scale, cx, py0, band_h):
    """Canonical bounds for a full-width band: fixed x/width (whole body column
    centered at cx), y/height from the band's position, all scaled to canvas."""
    w = round(img_w * scale)
    return {
        "x": round(cx - w / 2),
        "y": round(TOP_Y + py0 * scale),
        "width": w,
        "height": round(band_h * scale),
    }


def main():
    sheet = Image.open(SHEET).convert("RGB")
    a = np.asarray(sheet).astype(np.int16)

    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "sheets").mkdir(exist_ok=True)
    shutil.copy(PKG / "definition.json", OUT / "definition.json")

    # Column bands of the four top-row bodies (auto-detected earlier).
    BODIES = {"front": (50, 235), "side": (497, 625)}
    front = key_body(a, *BODIES["front"])
    side = key_body(a, *BODIES["side"])

    instances = []

    def add(part_img, slot, variant, cx, scale, py0, band_h, name):
        part_img.save(OUT / "sheets" / f"{name}.png")
        cb = place(part_img.width, part_img.height, scale, cx, py0, band_h)
        instances.append({
            "id": f"{slot}/{variant}", "slot": slot, "variant": variant,
            "view": "front", "source": f"sheets/{name}.png",
            "region": {"x": 0, "y": 0, "w": part_img.width, "h": part_img.height},
            "canonical_bounds": cb,
            "pivot": definition["slots"][slot]["pivot"], "angle": 0,
            "mirror_ok": False,
        })
        print(f"{name:14} {slot}/{variant:11} -> {cb['width']}x{cb['height']} @ {cb['x']},{cb['y']}")

    # --- FRONT body: head 0-100, torso 100-250, legs 250-end ----------------
    fs = TARGET_H / front.height
    fcx = CANVAS / 2
    fh, ft, fl = bands(front, [100, 250])
    add(fh, "head_base", "front",      fcx, fs, 0,   fh.height, "head_front")
    add(ft, "torso",     "idle_front", fcx, fs, 100, ft.height, "torso_front")
    add(fl, "left_leg",  "stand",      fcx, fs, 250, fl.height, "legs_front")

    # --- SIDE body (faces LEFT natively): same seams, same feet line --------
    ss = TARGET_H / side.height
    scx = CANVAS / 2
    sh, st, sl = bands(side, [100, 250])
    add(sh, "head_base", "side",  scx, ss, 0,   sh.height, "head_side")
    add(st, "torso",     "side",  scx, ss, 100, st.height, "torso_side")
    add(sl, "left_leg",  "side_stand", scx, ss, 250, sl.height, "legs_side")

    # --- WALK: use the real classic directional walk frames --------------
    # A believable articulated walk needs real frames; a single static side
    # pose can only be faked (it wiggles). The classic sprite set already has
    # a clean, on-model, directional 8-frame cycle (walkl / walkr) of THIS
    # character. Bind each full frame to the `torso` slot; during walk the
    # driver hides head_base/left_leg with `without_*` so only the full frame
    # shows. Placed at the same feet line + height as the idle so front<->walk
    # doesn't hop. A 1px empty part backs the `without_*` omission explicitly.
    empty = Image.new("RGBA", (1, 1), (0, 0, 0, 0))
    for slot, variant in (("head_base", "without_head"), ("left_leg", "without_leg")):
        add(empty, slot, variant, CANVAS / 2, 1.0, 250, 1, f"empty_{slot}")

    def classic(name):
        im = Image.open(SPRITES / f"{name}.png").convert("RGBA")
        cs = TARGET_H / im.height
        w = round(im.width * cs)
        cb = {"x": round(CANVAS / 2 - w / 2), "y": TOP_Y, "width": w, "height": TARGET_H}
        out_name = name.replace("img_y_", "")
        im.save(OUT / "sheets" / f"{out_name}.png")
        variant = out_name  # e.g. "walkr0"
        instances.append({
            "id": f"torso/{variant}", "slot": "torso", "variant": variant,
            "view": "front", "source": f"sheets/{out_name}.png",
            "region": {"x": 0, "y": 0, "w": im.width, "h": im.height},
            "canonical_bounds": cb,
            "pivot": definition["slots"]["torso"]["pivot"], "angle": 0,
            "mirror_ok": False,
        })
        print(f"{out_name:14} torso/{variant:11} -> {cb['width']}x{cb['height']} @ {cb['x']},{cb['y']}")

    for d in ("l", "r"):
        for i in range(8):
            classic(f"img_y_walk{d}{i}")

    (OUT / "manifest.json").write_text(
        json.dumps({"definition_id": definition["id"], "instances": instances}, indent=2),
        encoding="utf-8")
    print(f"\nWrote {OUT/'manifest.json'} with {len(instances)} parts.")


if __name__ == "__main__":
    main()
