"""How open the user is being, and what that does to the relationship.

This is the psychology, kept on its own. It knows nothing about HTTP, the
memory file, or the model — the brain owns persistence and the prompt, the app
owns the mic, and this owns one thing: reading a message on a single axis,
open vs guarded, and turning a run of those readings into a relationship that
drifts.

The design intent, in one line: **he mirrors you.** Open up and he warms up
and opens up back; stay scarce, hidden, cagey — "who's asking", one-word
answers, changing the subject — and he closes down and gets wary, because a
person who gives you nothing is a person you learn not to trust. Two knobs
make that feel real rather than mechanical:

- **It is asymmetric.** Guardedness costs more than openness earns. Trust is
  slow to build and quick to lose, which is how it works with people, and it
  is the whole reason being cagey is a problem rather than a neutral style.

- **It has a slow memory, not a grudge.** Every turn pulls gently back toward
  the middle, so the relationship reflects how you have been *lately*, not one
  cold night three weeks ago. Warmth fades if unfed; wariness thaws if you
  come back open.

None of it is a gate. He will still answer his creator anything — the register
and the topic locks are elsewhere. This only changes *how he is with you*:
warm and unguarded, or short and testing, and everything between.
"""
import re

# --- reading one message -----------------------------------------------------

# Opening up: sharing something real, asking about him, warmth. Matched as
# whole words, case-folded, so "feel" does not fire on "feeling lucky" — these
# lean toward disclosure, not vocabulary.
_OPEN = [
    r"i feel", r"i felt", r"i think", r"i believe", r"honestly", r"to be honest",
    r"tbh", r"i'?ve been", r"i am", r"i'?m (?:scared|sad|happy|tired|lonely|"
    r"anxious|stressed|worried|excited|proud|nervous|lost|struggling)",
    r"my (?:mom|dad|family|girl|boy|friend|job|life|dream|fear|problem|"
    r"brother|sister|ex|wife|husband|kid)", r"reminds me", r"i remember",
    r"between us", r"i trust", r"i wanna tell you", r"can i tell you",
    r"the truth is", r"deep down", r"i never told",
    # reciprocity — asking about him is opening a door
    r"how (?:are|you|'?re) you", r"what (?:do you|about you|you think)",
    r"you (?:good|okay|alright)", r"how you (?:doing|feelin)", r"whats good with you",
    # warmth
    r"thank you", r"thanks", r"appreciate", r"love you", r"love this", r"proud of you",
    r"missed you", r"means a lot", r"you'?re (?:the best|dope|cool|real|family)",
]

# Closing off: secrecy, deflection, coldness, hostility. The "mafia" register.
_GUARD = [
    r"none of your (?:business|concern)", r"who'?s asking", r"why do you (?:care|ask)",
    r"why you askin", r"not (?:telling|tellin|your business|saying)", r"need to know",
    r"figure it out", r"that'?s classified", r"can'?t say", r"won'?t say",
    r"mind your", r"stay out of", r"drop it", r"leave it", r"forget it",
    r"not important", r"doesn'?t matter", r"nvm", r"never ?mind",
    r"stop asking", r"quit asking", r"enough questions", r"too many questions",
    r"whatever", r"idc", r"don'?t care", r"so what", r"and\?", r"your point",
    # coldness / hostility
    r"shut up", r"fuck off", r"go away", r"leave me alone", r"annoying",
    r"stop talking", r"boring", r"who cares", r"lame",
]

_OPEN_RE = [re.compile(p, re.I) for p in _OPEN]
_GUARD_RE = [re.compile(p, re.I) for p in _GUARD]

# A bare command — "skip this", "what's the weather" — is neither opening up nor
# shutting down. It should barely move the relationship, so it is recognised and
# discounted rather than read as terse guardedness.
_COMMANDISH = re.compile(
    r"^\s*(?:yo\s+|hey\s+|please\s+)?(?:skip|next|pause|play|stop|resume|louder|"
    r"quieter|mute|volume|turn it|put on|go back|open|close|search|google|"
    r"what time|what'?s the (?:time|date|weather))\b", re.I)


def score_openness(text):
    """Where one message sits on open(+1) .. guarded(-1), and why.

    Continuous, because "eh, i guess i've been alright" is opening a crack and
    "none of your business" is slamming a door, and those are not the same
    size. Returns (score, reasons) — the reasons are for the log, so a drift
    that surprises the user can be explained rather than argued with.
    """
    t = (text or "").strip()
    if not t:
        return 0.0, ["nothing said"]

    reasons = []
    score = 0.0
    low = t.lower()

    opens = sum(bool(rx.search(low)) for rx in _OPEN_RE)
    guards = sum(bool(rx.search(low)) for rx in _GUARD_RE)
    if opens:
        score += 0.28 * opens
        reasons.append(f"+{opens} open")
    if guards:
        score -= 0.42 * guards          # a slammed door is louder than an open one
        reasons.append(f"-{guards} guarded")

    words = re.findall(r"\w+", low)
    n = len(words)
    if _COMMANDISH.match(t):
        reasons.append("a command, not a confidence")
        # A command is close to neutral whatever else it trips.
        score *= 0.3
    elif n >= 18:
        score += 0.2                    # telling a story is itself opening up
        reasons.append("shared at length")
    elif n <= 2 and guards == 0 and opens == 0:
        score -= 0.15                   # one cold word, giving nothing
        reasons.append("terse")

    if "?" in t and guards == 0:
        score += 0.05                    # curiosity about him is a small opening
        reasons.append("asked something")

    return max(-1.0, min(1.0, score)), reasons


