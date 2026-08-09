"""Standalone checks for the brain's pure helpers — `python test_think_server.py`.

Deliberately dependency-free: the project pins no test runner, and these
cover logic that is easy to get subtly wrong and painful to notice live.
"""
import os, sys

os.environ.setdefault("PET_KEEP_IT_REAL", "1")
os.environ.setdefault("PET_SPICE", "1")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import think_server as ts

fails = []


def check(name, got, want):
    if got != want:
        fails.append(f"{name}\n     got  {got!r}\n     want {want!r}")


# --- _clip -----------------------------------------------------------------
# The bug this replaced sliced mid-word, so an answer that ran long read as
# the character losing his nerve rather than as a trimmed answer.
check("short text untouched", ts._clip("hey there", 50), "hey there")
check("exact length untouched", ts._clip("abcde", 5), "abcde")

long = "Splice em right and bam, vroom vroom just like that. But be careful"
check("cuts on the sentence end", ts._clip(long, 60),
      "Splice em right and bam, vroom vroom just like that.")

# No sentence ends late enough to use, so fall back to a whole word + ellipsis.
check("word boundary fallback",
      ts._clip("supercalifragilistic expialidocious extravaganza", 30),
      "supercalifragilistic...")
check("never splits a word",
      " " not in ts._clip("aaaa bbbb cccc dddd", 12)[-1:] and
      ts._clip("aaaa bbbb cccc dddd", 12).endswith("..."), True)
check("strips dangling punctuation",
      ts._clip("yeah man, whatever you say boss", 14), "yeah man...")

# An early sentence end must NOT win — cutting at 10% of the budget throws
# away most of a perfectly good answer.
early = "Yo. " + "x" * 200
assert ts._clip(early, 100) != "Yo.", "early sentence end should be ignored"
check("ignores a too-early sentence end", ts._clip(early, 100).startswith("Yo. x"), True)

# --- _sanitize_tts_text ----------------------------------------------------
check("curly quotes flattened", ts._sanitize_tts_text("‘hi’"), "'hi'")
check("collapses whitespace", ts._sanitize_tts_text("a   b\n c"), "a b c")
check("limit clips cleanly, not mid-word",
      ts._sanitize_tts_text("hello there friend", limit=14), "hello there...")

# --- the 18+ register ------------------------------------------------------
# These two are the knobs that actually decide whether he sounds censored;
# a refactor that drops one is the exact regression worth catching.
check("keep-it-real is on", ts.PET_KEEP_IT_REAL, True)
check("spice register present", "ADULT, 18+" in ts.SYSTEM, True)
check("euphemism ban present", "No euphemism" in ts.SYSTEM, True)
check("ai self-reference banned", "Never call yourself an AI" in ts.SYSTEM, True)
check("hate limit survives", "no slurs" in ts.SYSTEM, True)

# --- music intent -----------------------------------------------------------
# Matched deterministically rather than left to the model, which agreed in
# words and omitted the field on most attempts.
for said, want in [
    ("yo skip this song", "next"),
    ("skip it", "next"),
    ("next track", "next"),
    ("go back a track", "previous"),
    ("turn it up a bit", "louder"),
    ("louder man", "louder"),
    ("turn it down", "quieter"),
    ("mute it", "mute"),
    ("pause the music for a sec", "pause"),
    ("stop it", "pause"),
    ("keep playing", "resume"),
]:
    got = ts.music_intent(said)
    check(f"intent {said!r}", got and got["do"], want)

got = ts.music_intent("put on some madvillain")
check("play carries the query", got, {"do": "play", "query": "some madvillain"})
check("bare play is the button, not a search",
      ts.music_intent("play"), {"do": "resume", "query": ""})

# Talking *about* music is not an instruction to touch it. Getting this wrong
# means he skips your track because you mentioned a song.
for said in [
    "what music do you like",
    "do you play games",
    "why is this song so good",
    "remember when we played that",
    "how you doing man",
    "",
]:
    check(f"not a command: {said!r}", ts.music_intent(said), None)

# --- body intent ------------------------------------------------------------
# The counterpart to music_intent: a told-to movement, matched deterministically
# so "take a walk" moves him even when the model forgets to fill the field.
for said, want in [
    ("walk", "walk"),
    ("Walk!", "walk"),
    ("yo walk", "walk"),
    ("move", "walk"),
    ("get moving", "walk"),
    ("yo take a walk", "walk"),
    ("go for a stroll man", "walk"),
    ("walk around a bit", "walk"),
    ("stretch your legs", "walk"),
    ("walk to the left", "walk_left"),
    ("go right", "walk_right"),
    ("walk right", "walk_right"),
    ("go to sleep", "sleep"),
    ("take a nap bro", "sleep"),
]:
    check(f"body: {said!r}", ts.body_intent(said), want)

# Talking *about* moving, or not about it at all, must not set him off.
for said in ["do you ever take walks", "what do you think about sleep",
             "tell me about your day", "play some music", ""]:
    check(f"not a body command: {said!r}", ts.body_intent(said), None)

# --- extract_json -----------------------------------------------------------
# Throwing away a real answer is worse than a refusal: it reads as censorship
# that was never there. Two of five research questions failed exactly so.
check("clean object", ts.extract_json(chr(123) + '"say": "hello"' + chr(125))["say"], "hello")
check("object with chatter around it",
      ts.extract_json("Sure! " + chr(123) + '"say": "hi"' + chr(125) + " hope that helps")["say"], "hi")
check("truncated mid-object still yields what it had",
      ts.extract_json(chr(123) + '"say": "it goes like this and then'),
      {"say": "it goes like this and then"})
check("escaped quotes survive the repair",
      ts.extract_json(chr(123) + r'"say": "he said \"go\" and left'),
      {"say": 'he said "go" and left'})
check("plain prose is an answer, not a failure",
      ts.extract_json("no json here, just the actual answer")["say"],
      "no json here, just the actual answer")
check("empty is still nothing", ts.extract_json("   "), None)

if fails:
    print(f"FAILED {len(fails)}:")
    for f in fails:
        print("  - " + f)
    sys.exit(1)
print("all brain helper checks passed")
