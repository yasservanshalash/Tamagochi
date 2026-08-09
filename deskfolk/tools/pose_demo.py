#!/usr/bin/env python3
"""Build several POSES of the assembled Yasser to prove modular animation:
each pose changes only the slots a gesture/expression touches (wave = right arm,
talk/happy = head), everything else stays identical. Renders each via the Rust
engine and montages them."""
import json, shutil, subprocess
from pathlib import Path
from collections import deque
import numpy as np
from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
PKG = HERE.parent / "characters" / "yasser" / "modular"
REGEN = HERE.parent / "characters" / "yasser" / "modular_regen"
OUT = HERE.parent / "characters" / "yasser" / "modular_assembled"
POSES = OUT / "poses"
POSES.mkdir(parents=True, exist_ok=True)
definition = json.loads((PKG / "definition.json").read_text(encoding="utf-8"))
slots = definition["slots"]

def key_background(path):
    rgb = np.asarray(Image.open(path).convert("RGB")).astype(np.int16)
    h, w, _ = rgb.shape
    mx = rgb.max(2); mn = rgb.min(2)
    bglike = (mn > 165) & ((mx - mn) < 30)
    bg = np.zeros((h, w), bool); dq = deque()
    for x in range(w):
        for y in (0, h - 1):
            if bglike[y, x] and not bg[y, x]: bg[y, x] = True; dq.append((y, x))
    for y in range(h):
        for x in (0, w - 1):
            if bglike[y, x] and not bg[y, x]: bg[y, x] = True; dq.append((y, x))
    while dq:
        y, x = dq.popleft()
        for dy, dx in ((1,0),(-1,0),(0,1),(0,-1)):
            ny, nx = y+dy, x+dx
            if 0<=ny<h and 0<=nx<w and bglike[ny,nx] and not bg[ny,nx]:
                bg[ny,nx]=True; dq.append((ny,nx))
    alpha = np.where(bg, 0, 255).astype(np.uint8)
    return Image.fromarray(np.dstack([rgb.astype(np.uint8), alpha]), "RGBA")

def prep(fname):
    """Key + save a regen part into sheets/, return (relpath, trimmed region)."""
    keyed = key_background(REGEN / fname)
    (OUT / "sheets").mkdir(exist_ok=True)
    keyed.save(OUT / "sheets" / fname)
    a = keyed.getchannel("A").point(lambda v: 255 if v > 8 else 0)
    l, t, r, b = a.getbbox()
    return f"sheets/{fname}", {"x": l, "y": t, "w": r - l, "h": b - t}

def union(*names):
    xs0, ys0, xs1, ys1 = [], [], [], []
    for n in names:
        bb = slots[n]["default_bounds"]
        xs0.append(bb["x"]); ys0.append(bb["y"])
        xs1.append(bb["x"]+bb["width"]); ys1.append(bb["y"]+bb["height"])
    x, y = min(xs0), min(ys0)
    return {"x": x, "y": y, "width": max(xs1)-x, "height": max(ys1)-y}

def inst(slot, variant, fname, bounds):
    src, region = prep(fname)
    pv = slots[slot]["pivot"]
    return {"id": f"{slot}/{variant}", "slot": slot, "variant": variant, "view": "front",
            "source": src, "region": region, "canonical_bounds": bounds,
            "pivot": [pv[0], pv[1]], "angle": 0, "mirror_ok": False}

# shared base pieces
LEGS  = lambda: inst("left_leg", "stand", "legs_front.png", union("pelvis","left_leg","right_leg","left_boot","right_boot"))
ARM_L = lambda: inst("left_upper_arm", "down", "arm_left.png", {"x":96,"y":128,"width":34,"height":96})
ARM_R = lambda: inst("left_forearm", "straight_down", "arm_right.png", {"x":190,"y":128,"width":34,"height":96})
TORSO = lambda: inst("torso", "idle_front", "torso_front.png", union("torso"))
# Every instance uses the slot's DEFAULT variant (what canonical_yasser selects),
# varying only the source PNG — otherwise the resolver won't bind it.
HEAD  = lambda f="head_front.png": inst("head_base", "front", f, union("head_base","beanie","headphones","eyes","eyebrows","nose","mouth","beard"))

POSE_SETS = {
    # idle: canonical standing
    "idle":  [LEGS(), ARM_L(), ARM_R(), TORSO(), HEAD()],
    # wave: ONLY the right arm changes — raised & waving (front_arm layer, drawn
    # over the torso); left arm + torso + head + legs are byte-identical to idle.
    "wave":  [LEGS(), ARM_L(), TORSO(),
              inst("right_upper_arm", "down", "arm_right_wave.png", {"x":198,"y":78,"width":70,"height":140}),
              HEAD()],
    # talk: ONLY the head changes
    "talk":  [LEGS(), ARM_L(), ARM_R(), TORSO(), HEAD("head_talk.png")],
    # happy: ONLY the head changes
    "happy": [LEGS(), ARM_L(), ARM_R(), TORSO(), HEAD("head_happy.png")],
}

frames = []
for name, insts in POSE_SETS.items():
    (OUT / "manifest.json").write_text(
        json.dumps({"definition_id": definition["id"], "instances": insts}, indent=2), encoding="utf-8")
    png = POSES / f"{name}.png"
    subprocess.run(["cargo", "run", "-q", "-p", "deskfolk-render-win", "--example", "render_modular",
                    "--", str(OUT), str(png), "3"], cwd=HERE.parent, check=True)
    frames.append((name, png))
    print(f"rendered pose: {name}")

# montage
tw = 300; cells = []
for name, png in frames:
    im = Image.open(png).convert("RGBA")
    bb = im.getbbox() or (0,0,im.width,im.height)
    im = im.crop(bb); w,h = im.size; th = round(h*tw/w)
    bg = Image.new("RGBA",(tw,th),(235,235,235,255)); bg.alpha_composite(im.resize((tw,th)))
    cells.append((name, bg.convert("RGB")))
ch = max(c.size[1] for _,c in cells)+20; W = len(cells)*tw
canvas = Image.new("RGB",(W,ch),(24,23,29)); dr = ImageDraw.Draw(canvas)
x=0
for name,c in cells:
    dr.text((x+6,4), name, fill=(240,200,120)); canvas.paste(c,(x,20)); x+=tw
mont = POSES / "poses_montage.png"; canvas.save(mont); print("montage:", mont)
