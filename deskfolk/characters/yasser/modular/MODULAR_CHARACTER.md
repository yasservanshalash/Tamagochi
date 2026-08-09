# Modular Yasser — architecture & authoring guide

Yasser can be assembled from independent sprite **parts** layered on a shared
**320×320** canvas, instead of one whole-character sprite per frame. This lets a
wave move only the right arm, a blink swap only the eyes, and a coffee cup appear
only in the right hand — the rig the spec (`definition.json`) describes.

This document covers the pipeline, the coordinate system, the manifest schema,
and — because the currently generated art does **not** conform (see
`ASSET_AUDIT.md`) — how to regenerate parts that do.

## The pipeline

```
CharacterDefinition   definition.json — slots, pivots, anchors, layer order   (spec, verbatim)
        │             engine::modular::definition
        ▼
SpriteManifest        manifest.json — parts → sheet regions in 320-space
        │             engine::modular::manifest   ·  validate() reports gaps
        ▼
CharacterState        which variant per slot, view, hidden layers
        │             engine::modular::state       ·  canonical_yasser() = defaults
        │  ▲
        │  └── AnimationPreset  mutates only the slots a gesture names
        │      engine::modular::preset  (from definition.gesture_presets)
        ▼
CompatibilityResolver rejects impossible combinations, reports (never repairs)
        │             engine::modular::compat
        ▼
LayerResolver         back-to-front draw list; reports slots with no bound sprite
        │             engine::modular::layer
        ▼
CharacterRenderer     render-win::modular_paint — one region-blit per part,
                      global scale, integer positions, nearest-neighbour
```

`engine::modular::audit` scores a manifest against the definition (how many part
slots are conforming vs missing vs flawed) and renders Markdown.

Nothing here touches the existing single-sprite `ClipPlayer`/`compose`/`paint`
path — the modular pipeline is entirely additive and dev-gated behind
`DESKFOLK_MODULAR=1` (`render_win::modular_enabled()`).

## Coordinate system

- Authoring canvas: **320×320**, origin top-left.
- `center_x = 160`, `ground_y = 290`. Character bounds ≈ `x96 y30 128×260`.
- Every part is authored **at this exact scale and position**. A part is never
  resized independently at runtime — only the whole character shares one global
  scale (`unit`, device-pixels-per-320-unit). This is enforced structurally:
  `blit_scaled_src` maps a sheet region straight onto the part's
  `canonical_bounds`; there is no per-part scale knob.
- Each slot has a **pivot** (its fixed attachment point) and, for connectors,
  **connection anchors** (e.g. a torso's `neck`, `left_shoulder`). A conforming
  part places its pivot at exactly the slot's canonical pivot so parts stay
  interchangeable. `manifest::validate` flags a pivot/bounds drift > 2px.

## Layer order (front)

`definition.layer_order_front` (17 rig layers, back→front). The concrete slots
that fill each rig layer live in `LayerResolver::slots_for_layer` (Rust) and the
mirrored `LAYER_SLOTS` map in the inspector:

```
back_accessory · back_hair_or_hood · rear_arm(L) · rear_leg(L) ·
torso(pelvis,torso) · front_leg(R) · front_arm(R) · neck · head_base · ears ·
face_features(eyes,eyebrows,nose,mouth) · beard · beanie · headphones ·
handheld_prop(props) · front_accessory · effects
```

Front view maps screen-left limbs to the rear layers and screen-right to the
front layers — a stable symmetric default. Other views are scaffolded via a
`view` field but only `front` is wired in this pass.

## Manifest schema (`manifest.json`)

```jsonc
{
  "definition_id": "deskfolk_yasser_modular_v1",
  "instances": [
    {
      "id": "right_hand/peace",        // stable, conventionally slot/variant[/view]
      "slot": "right_hand",
      "variant": "peace",
      "view": "front",                  // front | 3q_left | side_left | back | ...
      "source": "sheets/right_hand.png",// relative to this manifest's dir
      "region":  { "x": 812, "y": 40, "w": 120, "h": 150 },  // cell rect in the sheet
      "canonical_bounds": { "x": 198, "y": 198, "width": 21, "height": 25 }, // 320-space dest
      "pivot": [209, 207],              // must equal the slot's spec pivot
      "anchors": { },                   // optional connection anchors in 320-space
      "angle": 0,                       // degrees about the pivot; 0 for canonical art
      "mirror_ok": false                // true ONLY for artist-certified symmetrical art
    }
  ]
}
```

The shipped manifest is **empty** by design. Populate it with the inspector.

## The inspector (`tools/sprite_inspector.html`)

A self-contained, no-build tool (open in a browser, or the app can host it):

1. **Load** `definition.json` (or "Auto-load spec" via fetch) and the sheet PNGs.
2. **Auto-slice** — each sheet's transparent gutters are detected and every cell
   trimmed to its opaque bounding box; it copes with the irregular 4×2 / 6×4
   reality and shows what each batch actually contains.
3. **Bind** a cell → slot / variant / view. The binding fits the trimmed cell
   into the slot's canonical bounds + pivot from the spec (the off-canvas→320
   bridge).
