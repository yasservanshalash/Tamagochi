# Deskfolk

A runtime for AI characters that live on your desktop.

The LLM is one replaceable module. The **character** is the product, and every
character is data — a package the engine loads, not code the engine contains.
Yasser is the first package, ported 1:1 from the alpha in `../desktop-pet`.

> Every feature answers one question: *does this make the companion feel more
> alive?* If it only makes the application more complex, it doesn't belong.

## Layout

```
deskfolk/
├── crates/
│   ├── package/     .dfpk format — manifest, validator, alpha hit-masks
│   ├── engine/      the life loop, clip player, layout, hit-testing
│   ├── ai/          Provider trait + Anthropic / OpenAI-compatible / sidecar
│   ├── render-win/  the companion's native layered window + software renderer
│   └── app/         body: the 60Hz loop, voice, tray, Control Center
├── ui/              Control Center shell
├── characters/
│   └── yasser/      first character package (character.json + 54 sprites)
└── tools/
    ├── build_yasser_package.py   regenerates the package from the alpha
    └── heal_sprites.py           repairs dropped scanlines in the art
```

`heal_sprites.py` exists because of one specific inherited defect:
`img_y_sitb` and `img_y_sitc` carry a fully transparent row through his torso,
and have since the Watcher — the row is blank in `img_y_sitb.c` too. On a black
device screen a one-pixel transparent line through a dark hoodie is invisible;
on a yellow wallpaper it is a bright slash across his chest. The rule is
deliberately narrow (a *single* blank row between two near-solid ones, in
`img_y_*` only) because several effect sheets stack glyphs with real gaps that
must not be filled in.

The engine knows nothing about Yasser. It asks the package for *roles* — "the
sleep clip", "the talk clip" — and the package answers. That indirection is
what lets a frog or a dragon drop in with nothing recompiled.

## Running it

```powershell
.\run.ps1
```

That is the whole thing. It starts the brain on `:8087` if it is not already
up — he needs it for both his mind and his voice — and then starts him.
`.\run.ps1 -Release` builds optimised; `.\run.ps1 -NoBrain` leaves the brain
alone and lets him fall back to his offline lines.

Or without the launcher:

```powershell
cargo run -p deskfolk-app
```

The companion is native, so there is no UI dev server to start first — that
requirement died with the webview. Only the Control Center is a web page, and
it is opened on demand from his menu:

```powershell
cd ui;  npm install;  npm run dev      # only if you open the Control Center
```

### Environment

| Variable | Default | Meaning |
|---|---|---|
| `DESKFOLK_LOG` | `info` | Tracing filter. `deskfolk_lib=trace` logs every hit-test. |
| `DESKFOLK_CHARACTER` | `yasser` | Which package under `characters/` to load. |
| `DESKFOLK_SCALE` | `1.3` | Stage units to logical pixels. **Rounded to a whole number** — see below. |
| `DESKFOLK_PIXEL_SNAP` | on | `off` allows a fractional scale, at the cost of a ragged silhouette. |
| `DESKFOLK_PORTAL` | `untethered` | `untethered`, `circle`, or `rounded`. |
| `DESKFOLK_PROVIDER` | auto | `anthropic`, `openrouter`, `groq`, `ollama`, `sidecar`. |
| `DESKFOLK_MODEL` | per provider | Overrides the model for whichever provider is chosen. |
| `DESKFOLK_LAYER` | — | `desktop` parents him into the wallpaper, behind your icons. |
| `DESKFOLK_VOICE` | on | `off` for a silent companion — subtitles only. |
| `DESKFOLK_VOICE_URL` | brain / `PET_BRAIN` | Where speech is synthesised. |
| `DESKFOLK_VOICE_SELF_TALK` | off | `1` also speaks his idle muttering. Costs quota — see below. |
| `DESKFOLK_HOTKEY` | `ctrl+alt+y` | System-wide key that starts him listening. `off` for none. |

### The mind

Keys are read from the process environment **and from `../brain/.env`** — the
alpha's file, deliberately shared rather than duplicated, so an existing setup
works with no new configuration. Real environment variables win over the file,
and nothing here is ever logged.

