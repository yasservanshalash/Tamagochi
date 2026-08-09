# COMPLETE GENERATION PROMPT

Attach before using this prompt:

1. `deskfolk_yasser_modular_character_v1.json`
2. The canonical Yasser master reference sheet

Copy everything below into the image generator.

---

# GLOBAL COMPLETE

```text
Use the uploaded modular JSON as the authoritative structural specification.
Use the uploaded master reference sheet as the ONLY canonical visual reference.

CHARACTER LOCK

Adult male.
Warm tan skin.
Full dark-brown beard connected to moustache.
Thick dark eyebrows.
Heavy-lidded dark-brown eyes.
Olive-brown knitted beanie pulled low.
Large cream over-ear headphones with tan headband.
Brown quilted puffer jacket.
Dark-brown hoodie underneath.
Dark charcoal-brown trousers.
Brown boots.

BODY PROPORTIONS

Height: exactly 260 px.
Short, stocky body.
Rounded silhouette.
Slightly oversized head.
Short limbs.
Small hands and boots.

STYLE LOCK

Pixel art.
Chunky visible pixels.
Hard 1px outlines.
Nearest-neighbour.
Flat 2–3 tone cel shading.
No anti-aliasing.
No gradients.
No blur.
No glow.
No bloom.
No soft edges.
No drop shadows.
No ground shadows.

CANVAS

Transparent background.
Authoring canvas: 320×320 px.
Character centre: X=160.
Ground line: Y=290.

JSON RULES

The uploaded JSON defines:
- slot bounds
- pivots
- anchors
- layer order
- palette
- compatibility
- dimensions

Always follow the JSON if anything conflicts.

OUTPUT RULES

Generate ONLY the requested module or sprite-sheet batch.
Keep every part at its canonical coordinates.
Do not crop.
Do not scale.
Do not move pivots.
Do not redraw unrelated body parts.
Provide 1–2 hidden overlap pixels at connection seams where appropriate.

NEGATIVE PROMPT

No realistic rendering.
No painterly texture.
No soft shading.
No extra limbs.
No extra accessories.
No redesign.
No alternate clothing.
No different beard.
No different face.
No different proportions.
No different colours.
No labels.
No guides.
No text.
No background.
No borders.

QUALITY CHECK

Every generated asset must be visually interchangeable with every other compatible asset.
```

---

# Mouth Sprite Sheet Batch 2

```text
Generate ONE sprite sheet containing the variants listed below.

SPRITE SHEET LAYOUT

Cell size:
320×320 px

Grid:
4 columns × 2 rows

Final image size:
1280×640 px

Background:
Fully transparent

PLACEMENT RULE

Each cell is a complete independent 320×320 authoring canvas.
Place the requested component at its canonical coordinates inside EACH cell.
Do not crop or scale the component.
Do not move the pivot.
Do not combine variants inside one cell.
Do not add labels, numbers, separators, guides or borders.
Unused cells must remain completely transparent.

COMPONENT

Mouth

Canonical bounds:
x=146, y=91, width=28, height=15

Canonical pivot:
x=160, y=98

VARIANT ORDER

Cell 1: Shout
Cell 2: Surprised O
Cell 3: Talk A
Cell 4: Talk E
Cell 5: Talk I
Cell 6: Talk O
Cell 7: Talk U
Cell 8: Mumble

CELL ORDER

Cell 1: top-left
Cell 2: top row, second
Cell 3: top row, third
Cell 4: top-right
Cell 5: bottom-left
Cell 6: bottom row, second
Cell 7: bottom row, third
Cell 8: bottom-right

Every variant must preserve the same character identity, pixel density, palette, outline thickness, lighting direction and attachment geometry.
Generate ONLY the requested component in every cell.
```

---

# FINAL EXECUTION INSTRUCTION

Generate the requested sprite sheet now.

Return one transparent PNG at the exact final sheet dimensions specified above.
Do not return explanations, labels, previews, mockups, contact sheets with captions, or alternate layouts.
