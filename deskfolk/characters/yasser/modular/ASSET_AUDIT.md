# Modular asset conformance — `deskfolk_yasser_modular_v1`

_Audit of the generated sprite sheets against the modular contract in
`definition.json`. Generated once by hand from a direct inspection of the sheets;
the runtime equivalent is `engine::modular::audit`, and the web inspector
(`tools/sprite_inspector.html`) recomputes pixel-level detail live._

## Bottom line

**0 of 20 part slots are conforming.** The 42 curated sheets in
`deskfolk_flux_generation_pack/.../generated/accepted/` are high-quality
**full-character reference illustrations**, not the isolated, canonically-placed
modular parts the spec requires. They cannot be layered as-is. The shipped
`manifest.json` is therefore empty, and `audit` reports every part slot as
_missing_ rather than inventing a binding.

## What was generated

`progress.json` maps 42 accepted sheets covering nearly every slot
(torso, head_base, beanie, headphones, eyes, eyebrows, nose, mouth, beard, both
upper arms / forearms / hands, pelvis, both legs / boots, props, effects). So
coverage of _slots_ is broad — the problem is the _content_ of each sheet.

## Four systemic non-conformances

1. **Wrong dimensions.** Every accepted sheet is **1248×832**, never the spec's
   1280×640. 1248/4 = 312 and 832/2 = 416, so there is no clean 320-pixel grid
   and no cell sits on canonical coordinates.

2. **Composed characters, not isolated parts.** Cells are whole or partial
   assemblies:
   - `torso__batch_01` — 8 **full-body** Yassers (head + beanie + headphones +
     beard + jacket + trousers + boots).
   - `head_base__batch_01` — 8 **complete busts**: face + beanie + headphones +
     hood already fused. No bare head.
   - `right_hand__batch_01` — a **6×4** grid mixing busts _and_ hands.
   - `eyes__batch_01` — face crops, but with eyebrows fused in.
   You cannot layer a head over a torso when the torso already contains the head.

3. **Prohibited elements baked in.** Text labels ("FRONT", "SIDE", "TIRED", plus
   garbled AI text like "SUPSICITUSES", "THINKINSSED"), drop shadows under the
   feet, and stray "?" glyphs — all banned by `style_lock` / `generation_contract`.

4. **Inconsistent grids.** Some sheets are 4×2, some 6×4; per-cell scale and
   placement vary sheet to sheet.

Transparency _is_ present (clean alpha), which is the one thing that went right.

## Why this happened

The image model (Gemini "nano-banana" via OpenRouter) strongly prefers to draw a
single appealing character. Asked for "one isolated right hand on a 320 canvas",
it drew a themed sheet _of a guy and some hands_. The modular contract fights the
tool's grain; the prompts need to be much more forceful (see
`MODULAR_CHARACTER.md` → Regeneration).

## Per-slot status (against the empty manifest)

Every part slot below is **❌ missing** until conforming art is bound. This table
is what `audit(&def, &manifest).to_markdown()` prints; it flips to ✅ as the
inspector binds cells.

| slot group | slots | generated sheets exist? | conforming? |
| --- | --- | --- | --- |
| torso / pelvis | torso, pelvis | yes | ❌ (full-body / labelled) |
| head | head_base | yes | ❌ (full bust) |
| face | eyes, eyebrows, nose, mouth | yes | ❌ (fused / labelled) |
| facial hair | beard | yes | ❌ (attached to bust) |
| headwear | beanie, headphones | yes | ❌ (attached to bust) |
| arms | left/right upper_arm, forearm, hand | yes | ❌ (mostly busts) |
| legs | left/right leg, boot | yes | ❌ (full-body) |
| held / fx | props, effects | yes | ⚠ partially isolated, unaligned |

## Path forward

1. Curate any genuinely-isolated cells that _do_ exist (some hand poses, some
   effects) with the inspector, snapping them to canonical bounds — partial
   assembly is possible for those.
2. Regenerate the rest as true isolated parts with hardened prompts
   (`MODULAR_CHARACTER.md` → Regeneration), then bind them in the inspector.
3. The engine, renderer, and inspector are already built and tested; they light
   up the moment conforming parts land — no further code required for front view.