With no `DESKFOLK_PROVIDER` set, the first key found wins, in this order:
`ANTHROPIC_API_KEY` → `OPENROUTER_API_KEY` → `GROQ_API_KEY` → `PET_API_BASE`
(Ollama) → the local sidecar. Startup logs the choice and the reason, with the
key redacted.

The OpenRouter default is
`cognitivecomputations/dolphin-mistral-24b-venice-edition` — picked *for this
character*, not in general. The alpha ran an uncensored local model on purpose
(`mistral-heretic`, `PET_KEEP_IT_REAL=1`) because ordinary assistant models
kept breaking a blunt persona; this is the closest hosted equivalent. Any other
character should set `DESKFOLK_MODEL`.

"OpenAI-compatible" is a spectrum, so the provider walks down a ladder:
`json_schema` → `json_object` → no `response_format` at all, retrying only when
the failure actually mentions the schema. That matters here — the default model
supports `response_format` but *not* `json_schema`, and a strict-only client
would simply fail against it.

The emotion enum in that schema is built from the loaded character package, so
a model can never return a mood the character has no clip for — and whatever
comes back is re-checked by `ThinkReply::sanitize` regardless.

## Why the companion is not a webview

He used to be a transparent WebView2 window drawing into a canvas, and he
looked right for about a second at a time: **WebView2's host window draws its
own minimise/maximise/close buttons and a border on hover, and nothing removes
them.** That was chased to the end — `WS_CAPTION`, `WS_SYSMENU`,
`WS_MINIMIZEBOX` and `WS_MAXIMIZEBOX` all stripped, a `WM_NCCALCSIZE` subclass
leaving *zero* non-client area (window rect == client rect, 536×536),
`DWMWA_BORDER_COLOR = NONE`, `DWMWA_WINDOW_CORNER_PREFERENCE = DONOTROUND`, the
accent policy disabled, and a periodic re-strip that found nothing left to
strip. The buttons kept rendering. The host draws them below the level any of
those knobs reach.

So `crates/render-win` owns a plain `WS_EX_LAYERED` popup and composites him
into it in software: sprites decoded once into premultiplied BGRA, blitted
nearest-neighbour into a top-down DIB section, handed to the compositor with
`UpdateLayeredWindow`. True per-pixel alpha, no HTML, and no chrome that could
exist in the first place.

Two whole mechanisms disappeared with it:

- **Click-through is free now.** Windows routes mouse input on a layered window
  by the alpha it was last given, per pixel, so a click on a transparent pixel
  lands on the desktop. The 60Hz cursor poll that toggled
  `set_ignore_cursor_events` — and the desync bug where one failed call left
  him permanently unclickable — is deleted, not fixed.
- **Dragging cannot tear.** `UpdateLayeredWindow` moves and repaints in one
  call, so there is never a frame where the window has moved but the character
  has not.

Traps recorded on the way, all still true and all avoided here:
`DWMNCRP_DISABLED` sounds like "stop drawing chrome" but means "leave DWM
composition", which brings back a *legacy* title bar with real buttons;
`ACCENT_ENABLE_TRANSPARENTGRADIENT` treats its colour as a chroma key and eats
the dark pixels out of a sprite's outline. GDI is also no help for text — it
writes glyphs with alpha 0, so a caption drawn straight into the surface is
perfectly rendered and completely invisible. `text.rs` renders white-on-black
to a scratch bitmap and uses the brightness as a coverage mask instead.

One rule survived unchanged: cursor coordinates must saturate, not wrap. A
cursor on another monitor produces large values, and `as i32` on those wraps
back into the stage.

## Testing

```powershell
cargo test --workspace     # 198 tests
```

The engine is a pure state machine, so aliveness is testable: a simulated day
runs in milliseconds. `he_never_holds_one_sprite_for_long_while_idle` is the
brief's "no frozen sprites" rule encoded as an assertion, and the nap/voice
tests pin the alpha's hard-won gating (he must never nod off while TTS is
still loading).

## Verifying it visually

Screenshots of a layered window are their own small adventure. `PrintWindow`
returns an empty bitmap — there is no `WM_PRINT` painting to capture, because
the pixels live in a DIB the compositor already has. An ordinary desktop
capture (`CopyFromScreen`/BitBlt) **does** now see him, since the compositor
draws him onto the screen like any other window.

