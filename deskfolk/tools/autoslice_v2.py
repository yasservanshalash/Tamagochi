#!/usr/bin/env python3
"""Assemble a modular manifest from the ISOLATED parts found in the generated
sheets (face features, headwear, hands, forearms, boots). Grid-slices each
sheet, insets past the baked text labels, alpha-trims to the part, and binds it
to the slot's canonical bounds/pivot from definition.json.

Writes a self-contained scene to characters/yasser/modular_assembled/ (leaving
the intentionally-empty shipped manifest untouched)."""
import os, json, shutil
from pathlib import Path
from PIL import Image

HERE = Path(__file__).resolve().parent
PKG = HERE.parent / "characters" / "yasser" / "modular"
ACCEPTED = Path(os.environ.get(
    "DESKFOLK_ACCEPTED",
    r"C:\Users\yasse\Desktop\code\Tamagochi\deskfolk\deskfolk_flux_generation_pack\deskfolk_flux_pack\generated\accepted",
))
OUT = HERE.parent / "characters" / "yasser" / "modular_assembled"

# sheet, slot, variant, cell_index, cols, rows, (inset_top, inset_bottom, inset_lr)
# Insets drop the baked labels above/below the art before alpha-trimming.
BINDINGS = [
    ("eyes__batch_01.png",       "eyes",       "neutral",       0, 4, 2, (0.24, 0.26, 0.12)),
    ("eyebrows__batch_01.png",   "eyebrows",   "neutral",       0, 4, 2, (0.30, 0.34, 0.12)),
    ("nose__batch_01.png",       "nose",       "front",         0, 4, 2, (0.18, 0.30, 0.18)),
    ("mouth__batch_01.png",      "mouth",      "closed",        0, 4, 2, (0.22, 0.32, 0.18)),
    ("beanie__batch_01.png",     "beanie",     "default_front", 4, 4, 2, (0.10, 0.16, 0.10)),
    ("headphones__batch_02.png", "headphones", "on_ears_front", 0, 4, 2, (0.10, 0.16, 0.06)),
]


def trimmed_cell(path, cols, rows, idx, inset):
    im = Image.open(path).convert("RGBA")
    w, h = im.size
    cw, ch = w // cols, h // rows
    r, c = divmod(idx, cols)
    gx0, gy0 = c * cw, r * ch
    it, ib, ilr = inset
    ix0 = gx0 + int(cw * ilr)
    ix1 = gx0 + cw - int(cw * ilr)
    iy0 = gy0 + int(ch * it)
    iy1 = gy0 + ch - int(ch * ib)
    px = im.load()
    x0, y0, x1, y1 = ix1, iy1, ix0, iy0
    for y in range(iy0, iy1):
        for x in range(ix0, ix1):
            if px[x, y][3] > 60:
                x0, x1 = min(x0, x), max(x1, x)
                y0, y1 = min(y0, y), max(y1, y)
    if x1 < x0 or y1 < y0:
        return None
    return (x0, y0, x1 - x0 + 1, y1 - y0 + 1)


def main():
    definition = json.loads((PKG / "definition.json").read_text(encoding="utf-8"))
    slots = definition["slots"]

    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "sheets").mkdir(exist_ok=True)
    shutil.copy(PKG / "definition.json", OUT / "definition.json")

    instances = []
    for sheet, slot, variant, idx, cols, rows, inset in BINDINGS:
        src = ACCEPTED / sheet
        if not src.exists():
            print(f"  MISSING sheet {sheet} for {slot}")
            continue
        bbox = trimmed_cell(src, cols, rows, idx, inset)
        if not bbox:
            print(f"  {slot}: empty after trim")
            continue
        cx, cy, cw, ch = bbox
        shutil.copy(src, OUT / "sheets" / sheet)
        sdef = slots[slot]
        cb, pv = sdef["default_bounds"], sdef["pivot"]
        instances.append({
            "id": f"{slot}/{variant}", "slot": slot, "variant": variant, "view": "front",
            "source": f"sheets/{sheet}",
            "region": {"x": cx, "y": cy, "w": cw, "h": ch},
            "canonical_bounds": {"x": cb["x"], "y": cb["y"], "width": cb["width"], "height": cb["height"]},
            "pivot": [pv[0], pv[1]], "angle": 0, "mirror_ok": False,
        })
        print(f"  bound {slot}/{variant}: trimmed {cw}x{ch} -> canonical {cb['width']}x{cb['height']} @ {cb['x']},{cb['y']}")

    (OUT / "manifest.json").write_text(
        json.dumps({"definition_id": definition["id"], "instances": instances}, indent=2),
        encoding="utf-8")

    # coverage vs all part slots
    part_slots = [n for n, s in slots.items() if s.get("pivot")]
    bound = {i["slot"] for i in instances}
    missing = [s for s in part_slots if s not in bound]
    print(f"\nBound {len(bound)}/{len(part_slots)} part slots.")
    print("Missing (isolated part not available / not yet bound):")
    print("  " + ", ".join(sorted(missing)))
    print(f"\nRender: cargo run -q -p deskfolk-render-win --example render_modular -- \"{OUT}\" \"{OUT/'assembled.png'}\" 3")


if __name__ == "__main__":
    main()