4. **Preview** the assembled canonical Yasser live, with overlays for canonical
   bounds, pivots, the skeleton, and layer names.
5. **Reset to canonical Yasser** and **Export manifest.json** (drop it into this
   directory; the Rust runtime and the integration tests read it).

The inspector's `LAYER_SLOTS` and canonical-assembly logic mirror the Rust
`LayerResolver`, so the preview matches what `modular_paint` will draw.

## Regenerating clean parts (the working recipe)

The batch sheets in the flux pack are non-conforming (full-character busts). Clean
isolated parts come from generating **one part per image** with a hardened prompt:

- `tools/regen_part.py` — generates isolated parts (reuses `generate_sprites.py`'s
  OpenRouter path). Prompts scream isolation: "ONLY this part, NOTHING else, fully
  transparent, no text/shadow/grid". Some parts (legs, a waving arm) need one
  forceful reroll — the model likes to draw a whole person.
- **Gotcha:** the model returns opaque RGB with a neutral-gray/checker background,
  not real alpha. `tools/assemble_regen.py::key_background` removes it by
  **flood-filling transparency inward from the edges** over near-neutral light
  pixels — so warm light parts (the cream headphones) survive.
- `tools/assemble_regen.py` — keys + trims the regenerated parts and binds each to
  its slot at the UNION of that region's fine-slot bounds → `characters/yasser/
  modular_assembled/{sheets, manifest.json}`.
- `tools/pose_demo.py` — proves modular animation: wave changes only the right arm,
  talk/happy only the head → `modular_assembled/poses/poses_montage.png`.
- `tools/angles_demo.py` — front/3q/side/back turnaround (aspect-preserving,
  centered) → `modular_assembled/angles/turnaround.png`.

## Running the Rust path

**Headless render (a PNG):**
```
cargo run -p deskfolk-render-win --example render_modular -- \
    characters/yasser/modular_assembled out.png 3
# overlays: OVERLAY_BOUNDS=1 OVERLAY_PIVOTS=1 cargo run ...
```

**Live companion window:** launch the app with
```
DESKFOLK_MODULAR=1                 # render_win::modular_enabled()
DESKFOLK_MODULAR_DIR=<abs path>    # optional; defaults to <pkg>/modular_assembled
```
The window then assembles the modular Yasser (sized to the 320 canvas) in the real
layered window instead of the classic single-sprite frames. With the flag off the
shipping companion is untouched; if the scene fails to load it logs and falls back.

`render_win::ModularScene::load(dir)` loads the spec + manifest + sheets;
`scene.paint(canvas, &state, unit, overlays)` assembles it and returns how many
parts drew, logging every slot with no bound sprite. End-to-end coverage:
`crates/render-win/tests/modular_render.rs`.

## Regeneration — making parts that conform

The generated sheets failed because the model drew whole characters. To get
isolated parts, harden the prompts used by `tools/generate_sprites.py` and
regenerate the non-conforming slots. Make these **non-negotiable** in the prompt:

- **One part only.** "Draw ONLY the {slot}. No head, no torso, no other limb —
  the rest of the 320×320 canvas is fully transparent." Name explicitly what must
  be absent (e.g. for `head_base`: "a bare head — NO beanie, NO headphones, NO
  beard, NO hood").
- **Exact canvas & placement.** "The image is exactly the 320×320 authoring
  canvas. Place the part so its pivot is at pixel ({px},{py}) and its bounds are
  {x},{y},{w}×{h}." (Values come straight from the slot in `definition.json`.)
- **Correct output size.** Force a 2:1 sheet at 1280×640 for a 4×2 grid of 320
  cells — or, more reliably, **generate one 320×320 part per image** and skip
  sheets entirely (the model holds a single-cell layout far better than a grid).
- **No labels, no shadow, no AA.** "No text, captions, numbers, arrows, guides,
  grid lines. No drop shadow or ground shadow. Hard 1px outlines, flat 2–3 tone
  shading, transparent background."
- **Verify before accepting.** In the generator's keep/regenerate loop, reject
  any output with baked text, a shadow, or content outside the slot's bounds.

Because the model resists grids and isolation, prefer **one part per image at
320×320** and lean on the reference for identity. Then load the parts into the
inspector, which will place them on-canvas and let you bind them. Once bound,
`audit` flips the slot to ✅ and both the inspector preview and the Rust
`modular_paint` path render the assembled Yasser with no further code.
