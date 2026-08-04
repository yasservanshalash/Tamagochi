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

if fails:
    print(f"FAILED {len(fails)}:")
    for f in fails:
        print("  - " + f)
    sys.exit(1)
print("all brain helper checks passed")