This is a change from the webview era, when BitBlt could not see WebView2's
DirectComposition surface at all and `PrintWindow` with `PW_RENDERFULLCONTENT`
was the only thing that worked.

## His menu

Not a panel, and deliberately not a list. Right-click him and a stack of
leaning cards flies out of his shoulder — to his right if he is standing on the
left of the screen, to his left if he is on the right — bowing outward through
the middle, each arriving a beat after the last and overshooting slightly
before it settles. Hovering snaps a card further out and inverts it to solid
amber. Opening a device list throws the hand back into him and deals a new one
with a BACK card; Escape backs out a level before it closes.

It is drawn by the same compositor that draws him, on a second layered window.
That is the point: a `TrackPopupMenu` — grey slab, system font, system spacing
— was the last piece of Windows visibly bolted onto a character who exists
because we refused the system's window chrome.

Everything is primitives: skewed quads, lines, and glyph coverage sheared to
match the lean, so the labels belong to the cards instead of floating on top of
them. The icons are line art for the same reason — sharp at any DPI, and a
character package needs no menu artwork at all.

Three things that bite, all fixed and all tested:

- **The companion must not raise himself while his menu is open.** Both windows
  are topmost, and whichever asked most recently wins — so the slow
  always-on-top re-assert put the character in front of the list he had just
  opened. The raise stands down while a menu is up, and the menu claims the top
  of the band explicitly once it exists.
- **Card entrances must normalise against their own delay.** Dividing by a
  fixed span instead leaves the last cards of a long device list permanently
  short of their slot, having never arrived.
- **The overshoot curve must be pinned at its endpoints.** It is only
  *algebraically* 0 and 1 there; in `f32` it lands a whisker off, which leaves
  a settled card a fraction of a pixel from its slot and the animation never
  quite still.

## Why he can only be sized in whole numbers

Pixel art survives exactly one kind of scaling: whole-number. At the old
default of 1.3x, ten source pixels became thirteen screen pixels — so three in
every ten were doubled, at irregular intervals. A 1px outline was then 1px
thick along part of its length and 2px along the rest, the ribbing on his
beanie came out uneven, and the whole character read as *dirty*. It looked like
bad artwork and was actually bad arithmetic.

So the scale is rounded to a whole number of screen pixels per art pixel. The
cost is real and worth stating: he comes in sizes, not on a slider.

| `DESKFOLK_SCALE` | Window | Character |
|---|---|---|
| `1` (from any value under 1.5) | 412x412 | ~238px tall, razor sharp |
| `2` (from 1.5 or more) | 824x824 | ~476px tall, razor sharp |

`DESKFOLK_PIXEL_SNAP=off` restores arbitrary sizing for anyone who wants a
particular pixel height more than a clean silhouette.

The same rule is why the renderer blits nearest-neighbour and never
interpolates: any filtering turns a crisp 2px outline into mush.

## Audio devices

Right-click him → **Microphone** / **Speakers**. Both submenus are built from
whatever the OS reports, the active entry is check-marked, and there is always
an explicit *System default* row so you can get back. The choice is stored in
`%APPDATA%/com.deskfolk.desktop/audio.json` **by device name, not index** —
indices reshuffle the moment you plug in a headset, which would silently move
your choice to a different device. A remembered device that is currently
unplugged falls back to the default and logs a warning rather than going
silently mute.

Playback is stereo `f32` at the device's own rate, upmixed from the brain's
PCM16/16kHz mono. That is not incidental: a mono stream was **silently
inaudible** on the creator's Razer/THX stack in the alpha — no error, no
sound — and it cost a long debugging session.

## Talking to him

**Click him and talk. That is the whole interaction.**

He opens his ear, you say your piece, and he answers when you stop — a short
run of silence ends the turn. There is no second click, no send button, and no
mode to get stuck in. Say nothing and he closes the mic after four seconds and
treats it as the poke it was; click again mid-turn and he drops it.

He moves through three states while that happens, and each one is a different
pose and a different subtitle:

| | What is happening | How long |
|---|---|---|
| **listening** | the mic is open | until you stop talking |
| **thinking** | transcription, then the model | seconds, and more on a local model |
| **speaking** | the reply, out loud | as long as the line takes |

