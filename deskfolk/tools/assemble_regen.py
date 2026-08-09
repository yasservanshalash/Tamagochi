#!/usr/bin/env python3
"""Assemble the freshly-regenerated isolated parts into a front Yasser.

Each coarse part is bound to a representative slot, placed at the UNION of that
region's fine-slot canonical bounds from definition.json, so head/torso/arms/legs
line up on the shared 320 canvas. Writes a scene the Rust renderer consumes."""
import json, shutil
from pathlib import Path
from PIL import Image

HERE = Path(__file__).resolve().parent
PKG = HERE.parent / "characters" / "yasser" / "modular"
REGEN = HERE.parent / "characters" / "yasser" / "modular_regen"
OUT = HERE.parent / "characters" / "yasser" / "modular_assembled"

definition = json.loads((PKG / "definition.json").read_text(encoding="utf-8"))
slots = definition["slots"]

def union(*names):
    xs0, ys0, xs1, ys1 = [], [], [], []
    for n in names:
        b = slots[n]["default_bounds"]
        xs0.append(b["x"]); ys0.append(b["y"])
        xs1.append(b["x"] + b["width"]); ys1.append(b["y"] + b["height"])
    x, y = min(xs0), min(ys0)
    return {"x": x, "y": y, "width": max(xs1) - x, "height": max(ys1) - y}

import numpy as np

def key_background(path):
    """The model draws an opaque neutral-gray/checker background instead of real
    transparency. Remove it by flood-filling transparency inward from the edges
    over near-neutral light pixels — so interior light-but-WARM pixels (the cream
    headphones) survive. Returns an RGBA image."""
    rgb = np.asarray(Image.open(path).convert("RGB")).astype(np.int16)
    h, w, _ = rgb.shape
    mx = rgb.max(2); mn = rgb.min(2)
    # "background-like" = light and near-neutral (R~=G~=B)
    bglike = (mn > 165) & ((mx - mn) < 30)
    # flood fill from the border so we only clear the outer background, not any
    # neutral highlight trapped inside the character.
    from collections import deque
    bg = np.zeros((h, w), bool)
    dq = deque()
    for x in range(w):
        for y in (0, h - 1):
            if bglike[y, x] and not bg[y, x]:
                bg[y, x] = True; dq.append((y, x))
    for y in range(h):
        for x in (0, w - 1):
            if bglike[y, x] and not bg[y, x]:
                bg[y, x] = True; dq.append((y, x))
    while dq:
        y, x = dq.popleft()
        for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            ny, nx = y + dy, x + dx
            if 0 <= ny < h and 0 <= nx < w and bglike[ny, nx] and not bg[ny, nx]:
                bg[ny, nx] = True; dq.append((ny, nx))
    alpha = np.where(bg, 0, 255).astype(np.uint8)
    rgba = np.dstack([rgb.astype(np.uint8), alpha])
    return Image.fromarray(rgba, "RGBA")

def trim_alpha(im, thresh=8):
    a = im.getchannel("A").point(lambda v: 255 if v > thresh else 0)
    return a.getbbox() or (0, 0, im.width, im.height)

# coarse part -> (file, slot, variant, union-of-slots, explicit_bounds|None)
# The upper body is ONE unified puffer (jacket + sleeves + hands) to match the
# reference's single silhouette — no separate arm blocks. Three clean pieces:
# legs (rear) -> upper body (torso) -> head.
PARTS = [
    ("legs_front.png",       "left_leg",  "stand",      ["pelvis","left_leg","right_leg","left_boot","right_boot"], None),
    # Narrower puffer (less "fat"), and the head sits LOW so the beard tucks into
    # the collar (no floating gap): head bottom (y44+92=136) overlaps torso top (112).
    ("upper_body_front.png", "torso",     "idle_front", None, {"x": 110, "y": 112, "width": 100, "height": 108}),
    ("head_front.png",       "head_base", "front",      None, {"x": 118, "y": 44,  "width": 84,  "height": 92}),
    # Extra head variants for live animation (talk / blink) at the same head box.
    # Unused by the static canonical render (head_base defaults to "front"); the
    # window's animation driver swaps head_base to these for blink/talk.
    ("head_talk.png",        "head_base", "talk",       None, {"x": 118, "y": 44,  "width": 84,  "height": 92}),
    ("head_blink.png",       "head_base", "blink",      None, {"x": 118, "y": 44,  "width": 84,  "height": 92}),
    # Side view + walk-cycle for locomotion. `center_h` = aspect-preserving,
    # centered on x=160 at that art-pixel height (side silhouettes are narrower).
    ("head_side.png",        "head_base", "side",       None, {"center_h": 92,  "y": 44}),
    ("torso_side.png",       "torso",     "side",       None, {"center_h": 108, "y": 112}),
    ("legs_side_walk1.png",  "left_leg",  "walk1",      None, {"center_h": 95,  "y": 197}),
    ("legs_side_walk2.png",  "left_leg",  "walk2",      None, {"center_h": 95,  "y": 197}),
]

OUT.mkdir(parents=True, exist_ok=True)
(OUT / "sheets").mkdir(exist_ok=True)
shutil.copy(PKG / "definition.json", OUT / "definition.json")

instances = []
for fname, slot, variant, union_slots, explicit in PARTS:
    src = REGEN / fname
    if not src.exists():
        print("MISSING", fname); continue
    keyed = key_background(src)                       # remove baked background
    keyed.save(OUT / "sheets" / fname)               # now a real transparent PNG
    l, t, r, b = trim_alpha(keyed)
    if explicit and "center_h" in explicit:
        h = explicit["center_h"]
        w = max(1, round((r - l) / (b - t) * h))   # preserve the part's aspect
        cb = {"x": 160 - w // 2, "y": explicit["y"], "width": w, "height": h}
    else:
        cb = explicit if explicit else union(*union_slots)
    pv = slots[slot]["pivot"]
    instances.append({
        "id": f"{slot}/{variant}", "slot": slot, "variant": variant, "view": "front",
        "source": f"sheets/{fname}",
        "region": {"x": l, "y": t, "w": r - l, "h": b - t},
        "canonical_bounds": cb,
        "pivot": [pv[0], pv[1]], "angle": 0, "mirror_ok": False,
    })
    print(f"{slot:16} region {r-l}x{b-t}  -> canonical {cb['width']}x{cb['height']} @ {cb['x']},{cb['y']}")

(OUT / "manifest.json").write_text(
    json.dumps({"definition_id": definition["id"], "instances": instances}, indent=2),
    encoding="utf-8")
print(f"\nWrote {OUT/'manifest.json'} with {len(instances)} parts.")
