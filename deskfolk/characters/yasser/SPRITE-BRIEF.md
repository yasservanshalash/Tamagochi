# Sprite brief — Yasser v1

Ten frames. They unlock the behaviours the engine can already drive but has no
art for. Everything here is measured from your existing 54 sprites, so new
frames drop in without touching code.

## Hard rules (from the existing set)

| Rule | Value | Why |
|---|---|---|
| Format | PNG, RGBA | Alpha is already how transparency works — **never** a green screen |
| Alpha | Binary only: 0 or 255 | Your current set has **zero** partial-alpha pixels across 1.66M. Anti-aliased edges would halo against the desktop |
| Canvas | Trim tight to the ink | No padding. Every existing sprite has a **0px bottom gap** |
| Bottom row | The contact point | His feet / the beanbag base. The engine anchors bottom-centre, which is what stops him bobbing when clips swap |
| Height — full body | **238 px** | Seated and standing poses. Width free (yours run 114–252) |
| Height — bust | **176 px** | Head-and-shoulders (talk/expression). Width ~167–197 |
| Palette | Match existing | Same browns/creams, same outline weight |
| Scale | 1:1 with current art | Drawn at the same pixel density — do not draw at 2x and downscale |

Anything that breaks these still loads; the validator will just warn, and it
will look subtly wrong next to the rest.

## The ten frames

Ordered by how much aliveness each buys. If you only do four, do 1–4.

### 1–2. `img_y_look_l` / `img_y_look_r` — 238 tall
Seated on the beanbag, mug held as in `img_y_sitb`, **head turned** left/right.
Body stays put; this is a head-and-eyes turn, roughly 20–30°.

*Why:* cursor tracking is currently a whole-body lean because there is no head
turn. This is the single biggest upgrade — he stops sliding and starts looking.

### 3. `img_y_sip` — 238 tall
Mug raised to his mouth, mid-drink, eyes down. A pose, not a sequence — the
engine holds it and returns to idle.

*Why:* "notice him quietly making coffee" is in your brief twice. Right now he
holds a mug he never drinks from.

### 4. `img_y_yawn` — 238 tall
Seated, mouth wide, eyes shut, one hand up or the mug lowered.

*Why:* fires on the run-up to a night nap, so sleep gets a build-up instead of
snapping from awake to asleep.

### 5. `img_y_glance_down` — 238 tall
Seated, head tipped down-right, eyes toward the bottom-right corner.

*Why:* the taskbar/notification glance. Held ~700ms, then back to idle.

### 6. `img_y_look_up` — 238 tall
Head tilted up, eyes raised.

*Why:* reacting to something above him — a window opening, a notification
toast. Also reads as "thinking" without reusing the think pose.

### 7–8. `img_y_blink_l` / `img_y_blink_r` — 238 tall
Eyes-closed variants of frames 1–2.

*Why:* without these he cannot blink while looking sideways, so he freezes
mid-glance for the whole blink interval — very noticeable once you see it.

### 9. `img_y_sitmug2` — 238 tall
A second idle variant: same seated pose, small change — shifted weight, mug
tilted, other hand moved.

*Why:* the idle loop is 3 frames and starts reading as a loop after a minute.
One more variant roughly halves that.

### 10. `img_y_startled_l` — 238 tall
Recoiling away from something on his left, eyes wide.

*Why:* a directional startle when the cursor lunges at him. The existing
startle is symmetric and reads as generic.

## How to actually make them

### Don't draw from scratch — edit a copy

This is the whole trick. Your art has **3,595 distinct colours**; it is richly
shaded, not a tight indexed palette. Nobody reproduces that freehand and gets a
match. So every new frame starts as a duplicate of the closest existing one:

| New frame | Start from | What you change |
|---|---|---|
| `look_l` / `look_r` | `img_y_sitb` | Head + eyes only. Body, mug, beanbag untouched |
| `blink_l` / `blink_r` | your new `look_l`/`look_r` | Eyes closed |
| `sip` | `img_y_sitmug` | Raise the mug, tip the head, eyes down |
| `yawn` | `img_y_sitb` | Open mouth, close eyes |
| `glance_down` | `img_y_sitb` | Eyes + slight head tilt down-right |
| `look_up` | `img_y_sitb` | Eyes + slight head tilt up |
| `sitmug2` | `img_y_sitmug` | Shift weight, tilt the mug, move a hand |
| `startled_l` | `img_y_expr1` | Lean/recoil to his left |

Because the body is untouched pixel-for-pixel, the character never "jumps"
when the clip swaps — which is exactly the thing that breaks the illusion.

### Tool

**Aseprite** (~$20, or free if you build it from source) is the right tool —
it is built for exactly this. Free alternatives that work: **LibreSprite** (an
Aseprite fork), **Krita**, or **GraphicsGale**. Photoshop is fine if you already
know it.

Whatever you use, the two settings that matter:

- **Pencil tool, not brush.** Hard edges, no feathering.
- **Anti-aliasing OFF.** Every soft pixel becomes a halo against the wallpaper.
  In Aseprite this is off by default; in Photoshop turn it off on the Pencil,
  Eraser, *and* the Magic Wand / selection tools.

### Palette

`characters/yasser/palette.gpl` is generated from your own sprites — 47
representative colours. Load it:

- **Aseprite** → Palette menu → *Load Palette* → pick the `.gpl`
- **Krita / GIMP** → import as a palette resource
- **Photoshop** → convert with a `.gpl` → `.aco` tool, or just eyedropper

Honestly though: **the eyedropper beats the palette.** Sample directly from the
sprite you are editing and you cannot drift.

### Export

- PNG, **32-bit / RGBA**
- Background layer **deleted or hidden** — not filled with white, black, or
  green. There is nothing to key out; alpha does this natively
- **Trim tight** before saving (Aseprite: *Sprite → Trim*) — no empty rows below
  the artwork, or he floats
- Save into `characters/yasser/sprites/` using the exact names in the list above

### Check before you wire anything

```powershell
python tools/check_sprites.py img_y_look_l.png
python tools/check_sprites.py            # or check everything
```

It catches the four mistakes that actually happen: a non-RGBA export, an
anti-aliased edge, a stray empty row under the feet, and a wrong height. All 54
existing sprites pass it clean, so any error is genuinely yours.

### A note on AI-generated frames

If you generate these rather than hand-draw them, expect to still do a cleanup
pass: generators produce soft edges and near-miss colours, both of which the
checker will flag. Generate at the same scale, then pencil over the edges and
eyedropper the colours back.

## Dropping them in

Save into `characters/yasser/sprites/`, then add clips to `character.json`.
The engine references clips by **role**, so once these exist I wire:

```jsonc
"look_left":  { "frames": [{ "img": "img_y_look_l", "ms": 900 }] },
"sip":        { "frames": [{ "img": "img_y_sip",    "ms": 1400 }] },
"yawn":       { "frames": [{ "img": "img_y_yawn",   "ms": 1200 }] }
```

and add them to `life.fidgets` / the new gaze roles. Nothing in `crates/`
changes — that is the whole point of the package format.

> **Regenerating:** `character.json` is produced by
> `tools/build_yasser_package.py` from the alpha's `clips.py`. Add new clips to
> the generator, not by hand-editing the JSON, or the next run overwrites them.

## Checking your work

```powershell
cargo test -p deskfolk-package
```

Validates every frame exists, every role resolves, and every sprite path is
sane. It will fail loudly on a typo'd filename rather than showing a gap on
your desktop.
