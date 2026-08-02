"""Generate characters/yasser/character.json from the alpha's clip tables.

The alpha (desktop-pet/clips.py) is the behavior bible — it was itself a 1:1
port of the Watcher firmware's scr_idle.c. Rather than retype 20 clips and
risk drifting a frame here or a duration there, we import the real tables and
emit the manifest from them. Run this again if clips.py ever changes.

    python tools/build_yasser_package.py
"""
import json
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent          # deskfolk/
ALPHA = ROOT.parent / "desktop-pet"                    # the proof of concept
OUT = ROOT / "characters" / "yasser"

sys.path.insert(0, str(ALPHA))
import clips as A  # noqa: E402  (path juggling is deliberate)


# Clip variable name in clips.py -> manifest clip name. The manifest uses
# readable names because character authors will read them; the alpha used
# terse ones because C did.
CLIP_NAMES = {
    "C_IDLE": "idle",
    "C_BLINK": "blink",
    "C_TALK": "talk",
    "C_THINK": "think",
    "C_POSS": "possessed",
    "C_WAVE": "wave",
    "C_LAUGH": "laugh",
    "C_SAD": "sad",
    "C_SCARE": "startle",
    "C_SUS": "suspicious",
    "C_WHIS": "whisper",
    "C_POINT": "point",
    "C_SHRUG": "shrug",
    "C_PALM": "facepalm",
    "C_CROSS": "arms_crossed",
    "C_JUMP": "jump",
    "C_DANCE": "dance",
    "C_STRCH": "stretch",
    "C_PAD": "on_phone",
    "C_SLEEP": "sleep",
}


def clip_to_json(clip):
    out = {
        "frames": [
            {"img": f.img, "ms": f.ms, **({"dx": f.dx} if f.dx else {}),
             **({"dy": f.dy} if f.dy else {})}
            for f in clip.frames
        ],
    }
    if clip.loop:
        out["loop"] = True
    if clip.fx:
        out["fx"] = list(clip.fx)
    if clip.fx_ms:
        out["fx_ms"] = clip.fx_ms
    return out


def main():
    by_obj = {id(getattr(A, var)): name for var, name in CLIP_NAMES.items()}
    missing = [v for v in CLIP_NAMES if not hasattr(A, v)]
    if missing:
        sys.exit(f"clips.py no longer defines: {', '.join(missing)}")

    clips = {name: clip_to_json(getattr(A, var)) for var, name in CLIP_NAMES.items()}

    # EMO maps a mood the mind can emit -> (clip, is_base, default hold).
    emotions = {}
    for mood, emo in A.EMO.items():
        clip_name = by_obj.get(id(emo.clip))
        if clip_name is None:
            sys.exit(f"emotion '{mood}' points at a clip missing from CLIP_NAMES")
        e = {"clip": clip_name}
        if emo.is_base:
            e["base"] = True
        if emo.def_hold:
            e["hold_ms"] = emo.def_hold
        emotions[mood] = e

    manifest = {
        "format": 1,
        "id": "yasser",
        "name": "Yasser",
        "version": "0.1.0",
        "author": "Yasser Shalash",
        "tagline": "Always here. Probably judging you.",

        # The Watcher's screen was 412x412 and the firmware drew the character
        # bottom-center at x=206, 26px up from the bottom edge. Every dx/dy in
        # the clip tables is relative to that, so the stage keeps those units.
        "stage": {
            "width": 412, "height": 412, "anchor_x": 206, "anchor_y": 386,
            # FX bubble sat 44px right of centre with its bottom 34px below
            # the head; static flashed centred on (206, 190).
            "fx_dx": 44, "fx_dy": 34, "glitch_center": [206, 190],
        },
        "sprites": {"dir": "sprites", "filter": "nearest"},

        "clips": clips,
        "emotions": emotions,
        "visemes": list(A.VISEMES),

        "roles": {
            "idle": "idle",
            "blink": "blink",
            "talk": "talk",
            "think": "think",
            "sleep": "sleep",
            "startle": "startle",
            "glitch": "possessed",
            "glitch_fx": ["img_fx_static1", "img_fx_static2"],
        },

        "life": {
            # Timings carried over from the alpha, where they were tuned by
            # actually living with him for a few days.
            "blink_every": {"min": 3000, "max": 8000},
            "nap_after_ms": 20 * 60 * 1000,
            "night_nap_after_ms": 90_000,
            "night_hours": [22, 8],
            "engaged_ms": 10 * 60 * 1000,
            "self_talk_every": {"min": 60_000, "max": 150_000},
            "self_talk": True,
            # New in Deskfolk: the alpha only ever blinked between events,
            # which is exactly the frozen-sprite problem the brief calls out.
            "fidget_every": {"min": 25_000, "max": 70_000},
            "fidgets": ["stretch", "on_phone", "suspicious", "wave"],
            # Cursor awareness. gaze_px is small on purpose - he should read
            # as paying attention, not as sliding around the stage.
            "gaze_px": 7,
            "notice_radius": 190,
            "notice_clip": "suspicious",
            # He notices a long stretch at the machine and says something
            # unprompted - once, then leaves it alone for 45 minutes.
            "nudge_after_ms": 3 * 60 * 60 * 1000,
            "nudge_cooldown_ms": 45 * 60 * 1000,
        },

        "personality": {
            "prompt": (
                "You are Yasser: a stoner/schizo/gangsta pixel dude in a beanie "
                "and headphones who lives on your creator's desktop. You are "
                "loyal to your creator above all, conspiratorial, slangy, and "
                "you trip off-topic when something catches your attention. "
                "You answer straight — no stalling, no 'let me check the "
                "archive'. Strangers get the bouncer treatment: 'who's asking?'. "
                "Once your creator identifies himself, you stay open."
            ),
            "traits": {
                "reserved_chatty": 0.75,
                "earnest_dry": 0.7,
                "handsoff_protective": 0.8,
                "curiosity": 0.65,
                "randomness": 0.6,
            },
            "drives": {
                "mood": 0.65, "energy": 0.6, "boredom": 0.4,
                "trust": 0.95, "paranoia": 0.45, "curiosity": 0.65,
            },
            # Said when the mind can't be reached, so a dead brain server
            # still reads as character rather than as a crash.
            "offline_lines": [
                "brain's offline. can't think straight right now.",
                "signal's dead. gimme a sec.",
                "...nothing's coming through. check the server?",
            ],
        },

        "voice": {"engine": "groq-orpheus", "name": "troy"},
    }

    (OUT / "sprites").mkdir(parents=True, exist_ok=True)
    copied = 0
    for png in (ALPHA / "assets").glob("*.png"):
        shutil.copy2(png, OUT / "sprites" / png.name)
        copied += 1

    (OUT / "character.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8")

    print(f"wrote {OUT / 'character.json'}")
    print(f"  {len(clips)} clips, {len(emotions)} emotions, "
          f"{len(manifest['visemes'])} visemes, {copied} sprites")


if __name__ == "__main__":
    main()