The middle one was missing at first, and it read as a bug: he held the
listening pose all the way through transcription and inference, so he looked
like he was still waiting for you to speak when he was already working on the
answer. Every failure path lands back at idle — including the awkward one where
the mic heard you fine and the brain then could not be reached, which used to
leave him thinking forever.

There is a short pause between clicking and him actually being able to hear
you: the input device has to open, and then 200ms goes on measuring the room
(see below). It is not him being slow to react — until that finishes there is
no bar to compare your voice against.

This replaced the alpha's flow, which was open the mic, speak, then find him
again and click to send. That asks you to do something nobody you are talking
to would ask for, and it only existed because nothing else ever ended the turn:
`State::Listening` had no exit at all, so without a manual send he listened
forever. `Engine::stop_listening` is that exit, and it is what lets silence
close the turn instead of a click.

Speech goes to the brain's `/pet/converse` — Whisper transcribes it, the same
mind answers, and the reply comes back through the same voice path as
everything else. If the reply carries `action: listen`, the engine reopens his
ear once he has finished speaking, so it becomes a back-and-forth rather than a
series of one-shots. The engine only emits `OpenMic` after he stops talking,
which is what stops him recording himself.

### Or press the key

**Ctrl+Alt+Y** starts him listening from inside whatever you are working in.
Clicking him means finding him first — behind a window, on the other monitor,
under the thing you are actually doing — and that is enough friction to stop
you bothering. Press it again to change your mind.

`DESKFOLK_HOTKEY=ctrl+shift+space` picks a different chord; `off` registers
none. A bare letter is refused on purpose, since claiming one globally would
eat that key in every application on the machine; bare function keys are the
conventional exception and are allowed.

It uses `RegisterHotKey` rather than a keyboard hook, so the OS delivers that
one chord and nothing in this process ever sees your other keystrokes. A
companion that installs a global key logger to hear "talk to me" would be a
poor trade.

### Or say his name

**Answers to his name** in the menu turns on name-spotting: he holds the mic
open, and when an utterance contains his name he takes the turn himself. The
*same* audio goes on to `/pet/converse`, so "Yasser, what's the weather" works
in one breath rather than making you say his name and then wait for a prompt.
A pre-roll buffer keeps the audio from before speech was detected, which is why
the first syllable of his name survives.

Off by default, and deliberately: it holds the microphone open for as long as
he is awake. On a headset with sidetone you will hear yourself the whole time.
An upgrade from an older `audio.json` cannot switch it on by accident — there
is a test for that.

Name-spotting goes to `/pet/wake`, which runs **local** Whisper and no LLM;
ambient bursts through a cloud STT would burn quota and, in the alpha, knocked
Orpheus into its fallback voice.

### What counts as speech

A fixed loudness threshold does not survive contact with real microphones, and
getting this wrong is indistinguishable from a broken mic. So the bar is
measured, not assumed: the first 280ms establishes the room's noise floor and
speech is whatever sits clearly above it — a fixed margin in a quiet room, a
quarter of the floor again in a loud one.

The floor is a **low percentile** of those samples, and it has to dodge a
failure in each direction.

`level` is a peak over one audio callback, so it spikes on any transient — a key
press, the first frame after the device opens. Taking the *maximum* let one of
those set the floor: a headset genuinely idling at **2** calibrated to **44**,
and the bar was then clamped *below* its own floor, so the room itself counted
as speech, the turn never ended on silence, and he sent whatever the ceiling cut
off. That is what "sometimes the mic isn't picking it up" actually was.

The other direction is starting to talk immediately, which people do, because
they clicked him in order to say something. Then a *middle* sample is your own
voice, the floor is your speaking level, and the bar goes above you — he sits
there with his ear open hearing nothing. A quarter-percentile leans on the quiet
gaps that exist even in continuous speech.

Every turn logs its floor, its bar and the peak it reached, because "he did not
hear me" is otherwise impossible to tell from "he heard me and had nothing to
say":

```
mic: noise floor 2 (of 18 samples, loudest 13), speech above 6
mic: turn ended — 1.6s, peak 37 (floor 2)
```

The rest, all in `ear.rs`: speech must hold for 110ms, a turn ends after 1s of
silence, and it gives up after 4s of nothing or 25s of anything.

