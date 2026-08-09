#!/usr/bin/env python3
"""Autoslice real generated sheets into a demo modular manifest.

This is the headless stand-in for interactive inspector use: it slices each
sheet the same way (transparent-gutter grid detection + per-cell alpha trim),
picks a chosen cell, and binds it to a slot at that slot's canonical bounds/pivot
from definition.json. It then assembles a self-contained demo scene directory
(definition + sheets/ + manifest.json) that the Rust example can render.

It does NOT touch the shipped, intentionally-empty package manifest.

Usage:
  python tools/autoslice_manifest.py
"""
import json, shutil
from pathlib import Path
from PIL import Image

HERE = Path(__file__).resolve().parent
PKG = HERE.parent / "characters" / "yasser" / "modular"
# The generated sheets are gitignored, so they live in the main checkout's flux
# pack, not this worktree. Override with DESKFOLK_ACCEPTED if yours is elsewhere.
import os
ACCEPTED = Path(os.environ.get(
    "DESKFOLK_ACCEPTED",
    r"C:\Users\yasse\Desktop\code\Tamagochi\deskfolk\deskfolk_flux_generation_pack\deskfolk_flux_pack\generated\accepted",
))
DEMO = HERE.parent / "characters" / "yasser" / "modular_demo"

# (sheet file, slot, variant, cell index). Cell 0 = top-left = the front/neutral
# default on every sheet. These four dedicated face/head sheets are the most
# isolated of the generated art; other slots came out as full busts (see
# ASSET_AUDIT.md) and are deliberately left unbound.
BINDINGS = [
    ("head_base__batch_01.png", "head_base", "front",   0),
    ("eyes__batch_01.png",      "eyes",      "neutral", 0),
    ("nose__batch_01.png",      "nose",      "front",   0),
    ("mouth__batch_01.png",     "mouth",     "closed",  0),
]


def slice_cells(path):
    """Return trimmed cell bboxes [(x,y,w,h), ...] in reading order."""
    img = Image.open(path).convert("RGBA")
    w, h = img.size
    px = img.load()
    a = [[px[x, y][3] for x in range(w)] for y in range(h)]

    col_has = [any(a[y][x] > 24 for y in range(h)) for x in range(w)]
    row_has = [any(a[y][x] > 24 for x in range(w)) for y in range(h)]

    def bands(flags):
        out, s = [], -1
        for i, v in enumerate(flags):
            if v and s < 0:
                s = i
            elif not v and s >= 0:
                if i - s >= 6:
                    out.append((s, i))
                s = -1
        if s >= 0 and len(flags) - s >= 6:
            out.append((s, len(flags)))
        return out

    col_bands, row_bands = bands(col_has), bands(row_has)
    cells = []
    for (ry0, ry1) in row_bands:
        for (cx0, cx1) in col_bands:
            x0, y0, x1, y1 = cx1, ry1, cx0, ry0
            for y in range(ry0, ry1):
                for x in range(cx0, cx1):
                    if a[y][x] > 24:
                        x0, x1 = min(x0, x), max(x1, x)
                        y0, y1 = min(y0, y), max(y1, y)
            if x1 >= x0 and y1 >= y0:
                cells.append((x0, y0, x1 - x0 + 1, y1 - y0 + 1))
    return cells


def grid_cells(path, cols, rows):
    """Divide a sheet into a fixed cols x rows grid and trim each cell to its
    opaque content. More robust than gutter detection on dense/labelled sheets."""
    img = Image.open(path).convert("RGBA")
    w, h = img.size
    px = img.load()
    cw, ch = w // cols, h // rows
    cells = []
    for r in range(rows):
        for c in range(cols):
            gx0, gy0 = c * cw, r * ch
            gx1, gy1 = gx0 + cw, gy0 + ch
            x0, y0, x1, y1 = gx1, gy1, gx0, gy0
            for y in range(gy0, gy1):
                for x in range(gx0, gx1):
                    if px[x, y][3] > 40:
                        x0, x1 = min(x0, x), max(x1, x)
                        y0, y1 = min(y0, y), max(y1, y)
            if x1 >= x0 and y1 >= y0:
                cells.append((x0, y0, x1 - x0 + 1, y1 - y0 + 1))
            else:
                cells.append((gx0, gy0, cw, ch))
    return cells


def main():
    definition = json.loads((PKG / "definition.json").read_text(encoding="utf-8"))
    slots = definition["slots"]

    DEMO.mkdir(parents=True, exist_ok=True)
    (DEMO / "sheets").mkdir(exist_ok=True)
    shutil.copy(PKG / "definition.json", DEMO / "definition.json")

    instances = []
    for sheet_file, slot, variant, idx in BINDINGS:
        src = ACCEPTED / sheet_file
        if not src.exists():
            print(f"  skip {slot}: {sheet_file} not found")
            continue
        cells = grid_cells(src, 4, 2)
        if idx >= len(cells):
            print(f"  skip {slot}: sheet has only {len(cells)} cells")
            continue
        cx, cy, cw, ch = cells[idx]
        shutil.copy(src, DEMO / "sheets" / sheet_file)

        sdef = slots[slot]
        cb = sdef["default_bounds"]
        pv = sdef["pivot"]
        instances.append({
            "id": f"{slot}/{variant}",
            "slot": slot, "variant": variant, "view": "front",
            "source": f"sheets/{sheet_file}",
            "region": {"x": cx, "y": cy, "w": cw, "h": ch},
            "canonical_bounds": {"x": cb["x"], "y": cb["y"], "width": cb["width"], "height": cb["height"]},
            "pivot": [pv[0], pv[1]],
            "angle": 0, "mirror_ok": False,
        })
        print(f"  bound {slot}/{variant}: cell {idx} = {cw}x{ch} @ {cx},{cy}  ->  canonical {cb['width']}x{cb['height']} @ {cb['x']},{cb['y']}")

    manifest = {"definition_id": definition["id"], "instances": instances}
    (DEMO / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    print(f"\nWrote {DEMO/'manifest.json'} with {len(instances)} instances.")
    print(f"Render it:  cargo run -p deskfolk-render-win --example render_modular -- \"{DEMO}\" demo.png 2")


if __name__ == "__main__":
    main()
