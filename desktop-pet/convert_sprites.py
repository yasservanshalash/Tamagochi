"""One-time converter: Watcher LVGL C sprite arrays -> PNG files.

The firmware stores sprites as LVGL image descriptors:
  - LV_IMG_CF_TRUE_COLOR_ALPHA : 3 bytes/px = RGB565 (byte-swapped) + alpha
  - LV_IMG_CF_TRUE_COLOR       : 2 bytes/px = RGB565 (byte-swapped)
LV_COLOR_16_SWAP is enabled on the device, so color bytes are [hi, lo].

Usage: python convert_sprites.py
Writes PNGs into ./assets/
"""
import re
import sys
from pathlib import Path

import numpy as np
from PIL import Image

SRC = Path(__file__).resolve().parents[1] / "firmware" / "examples" / \
    "lumen-watcher" / "main" / "pet"
OUT = Path(__file__).resolve().parent / "assets"


def convert(cfile: Path) -> str:
    text = cfile.read_text()
    m = re.search(r"_map\[\]\s*=\s*\{(.*?)\};", text, re.S)
    if not m:
        return f"skip {cfile.name} (no map array)"
    raw = np.array([int(b, 16) for b in
                    re.findall(r"0x([0-9a-fA-F]{2})", m.group(1))],
                   dtype=np.uint8)
    w = int(re.search(r"\.header\.w\s*=\s*(\d+)", text).group(1))
    h = int(re.search(r"\.header\.h\s*=\s*(\d+)", text).group(1))
    has_alpha = "TRUE_COLOR_ALPHA" in text
    bpp = 3 if has_alpha else 2
    if len(raw) < w * h * bpp:
        return f"skip {cfile.name} (short data {len(raw)} < {w*h*bpp})"
    px = raw[: w * h * bpp].reshape(h, w, bpp).astype(np.uint16)

    c = (px[:, :, 0] << 8) | px[:, :, 1]          # swapped: [hi, lo]
    r = ((c >> 11) & 0x1F) * 255 // 31
    g = ((c >> 5) & 0x3F) * 255 // 63
    b = (c & 0x1F) * 255 // 31
    a = px[:, :, 2] if has_alpha else np.full((h, w), 255, dtype=np.uint16)

    rgba = np.dstack([r, g, b, a]).astype(np.uint8)
    name = cfile.stem + ".png"
    Image.fromarray(rgba, "RGBA").save(OUT / name)
    return f"ok   {name}  {w}x{h}  {'RGBA' if has_alpha else 'RGB'}"


def main():
    OUT.mkdir(exist_ok=True)
    files = sorted(SRC.glob("img_*.c"))
    if not files:
        sys.exit(f"no sprite files found in {SRC}")
    for f in files:
        print(convert(f))
    print(f"\n{len(files)} files -> {OUT}")


if __name__ == "__main__":
    main()