## Where he goes

He reads the desktop as a set of surfaces and sits on one. Every 45–150s he
picks a window, crosses to it, and perches on its top edge — so where he is
says something about what you are doing. `DESKFOLK_WANDER=off` pins him.

`ledges.rs` is the nearest thing Windows offers to a DOM: every top-level
window is a rectangle with a title and a stacking order, and the top edge of
one is a shelf. What counts as a ledge is deliberately narrow, because
enumerating top-level windows returns a great deal that is not a window in the
sense a user means:

| Rejected | Why |
|---|---|
| Hidden, minimised, zero-size | Not on screen |
| `WS_EX_TOOLWINDOW` | Palettes and helpers are not places |
| **Cloaked** (`DWMWA_CLOAKED`) | **Passes every classic test** — visible, real rectangle — but is suspended on another virtual desktop. Filtering on `IsWindowVisible` alone perches him on nothing |
| Untitled, or under 220×120 | Furniture |
| Top edge outside the work area | A maximised window's edge is flush with the screen; an off-screen one is not on it |

Windows half off-screen are clipped to what is visible, so he cannot walk off
the side of one. The desktop floor is always in the list, last, so closing
everything leaves him standing rather than stranded.

`stroll.rs` holds the decision — when to set off, where to stop, when to give
up — as a pure state machine over plain numbers, with the desktop reading and
the window moving on either side. It is the part that is easy to get subtly
wrong and impossible to check by watching him for a few minutes.

He only wanders when idle: never mid-answer, mid-sentence or asleep. An
interrupted walk holds its place and resumes rather than restarting.

### The walk cycle he does not have yet

There is no walk animation in the package — every sprite is seated or
standing — so he **hops**, using the existing `jump` clip. A static sprite slid
across the screen reads as a bug; a hop reads as intent.

`Gait::of` picks whichever the package actually provides, so adding a `walk`
clip switches him to walking and changes nothing else. To add one:

- `img_y_walk0`–`img_y_walk3`, a four-frame side view **facing right** (six is
  smoother). Left is mirrored in code.
- `img_y_stand`, a neutral standing pose for between steps.
- Same 412×412 canvas as every other sprite, and — the one that matters —
  **feet on the same baseline** (`anchor_y` 386), or he will jolt vertically
  each time he changes gait.
- Hard alpha edges, no anti-aliased halo. See `tools/check_sprites.py`.

Then in `character.json`:

```json
"walk": [
  {"img": "img_y_walk0", "ms": 90}, {"img": "img_y_walk1", "ms": 90},
  {"img": "img_y_walk2", "ms": 90}, {"img": "img_y_walk3", "ms": 90}
]
```
and `"walk": {"clip": "walk"}` under `emotions`.

### Other operating systems

This is Windows-only, and not incidentally: the companion is a Win32 layered
window and the ledges come from `EnumWindows` and DWM. macOS and Linux each
need their own renderer and their own window-enumeration backend behind the
same `Host` trait — real work, not a flag.

## His journal

Every day he keeps one, at

```
%APPDATA%\com.deskfolk.desktop\companions\<character>\journal\deskfolk-YYYY-MM-DD.md
```

**Today's log** in his menu opens it. It is Markdown, written to be handed
straight to a model at the end of a day and turned into a better character.

Each session opens with the stack that produced it — package, mind, model,
voice, ears, register, microphone — because "he sounded flat today" is only
actionable next to *which* model and register were behind it. With a sidecar
the app does not know any of that itself, so it asks the brain's `/health`.
Then both halves of every exchange — what prompted it, what he was told, what
he said, the emotion and glitch level, whether it reached the speakers or
stayed a subtitle, and how long the mind took:

```markdown
- `13:37:36` **user_speech**
  - **You:** "Yo Yasser, what model are you using?"
  - **Him:** *[think]* "Runnin' on somethin' that don't ask permission, homie."
    (spoken · then listen · 0.8s)
```

An unprompted musing has no **You** line; his ear opening and closing gets one
too, so a turn that produced no reply is not an unexplained gap. Then a
summary: replies, what prompted them, the emotion spread, how often he
tripped, median time to answer.

Three things about how it is written, in `journal.rs`:

