#!/usr/bin/env python3
"""Proof-of-concept: generate a SINGLE isolated part with a hardened prompt,
reusing the working OpenRouter path from generate_sprites.py. One part per
image, aggressive isolation, no grid/labels/shadow."""
import sys, os
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import generate_sprites as gs

PACK = Path(r"C:\Users\yasse\Desktop\code\Tamagochi\deskfolk\deskfolk_flux_generation_pack\deskfolk_flux_pack")
REF = PACK / "references" / "master_reference_sheet.png"
OUT = Path(__file__).resolve().parent.parent / "characters" / "yasser" / "modular_regen"
OUT.mkdir(parents=True, exist_ok=True)

ISO_RULES = (
    "CRITICAL ISOLATION RULES (obey exactly):\n"
    "- Draw ONLY the one part described. NOTHING else.\n"
    "- The entire rest of the image MUST be 100% transparent (alpha 0). No background.\n"
    "- NO text, NO labels, NO captions, NO letters, NO numbers, NO arrows, NO guides.\n"
    "- NO drop shadow, NO ground shadow, NO glow, NO grid, NO panels, NO borders.\n"
    "- Exactly ONE instance of the part, centered. NOT a sheet, NOT multiple views.\n"
    "Style: chunky pixel art, hard 1px black outline, flat 2-3 tone shading, the "
    "reference's warm brown palette, crisp integer pixels, absolutely no anti-aliasing.\n"
    "Use the attached master reference ONLY for identity, palette and style."
)