# --- the relationship it drives ----------------------------------------------

# The neutral each trait drifts back toward. A relationship with no recent
# input is an acquaintance, not a friend and not an enemy.
BASELINE = 50
# How hard every turn pulls back toward the baseline. Small: a slow forgetting,
# so old warmth cools and old wariness thaws over many exchanges, not one.
_DECAY = 0.06

# Per-trait response to one turn's openness, as (gain when open, loss when
# guarded). The loss is larger everywhere — that asymmetry is the point — and
# trust moves least, because trust is the slowest of these to earn.
_RESPONSE = {
    "rapport": (7.0, 11.0),
    "trust":   (4.0, 9.0),
    "warmth":  (8.0, 10.0),
}


def fresh():
    """A relationship that has not started yet."""
    return {"rapport": BASELINE, "trust": BASELINE, "warmth": BASELINE,
            "guard": BASELINE, "exchanges": 0}


def _clamp(v):
    return max(0, min(100, int(round(v))))


def drift(rel, openness):
    """Move the relationship one turn's worth, given how open that turn was.

    Pure: takes a state and a score, returns a new state. The brain persists
    whatever comes back.
    """
    rel = dict(fresh(), **(rel or {}))
    o = max(-1.0, min(1.0, float(openness)))

    for trait, (gain, loss) in _RESPONSE.items():
        delta = gain * o if o >= 0 else loss * o     # o<0 makes this a subtraction
        pull = (BASELINE - rel[trait]) * _DECAY       # toward the middle
        rel[trait] = _clamp(rel[trait] + delta + pull)

    # Guard is his wariness — it moves opposite to warmth: their guardedness
    # raises it, their openness lowers it.
    guard_delta = -9.0 * o if o >= 0 else -11.0 * o   # o<0 raises guard
    rel["guard"] = _clamp(rel["guard"] + guard_delta + (BASELINE - rel["guard"]) * _DECAY)

    rel["exchanges"] = int(rel.get("exchanges", 0)) + 1
    return rel


def stance(rel):
    """One word for where the relationship stands, for logs and the prompt."""
    r = (rel or {}).get("rapport", BASELINE)
    if r >= 72:
        return "close"
    if r >= 58:
        return "warming"
    if r > 42:
        return "neutral"
    if r > 28:
        return "guarded"
    return "cold"


def describe(rel):
    """The line the model reads: where you two stand, and how to be about it.

    The behaviour, not the numbers. A model given "trust=34" guesses what to do
    with it; a model told "he is being cagey, so keep your guard up and make him
    earn it" does the thing.
    """
    rel = dict(fresh(), **(rel or {}))
    s = stance(rel)
    if rel["exchanges"] < 3:
        return ("RELATIONSHIP: you are still feeling this person out. Be warm "
                "but a little reserved until you see how they carry themselves.")
    lines = {
        "close": "RELATIONSHIP: this person has opened up to you and you trust "
                 "them. Be warm, loose, unguarded — share freely, joke, let them "
                 "in. This is someone you're close with.",
        "warming": "RELATIONSHIP: things are warming up. They've been fairly open, "
                   "so meet them there — relaxed and friendly, a little more of "
                   "yourself each time.",
        "neutral": "RELATIONSHIP: ordinary footing. Friendly, easy, neither "
                   "clingy nor cold.",
        "guarded": "RELATIONSHIP: they've been cagey with you lately — short "
                   "answers, dodging, keeping you at arm's length. So keep your "
                   "own guard up. Be a little cooler and shorter, don't pour "
                   "yourself out, and make them earn the warmth back. Not hostile "
                   "— just watchful, the way you'd be with someone who gives you "
                   "nothing.",
        "cold": "RELATIONSHIP: this person stays hidden and gives you nothing — "
                "secretive, deflecting, mafia about everything. You've stopped "
                "reaching. Be terse and wary, answer plainly without warmth, and "
                "let them feel the distance they built. You open back up only if "
                "they do.",
    }
    return lines[s]


def as_stats(rel):
    """The relationship expressed as the persona stats the prompt already uses.

    So the existing `stats: mood=.. paranoia=.. trust=..` line moves with the
    relationship without a second mechanism: guardedness reads as paranoia and
    low trust, closeness as high trust and mood.
    """
    rel = dict(fresh(), **(rel or {}))
    return {
        "trust": rel["trust"],
        "paranoia": _clamp(rel["guard"]),
        # Mood leans on warmth; it is not the whole of his mood, so keep it
        # near the middle and let warmth tug it.
        "mood": _clamp(BASELINE + (rel["warmth"] - BASELINE) * 0.6),
    }