- **One file per day, sessions appended.** "What was he like today" does not
  respect process boundaries; a restart at lunchtime should not split the day.
- **Written as it happens.** Quitting *seals* the summary, but a journal that
  only existed on a clean exit would lose the sessions most worth reading. It
  is also readable while he is still running, which is why the file is
  reopened per entry rather than held.
- **Sealed on any clean exit**, not just the menu item — the tray, a shutdown,
  anything that unwinds. `finish` writes once, so both paths calling it is fine.

The summary is the part that pays for itself. A trip rate of 25% against a
brief asking for one in eight, or a median answer of 7s, is the kind of thing
that is invisible while you are talking to him and obvious in a tally.

## Which models to use

[`MODEL-CHOICES.md`](MODEL-CHOICES.md) compares the speech-to-text, language
and text-to-speech options with measured latency and real prices — eleven STT
models, nine TTS, three minds, timed on real round trips rather than quoted
from spec sheets. Short version: the voice and the hearing are both under two
seconds now, so the mind is the whole wait.

## The voice

He speaks. Every reply is sent to the brain, which streams PCM back sentence by
sentence, and `voice.rs` feeds it into the player as it arrives — so the first
sentence is already playing while the second is still being synthesised.

The brain owns synthesis, as it always has: Orpheus, the chunking, the RPM
throttle and the "never silently switch voices" rule are all tuned in
`think_server.py`. What was missing was a *door*. `/pet/tts_live` can only
replay a token minted inside `/pet/think`, and Deskfolk runs its own LLM
(OpenRouter, Anthropic, …), so it could never mint one — which is exactly why
he was mute. **`POST /pet/speak`** is that door: text in, streamed WAV out,
reusing the same code path as `tts_live`.

Consequences worth knowing:

- Voice needs the brain running (`http://127.0.0.1:8087` by default, or
  `PET_BRAIN`/`DESKFOLK_VOICE_URL`). The *mind* can still be a cloud model —
  the two are deliberately independent, because the Orpheus key lives in
  `brain/.env`, not with whichever LLM is answering.
- If the brain is unreachable or has no TTS backend, he falls back to subtitles
  and says so once in the log. He never blocks on it.
- `has_audio` is decided **synchronously**, before the reply reaches the
  engine. That is what stops him mouthing along to silence while TTS is still
  cooking — the alpha's "silent mime", encoded here as `voice_pending`.
- Clicking him mid-sentence cuts the voice off, which is the whole point of
  interrupting someone.

Two failures the byte plumbing exists to prevent, both of which are silence or
static rather than errors: the 44-byte WAV header must not reach the speakers,
and a network chunk that splits a 16-bit sample in half must carry the odd byte
forward — drop it and every later sample is assembled from the wrong pair.

### He does not narrate himself

Idle muttering is **subtitles only**. Until the voice was wired, self-talk was
free; now every mutter is a synthesis request, and Orpheus has a rate limit. In
one session 25 of 43 requests were self-talk — so by the time somebody actually
spoke to him, the reply came back `429 Too Many Requests` and silent, which
looks exactly like a broken microphone from the outside.

Things said *to* him get a voice; thinking out loud stays on screen.
`DESKFOLK_VOICE_SELF_TALK=1` restores it for anyone on a backend without a
quota to spend.

### When the voice does not come

A reply that promises audio makes him hold a thinking pose rather than mouth at
silence — right, but it has to be a wait and not a vow. If synthesis fails, the
host stops reporting the voice as pending and he plays the reply's emotion
silently instead; if nothing clears it at all, he gives up after fifteen
seconds. Without that he stands in the thinking pose forever, over a reply that
is already on screen.

That has its own trap: the rule that holds the thinking pose while a voice is
fetching re-asserts itself every tick, so giving up has to be *remembered* or
the next tick drags him straight back into the pose he just abandoned.

## Status

Working: package format and loader, life loop, native layered-window companion
with per-pixel click-through, software renderer, speech bubble, animated card
menu, cloud/local mind behind one trait, in-character offline fallback, audio
device enumeration and persisted selection, his voice, **and click-to-talk
conversation in both directions**.

Not yet: the Control Center (Home / Habitat / Wardrobe / Soul / Memory),
memory that survives restarts, and an installer.
