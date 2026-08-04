"""Checks for the relationship engine — `python test_rapport.py`.

The psychology is the kind of thing that looks right in the code and is wrong
on the third conversation, so the behaviours are pinned here rather than
eyeballed live.
"""
import rapport as r

fails = []


def check(name, got, want):
    if got != want:
        fails.append(f"{name}\n     got  {got!r}\n     want {want!r}")


def ok(name, cond):
    if not cond:
        fails.append(name)


# --- reading a single message ------------------------------------------------

ok("opening up scores positive", r.score_openness("honestly i've been really stressed about my job")[0] > 0.2)
ok("a slammed door scores negative", r.score_openness("none of your business")[0] < -0.3)
ok("asking about him is an opening", r.score_openness("how you doing man")[0] > 0)
ok("a cold one-word answer dips", r.score_openness("whatever")[0] < 0)
ok("a command barely moves it",
   abs(r.score_openness("yo skip this song")[0]) < 0.15)
ok("nothing said is neutral", r.score_openness("")[0] == 0.0)

# The asymmetry: a door slams harder than a door opens.
open_amt = r.score_openness("honestly i trust you, my family's been rough lately")[0]
guard_amt = r.score_openness("who's asking, none of your business")[0]
ok("guardedness outweighs openness", abs(guard_amt) > abs(open_amt) * 0.9)


# --- how it drifts over a run of turns ---------------------------------------

def run(lines):
    rel = r.fresh()
    for line in lines:
        rel = r.drift(rel, r.score_openness(line)[0])
    return rel


opener = ["honestly i've been struggling lately",
          "my dad and i haven't spoken in years, it eats at me",
          "how you holding up though? i appreciate you man",
          "i trust you with this, deep down i'm scared i'll end up like him"]
closer = ["who's asking", "none of your business", "why do you care",
          "figure it out", "stop asking", "whatever man"]

warm = run(opener)
cold = run(closer)

ok("opening up builds rapport", warm["rapport"] > 60)
ok("opening up builds trust", warm["trust"] > 55)
ok("staying hidden tanks rapport", cold["rapport"] < 38)
ok("staying hidden raises his guard", cold["guard"] > 60)
ok("the warm arc reads as close or warming", r.stance(warm) in ("close", "warming"))
ok("the cold arc reads as guarded or cold", r.stance(cold) in ("guarded", "cold"))

# Everything stays on the dial.
for rel in (warm, cold):
    for k in ("rapport", "trust", "warmth", "guard"):
        ok(f"{k} stays 0..100 ({rel[k]})", 0 <= rel[k] <= 100)


def test_asymmetry_over_a_run():
    # Six open turns then six cold ones should not net out neutral — the cold
    # run bites deeper, so he ends up warier than he started.
    rel = r.fresh()
    for line in opener * 2:
        rel = r.drift(rel, r.score_openness(line)[0])
    peak = rel["rapport"]
    for line in closer * 2:
        rel = r.drift(rel, r.score_openness(line)[0])
    ok("trust lost faster than it was earned", rel["rapport"] < peak - 25)


test_asymmetry_over_a_run()


def test_it_forgets_slowly_not_never():
    # A cold spell should thaw if they come back open, rather than holding a
    # permanent grudge — the relationship tracks recent behaviour.
    rel = run(closer)
    frozen = rel["rapport"]
    for line in opener * 3:
        rel = r.drift(rel, r.score_openness(line)[0])
    ok("wariness thaws when they reopen", rel["rapport"] > frozen + 15)


test_it_forgets_slowly_not_never()


def test_neutral_relationship_decays_toward_the_middle():
    # Left alone with neutral chatter, an extreme relationship should ease back,
    # not stick at the edge forever.
    rel = dict(r.fresh(), rapport=95, trust=95, warmth=95, guard=5)
    for _ in range(30):
        rel = r.drift(rel, 0.0)
    ok("high rapport eases toward the middle without input", rel["rapport"] < 80)
    ok("but does not crash to zero", rel["rapport"] > 45)


test_neutral_relationship_decays_toward_the_middle()


# --- what the model is told --------------------------------------------------

ok("a new relationship says so", "feeling this person out" in r.describe(r.fresh()))
ok("a close relationship says be open", "unguarded" in r.describe(warm).lower()
   or "warm" in r.describe(warm).lower())
ok("a cold relationship says be wary",
   "wary" in r.describe(cold).lower() or "terse" in r.describe(cold).lower()
   or "guard" in r.describe(cold).lower())

# The stat projection the existing prompt line consumes.
cold_stats = r.as_stats(cold)
warm_stats = r.as_stats(warm)
ok("guardedness shows up as paranoia", cold_stats["paranoia"] > warm_stats["paranoia"])
ok("closeness shows up as trust", warm_stats["trust"] > cold_stats["trust"])
for s in (cold_stats, warm_stats):
    for k, v in s.items():
        ok(f"stat {k} in range ({v})", 0 <= v <= 100)


if fails:
    print(f"FAILED {len(fails)}:")
    for f in fails:
        print("  - " + f)
    raise SystemExit(1)
print("all rapport checks passed")
