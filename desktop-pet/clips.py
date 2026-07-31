"""Clip + emotion tables, ported 1:1 from the Watcher firmware
(firmware/examples/lumen-watcher/main/screens/scr_idle.c).

A frame is (sprite_name, ms, dx, dy). dx/dy nudge the character around its
bottom-center anchor. A clip is a named tuple of frames plus loop flag and
an optional FX overlay list (drawn above-right of the character); fx_ms > 0
cycles through the FX images.
"""
from typing import NamedTuple, Optional, Tuple


class Frame(NamedTuple):
    img: str
    ms: int
    dx: int = 0
    dy: int = 0


class Clip(NamedTuple):
    name: str
    frames: Tuple[Frame, ...]
    loop: bool
    fx: Tuple[str, ...] = ()
    fx_ms: int = 0


F = Frame

C_IDLE = Clip("idle", (F("img_y_sitmug", 900), F("img_y_sitb", 700),
                       F("img_y_sitc", 520), F("img_y_sitb", 700)), True)
C_BLINK = Clip("blink", (F("img_y_blinkh", 60), F("img_y_blink", 110),
                         F("img_y_blinkh", 60)), False)
C_TALK = Clip("talk", (F("img_y_talk0", 100), F("img_y_talk1", 90),
                       F("img_y_talk2", 100), F("img_y_talk3", 120),
                       F("img_y_talk2", 90), F("img_y_talk1", 90)), True)
C_THINK = Clip("think", (F("img_y_expr3", 800), F("img_y_expr3", 800, 0, 1)),
               True, ("img_fx_think",))
C_POSS = Clip("poss", (F("img_y_pos0", 140, 2), F("img_y_pos1", 140, -2),
                       F("img_y_pos2", 140, 1), F("img_y_pos3", 140, -1),
                       F("img_y_pos4", 140, 2)), True)
C_WAVE = Clip("wave", (F("img_y_act0", 150), F("img_y_act1", 140),
                       F("img_y_act0", 150), F("img_y_act1", 140),
                       F("img_y_act0", 150), F("img_y_act1", 150)), False)
C_LAUGH = Clip("laugh", (F("img_y_expr2", 1700),), False, ("img_fx_heart",))
C_SAD = Clip("sad", (F("img_y_expr0", 1300), F("img_y_expr0", 1300, 0, 1)),
             True, ("img_fx_drop",))
C_SCARE = Clip("scare", (F("img_y_expr1", 1500),), False, ("img_fx_bangq",))
C_SUS = Clip("sus", (F("img_y_expr4", 1800),), False)
C_WHIS = Clip("whis", (F("img_y_speak2", 900), F("img_y_speak2", 900, 0, 1)),
              True)
C_POINT = Clip("point", (F("img_y_speak0", 1500),), False)
C_SHRUG = Clip("shrug", (F("img_y_act2", 1400),), False)
C_PALM = Clip("palm", (F("img_y_act3", 1700),), False)
C_CROSS = Clip("cross", (F("img_y_act4", 1200), F("img_y_act4", 1200, 1)),
               True, ("img_fx_anger",))
C_JUMP = Clip("jump", (F("img_y_big0", 200, 0, -6), F("img_y_big1", 190, 0, 30),
                       F("img_y_big2", 220, 0, 2), F("img_y_big0", 140, 0, -2)),
              False)
C_DANCE = Clip("dance", (F("img_y_big3", 240, -3), F("img_y_big4", 240, 3)),
               True, ("img_fx_note1", "img_fx_note2"), 380)
C_STRCH = Clip("stretch", (F("img_y_stretch", 2200),), False)
C_PAD = Clip("pad", (F("img_y_sitpad", 6000),), True)
C_SLEEP = Clip("sleep", (F("img_y_zsleep", 1400), F("img_y_zsleep", 1400, 0, 1)),
               True)


class Emo(NamedTuple):
    clip: Clip
    is_base: bool
    def_hold: int   # ms the base emotion holds before reverting to idle


# emotion name (what the brain returns) -> clip behavior
EMO = {
    "idle":       Emo(C_IDLE,  True,  0),
    "happy":      Emo(C_WAVE,  False, 0),
    "talk":       Emo(C_TALK,  True,  2600),
    "think":      Emo(C_THINK, True,  4000),
    "glitch":     Emo(C_POSS,  True,  2200),
    "dance":      Emo(C_DANCE, True,  5200),
    "celebrate":  Emo(C_DANCE, True,  5200),
    "jump":       Emo(C_JUMP,  False, 0),
    "confused":   Emo(C_SHRUG, False, 0),
    "sad":        Emo(C_SAD,   True,  3500),
    "scared":     Emo(C_SCARE, False, 0),
    "laugh":      Emo(C_LAUGH, False, 0),
    "suspicious": Emo(C_SUS,   False, 0),
    "whisper":    Emo(C_WHIS,  True,  3000),
    "point":      Emo(C_POINT, False, 0),
    "facepalm":   Emo(C_PALM,  False, 0),
    "grumpy":     Emo(C_CROSS, True,  3000),
    "stretch":    Emo(C_STRCH, False, 0),
    "busy":       Emo(C_PAD,   True,  6000),
}

# viseme sprites indexed by mouth openness (voice level buckets 8/30/60)
VISEMES = ("img_y_talk0", "img_y_talk1", "img_y_talk2", "img_y_talk3")
