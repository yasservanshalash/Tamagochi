#!/usr/bin/env python3
"""Assemble Yasser at several viewing angles (front / 3-quarter / side / back)
to prove he can turn. Angle parts are placed centered on x=160 with their
natural aspect ratio preserved (side/back silhouettes are narrower than front,
so we target a height and derive width). Renders each and montages a turnaround."""
import json, subprocess
from pathlib import Path
from collections import deque
import numpy as np
from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
PKG = HERE.parent / "characters" / "yasser" / "modular"
REGEN = HERE.parent / "characters" / "yasser" / "modular_regen"
OUT = HERE.parent / "characters" / "yasser" / "modular_assembled"
ANG = OUT / "angles"; ANG.mkdir(parents=True, exist_ok=True)
definition = json.loads((PKG / "definition.json").read_text(encoding="utf-8"))
slots = definition["slots"]
CENTER = 160

def key_background(path):
    rgb = np.asarray(Image.open(path).convert("RGB")).astype(np.int16)
    h, w, _ = rgb.shape
    mx = rgb.max(2); mn = rgb.min(2)
    bglike = (mn > 165) & ((mx - mn) < 30)
    bg = np.zeros((h, w), bool); dq = deque()
    for x in range(w):
        for y in (0, h-1):
            if bglike[y,x] and not bg[y,x]: bg[y,x]=True; dq.append((y,x))
    for y in range(h):
        for x in (0, w-1):
            if bglike[y,x] and not bg[y,x]: bg[y,x]=True; dq.append((y,x))
    while dq:
        y,x = dq.popleft()
        for dy,dx in ((1,0),(-1,0),(0,1),(0,-1)):
            ny,nx=y+dy,x+dx
            if 0<=ny<h and 0<=nx<w and bglike[ny,nx] and not bg[ny,nx]:
                bg[ny,nx]=True; dq.append((ny,nx))
    return Image.fromarray(np.dstack([rgb.astype(np.uint8), np.where(bg,0,255).astype(np.uint8)]), "RGBA")

def prep(fname):
    keyed = key_background(REGEN / fname)
    (OUT/"sheets").mkdir(exist_ok=True)
    keyed.save(OUT/"sheets"/fname)
    a = keyed.getchannel("A").point(lambda v: 255 if v>8 else 0)
    return f"sheets/{fname}", a.getbbox()

def place(slot, fname, y, h):
    """Center a part on x=160 at height h, width derived from its aspect."""
    src, (l, t, r, b) = prep(fname)
    rw, rh = r-l, b-t
    w = max(1, round(rw / rh * h))
    pv = slots[slot]["pivot"]
    return {"id": f"{slot}/{slots[slot]['variants'][0]}", "slot": slot,
            "variant": slots[slot]["variants"][0], "view": "front", "source": src,
            "region": {"x": l, "y": t, "w": rw, "h": rh},
            "canonical_bounds": {"x": CENTER - w//2, "y": y, "width": w, "height": h},
            "pivot": [pv[0], pv[1]], "angle": 0, "mirror_ok": False}

# y/height bands (from the front canonical union bounds)
HEAD=(29,89); TORSO=(111,104); LEGS=(197,95); ARM=(124,96)

VIEWS = {
    "front": [place("left_leg","legs_front.png",*LEGS), place("torso","torso_front.png",*TORSO),
              place("head_base","head_front.png",*HEAD)],
    "3q":    [place("left_leg","legs_front.png",*LEGS), place("torso","torso_front.png",*TORSO),
              place("head_base","head_3q.png",*HEAD)],
    "side":  [place("left_leg","legs_side.png",*LEGS),
              place("torso","torso_side.png",*TORSO),
              place("right_upper_arm","arm_side.png",*ARM),
              place("head_base","head_side.png",*HEAD)],
    "back":  [place("left_leg","legs_back.png",*LEGS), place("torso","torso_back.png",*TORSO),
              place("head_base","head_back.png",*HEAD)],
}

frames=[]
for name, insts in VIEWS.items():
    (OUT/"manifest.json").write_text(json.dumps({"definition_id":definition["id"],"instances":insts},indent=2),encoding="utf-8")
    png = ANG/f"{name}.png"
    subprocess.run(["cargo","run","-q","-p","deskfolk-render-win","--example","render_modular","--",str(OUT),str(png),"3"],cwd=HERE.parent,check=True)
    frames.append((name,png)); print("rendered",name)

tw=280; cells=[]
for name,png in frames:
    im=Image.open(png).convert("RGBA"); bb=im.getbbox() or (0,0,im.width,im.height)
    im=im.crop(bb); w,h=im.size; th=round(h*tw/w)
    bg=Image.new("RGBA",(tw,th),(235,235,235,255)); bg.alpha_composite(im.resize((tw,th)))
    cells.append((name,bg.convert("RGB")))
ch=max(c.size[1] for _,c in cells)+20; W=len(cells)*tw
canvas=Image.new("RGB",(W,ch),(24,23,29)); dr=ImageDraw.Draw(canvas); x=0
for name,c in cells:
    dr.text((x+6,4),name,fill=(240,200,120)); canvas.paste(c,(x,20)); x+=tw
mont=ANG/"turnaround.png"; canvas.save(mont); print("montage:",mont)