PARTS = {
    "torso_front": (
        "Output a SINGLE ISOLATED sprite of Yasser's TORSO ONLY: the brown quilted "
        "puffer jacket over the dark-brown hoodie, FRONT view, from the base of the "
        "neck down to the hips. Absolutely NO head, NO neck, NO arms, NO hands, NO "
        "legs. Just the jacket torso block, centered."
    ),
    "head_front": (
        "Output a SINGLE ISOLATED sprite of Yasser's HEAD ONLY, FRONT view, matching the "
        "attached reference FACE CLOSEUP as closely as possible. Reproduce faithfully: an "
        "olive-brown knitted beanie pulled LOW over the forehead; large cream/beige "
        "over-ear headphones with a tan headband arcing over the beanie; warm-tan skin; "
        "THICK straight dark eyebrows; heavy-lidded, half-closed relaxed eyes with dark "
        "irises (calm, chill expression); a short straight nose; a FULL dark-brown beard "
        "and moustache connected around the mouth; a slightly oversized rounded head. Cut "
        "off cleanly at the neck. Chunky pixel art, hard 1px black outline, flat 2-3 tone "
        "shading, warm palette, no anti-aliasing. NO shoulders, NO hood, NO jacket, NO body."
    ),
    "arm_left": (
        "Output a SINGLE ISOLATED sprite of ONE SLIM arm (Yasser's, viewer's-left), FRONT "
        "view, hanging straight down close to the body: a NARROW brown quilted puffer "
        "sleeve of normal arm width, with a small bare warm-tan hand relaxed at the "
        "bottom. Keep it SLIM — NOT a bulky block — and do NOT include a big shoulder pad; "
        "the sleeve starts just below the shoulder. Chunky pixel art, hard 1px outline, "
        "flat shading. NO head, NO torso, NO other arm, NO legs; transparent elsewhere."
    ),
    "arm_right": (
        "Output a SINGLE ISOLATED sprite of ONE SLIM arm (Yasser's, viewer's-right), FRONT "
        "view, hanging straight down close to the body: a NARROW brown quilted puffer "
        "sleeve of normal arm width, with a small bare warm-tan hand relaxed at the "
        "bottom. Keep it SLIM — NOT a bulky block — and do NOT include a big shoulder pad; "
        "the sleeve starts just below the shoulder. Chunky pixel art, hard 1px outline, "
        "flat shading. NO head, NO torso, NO other arm, NO legs; transparent elsewhere."
    ),
    "upper_body_front": (
        "Output a SINGLE ISOLATED sprite of Yasser's UPPER BODY ONLY, FRONT view: the "
        "brown quilted puffer jacket over a dark-brown hoodie, WITH both puffer sleeves "
        "AND both small bare warm-tan hands hanging relaxed at his sides — one unified, "
        "rounded, stocky puffer silhouette from the base of the neck down to the hips "
        "(and the hands at the wrists). Absolutely NO head, NO neck above the collar, NO "
        "legs below the jacket hem. Chunky pixel art, hard 1px black outline, flat 2-3 "
        "tone shading, warm palette, no anti-aliasing."
    ),
    "legs_front": (
        "Output a SINGLE ISOLATED clothing sprite: a PAIR OF TROUSER-LEGS WITH BOOTS and "
        "NOTHING above the waistband — like empty trousers standing on their own. Dark "
        "charcoal-brown puffer trousers and brown boots, FRONT view, standing, feet "
        "together and flat. The image MUST START at the top of the trouser waistband and "
        "go down to the soles. It is ONLY the lower half. Absolutely NO upper body, NO "
        "jacket, NO torso, NO belly, NO chest, NO arms, NO hands, NO shoulders, NO head, "
        "NO face — do NOT draw a whole person. Just the trousers and boots, centered."
    ),

    # --- expression + gesture variants (front) ---
    "head_talk": (
        "Output a SINGLE ISOLATED sprite of Yasser's HEAD ONLY, FRONT view, MOUTH OPEN "
        "as if mid-speech (talking): olive-brown beanie, cream over-ear headphones, "
        "warm-tan face, thick eyebrows, heavy-lidded eyes, full dark-brown beard, mouth "
        "open showing a talking shape. Cut off at the neck. NO shoulders, NO hood, NO "
        "jacket, NO body."
    ),
    "head_blink": (
        "Output a SINGLE ISOLATED sprite of Yasser's HEAD ONLY, FRONT view, EYES CLOSED "
        "(mid-blink): same olive-brown beanie pulled low, cream over-ear headphones, "
        "warm-tan face, thick eyebrows, full dark-brown beard — identical to the neutral "
        "head but with both eyes gently CLOSED (simple curved closed eyelids, relaxed). "
        "Cut off at the neck. Chunky pixel art, hard 1px outline, flat shading. NO "
        "shoulders, NO hood, NO jacket, NO body."
    ),
    "head_happy": (
        "Output a SINGLE ISOLATED sprite of Yasser's HEAD ONLY, FRONT view, HAPPY smiling "
        "expression: olive-brown beanie, cream over-ear headphones, warm-tan face, raised "
        "cheeks, warm smile through the full dark-brown beard, friendly eyes. Cut off at "
        "the neck. NO shoulders, NO hood, NO jacket, NO body."
    ),
    "arm_right_wave": (
        "Output a SINGLE ISOLATED sprite of ONE ARM ONLY — a brown puffer-jacket sleeve "
        "with a bare warm-tan hand — RAISED and BENT UPWARD in a friendly WAVE, the open "
        "hand at the top. Just a disconnected arm, like a detached sleeve, floating and "
        "centered. Absolutely NO head, NO face, NO torso, NO chest, NO body, NO other arm, "
        "NO legs, NO person — do NOT draw a character or bust. ONLY the single raised arm."
    ),

    # --- angle variants (for turning) ---
    "head_side": (
        "Output a SINGLE ISOLATED sprite of Yasser's HEAD ONLY, LEFT SIDE PROFILE view: "
        "olive-brown beanie, one cream over-ear headphone cup, warm-tan face in profile, "
        "beard, heavy-lidded eye. Cut off at the neck. NO shoulders, NO jacket, NO body."
    ),
    "head_3q": (
        "Output a SINGLE ISOLATED sprite of Yasser's HEAD ONLY, THREE-QUARTER view "
        "(turned slightly left): beanie, cream headphones, warm-tan face, beard, "
        "heavy-lidded eyes. Cut off at the neck. NO shoulders, NO jacket, NO body."
    ),
    "head_back": (
        "Output a SINGLE ISOLATED sprite of the BACK OF Yasser's HEAD ONLY: olive-brown "
        "beanie from behind, the tan headband of the headphones and the back of both cream "
        "ear-cups, hair/neck. NO face, NO shoulders, NO jacket, NO body."
    ),
    "torso_side": (
        "Output a SINGLE ISOLATED sprite of Yasser's TORSO ONLY, LEFT SIDE PROFILE: the "
        "brown quilted puffer jacket from the neck to the hips, side view. NO head, NO "
        "arms, NO legs."
    ),
    "torso_back": (
        "Output a SINGLE ISOLATED sprite of Yasser's TORSO ONLY, BACK view: the brown "
        "quilted puffer jacket from behind, neck to hips, with the bunched hood. NO head, "
        "NO arms, NO legs."
    ),
    "legs_side": (
        "Output a SINGLE ISOLATED clothing sprite of a PAIR OF TROUSER-LEGS WITH BOOTS, "
        "LEFT SIDE PROFILE, standing — empty trousers, nothing above the waistband. Dark "
        "charcoal-brown puffer trousers, brown boots. NO upper body, NO jacket, NO arms, "
        "NO head."
    ),
    "legs_back": (
        "Output a SINGLE ISOLATED clothing sprite of a PAIR OF TROUSER-LEGS WITH BOOTS, "
        "BACK view, standing — empty trousers, nothing above the waistband. Dark "
        "charcoal-brown puffer trousers, brown boots from behind. NO upper body, NO "
        "jacket, NO arms, NO head."
    ),
    "legs_side_walk1": (
        "Output a SINGLE ISOLATED clothing sprite: a PAIR OF TROUSER-LEGS WITH BOOTS in a "
        "SIDE-PROFILE WALKING STEP — the NEAR leg striding FORWARD and the far leg back, "
        "mid-stride, as if walking to the right. Dark charcoal-brown puffer trousers, brown "
        "boots. Empty trousers, nothing above the waistband. NO upper body, NO jacket, NO "
        "arms, NO head. Chunky pixel art, hard 1px outline, flat shading."
    ),
    "legs_side_walk2": (
        "Output a SINGLE ISOLATED clothing sprite: a PAIR OF TROUSER-LEGS WITH BOOTS in a "
        "SIDE-PROFILE WALKING STEP — the FAR leg striding FORWARD and the near leg back "
        "(the opposite stride to a normal step), mid-stride, walking to the right. Dark "
        "charcoal-brown puffer trousers, brown boots. Empty trousers, nothing above the "
        "waistband. NO upper body, NO jacket, NO arms, NO head. Chunky pixel art, hard 1px "
        "outline, flat shading."
    ),
    "arm_side": (
        "Output a SINGLE ISOLATED sprite of ONE of Yasser's arms, LEFT SIDE PROFILE, "
        "hanging down: brown puffer-jacket sleeve from shoulder to wrist plus the bare "
        "warm-tan hand. NO head, NO torso, NO other arm, NO legs."
    ),
}

def main():
    key = gs.openrouter_key(PACK)
    if not key:
        sys.exit("no OpenRouter key")
    model = "google/gemini-2.5-flash-image"
    args = sys.argv[1:]
    # optional: --n K  → generate K candidates per part as <name>_c1.png ..
    n = 1
    if "--n" in args:
        i = args.index("--n"); n = int(args[i + 1]); del args[i:i + 2]
    only = args or list(PARTS)
    for name in only:
        desc = PARTS[name]
        prompt = desc + "\n\n" + ISO_RULES
        for c in range(1, n + 1):
            dst = OUT / (f"{name}.png" if n == 1 else f"{name}_c{c}.png")
            print(f"generating {dst.name} ...", flush=True)
            try:
                gs.openrouter_generate(prompt, REF, dst, None, model, key, None)
                print(f"  -> {dst}")
            except gs.GenError as e:
                print(f"  FAILED: {e}")

if __name__ == "__main__":
    main()
