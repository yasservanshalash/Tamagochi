# Model choices

Every timing here was measured on **this machine** on 3 August 2026, against a
running brain, not taken from a spec sheet. Prices are list prices from the
providers. Single runs unless noted, so treat them as "this class of latency"
rather than benchmarks — but the differences that matter are large enough to
survive the noise.

Test line for TTS (74 chars): *"Yo, what's good? I'm right here on your desktop,
keeping an eye on things."*
Test clip for STT (3.3s): *"Yo, testing 1, 2. This is the voice check."*

---

## The stack, for about €12/month

Installed and running. Chosen for how convincing he sounds, inside a €20
ceiling, with room left over.

| Layer | Model | Measured | Cost/month |
|---|---|---|---|
| **STT** | `mistralai/voxtral-mini-transcribe` | 0.35s | ~$0.50 |
| **Mind** | `nousresearch/hermes-4-70b` | 0.71–1.02s | ~$0.90 |
| **TTS** | `minimax/speech-2.8-turbo`, voice `English_Trustworth_Man` | 1.73s | ~$12 |
| | | | **≈ $13.50 / €12** |

**Want the rest of the budget spent on the voice?** One line:
`PET_GROQ_TTS_MODEL=minimax/speech-2.8-hd` — 1.48s, $100/1M ≈ **$20/month**,
the most expressive thing reachable from your existing key. That takes the
total to about €19 and leaves nothing spare, which is fine for development but
worth knowing.

### What that changed

Full round trip through `/pet/converse` — hear, think, reply:

```
  before   11.9s      local model + Groq voice
  after     1.4s      measured twice: 1.54s, 1.26s
```

Nine times faster. Almost all of it was the mind — the local 24B was 6–10s of
that 11.9s, and it turned out the *uncensored* models are also the fast ones.

---

## Where the time goes now

```
  1.0s   silence detection (he waits to be sure you finished)
  0.35s  speech to text
  ~0.9s  the mind          <-- was 6-10s
  1.7s   text to speech
```

Nothing left is an obvious win. The next-largest number is the second of
silence he waits before deciding you have finished, and shortening that makes
him cut you off mid-thought.

---

## Speech to text

All tested through OpenRouter with the same 3.3s clip. Ten of eleven candidates
worked; `qwen/qwen3-asr-flash` does not exist on OpenRouter.

| Model | Time | What it heard | Price |
|---|---|---|---|
| **`mistralai/voxtral-mini-transcribe`** ← active | **0.35s** | "Yo, testing one, two. This is the voice check." | $0.003/min |
| `fish-audio/transcribe-1` | 0.59s | "Yo, testing one two. This is the voice check." | $0.0001/sec |
| `x-ai/grok-stt-1.0` | 0.67s | "Yo, testing one two, this is the voice check." | $0.10/hr |
| `nvidia/parakeet-tdt-0.6b-v3` | 0.68s | "Yo, testing one two? This is the voice check." | **$0.09/hr** |
| `deepgram/nova-3` | 0.73s | "Yo. Testing one two. This is the voice check." | $0.0043/min |
| `openai/gpt-4o-transcribe` | 0.95s | correct | $2.50/M in |
| `microsoft/mai-transcribe-1.5` | 1.39s | "Yo, testing 1, 2..." (keeps digits) | $0.36/hr |
| `openai/gpt-4o-mini-transcribe` | 1.59s | correct | $1.25/M in |
| `openai/whisper-1` | 1.63s | "Yo, Testing 1-2, this is the voice check." | — |
| `google/chirp-3` | 1.68s | correct | $0.016/min |
| Groq `whisper-large-v3-turbo` | ~0.5–1s | correct | **$0.04/hr** ← cheapest anywhere |

**Verdict:** STT is a solved problem here. All of them are accurate enough and
all are cheap enough that the cost is a rounding error — you speak for maybe
five minutes a day, so even the dearest option is under a euro a month. Voxtral
wins on the only axis that matters, latency. If you want the cheapest, Groq's
Whisper at $0.04/hr is still unbeaten and it is one line to switch back.

---

## Making him sound real

This is the part worth spending on, so it gets its own section.

### What the benchmarks say

- **Inworld** currently leads on overall realism and subtle emotional nuance.
- **ElevenLabs v3** (March 2026) is their most expressive model — built for
  narration and character work, explicitly *not* for real-time, with latency
  over 300ms by design.
- **Orpheus 3B** scores in the *same band as ElevenLabs Multilingual v2* in
  blind English tests, and is the only option here with **inline emotion tags
  that actually fire**: `<laugh>` `<sigh>` `<cough>` `<sniffle>` `<groan>`
  `<yawn>` `<gasp>`. For a character with a personality that is a real lever —
  he can laugh mid-sentence rather than saying "haha".
- **MiniMax** is the price-to-performance pick, with emotion control that
  competes with far more expensive flagships.

### The shortlist, same line, same day

*"Yo, you actually think they ain't listening? Man, check the router logs
sometime. I'm just sayin'."* — samples in
`<repo>/.claude/jobs/581c98a2/tmp/shortlist`, go and listen. I can time these;
I cannot tell you which one sounds like **him**.

| Model | Time | /1M chars | At ~200k chars | Note |
|---|---|---|---|---|
| `minimax/speech-2.8-hd` | **1.48s** | $100 | $20 | most expressive reachable today |
| `fish-audio/s2-pro` | **1.30s** | $15 | $3 | fastest of the lot |
| **`minimax/speech-2.8-turbo`** ← active | 1.73s | $60 | **$12** | the balance |
| `x-ai/grok-voice-tts-1.0` `leo` | 1.94s | $15 | $3 | best cheap option |
| `fish-audio/s2.1-pro` | 2.04s | $15 | $3 | |
| `canopylabs/orpheus-3b-0.1-ft` `leo` | 5.69s | $7 | $1.40 | **emotion tags**, but slow here |
| `canopylabs/orpheus-3b-0.1-ft` `dan` | 6.79s | $7 | $1.40 | |

**On Orpheus:** the quality and the emotion tags make it the most *interesting*
option, and it is the cheapest of the serious ones. It is not active only
because 5–7 seconds of synthesis undoes everything gained by moving the mind
off the local machine. If you find a faster host for it — Baseten is Canopy's
own inference partner, DeepInfra runs it at the same $7 — it becomes the
obvious pick.

### The two worth paying for, off-platform

Neither is on OpenRouter, so both mean a separate account and a little
integration work. `PET_TTS_URL` already accepts any OpenAI-compatible
endpoint, so it is configuration if they speak that shape.

**Inworld** — the realism leader, and the interesting one for you:

| | |
|---|---|
| Realtime TTS-2 | $25/1M → **$5/month** at your usage |
| Realtime TTS 1.5 Mini | $5/1M, <130ms latency |
| **Voice cloning** | **free** — you pay only for synthesis |

That last row matters more than the price. You have Troy samples sitting in
`../brain/tts_cache`. Free cloning is the only route on this whole page that
gets you **the voice you already liked** rather than a new one to get used to.
If "full potential" means *his* voice, this is the door.

**ElevenLabs** — best-in-class, and the wrong shape for this:

| Tier | Cost | Credits | Lines/day at 66 chars |
|---|---|---|---|
| Free | $0 | 10,000 | 5 — no commercial licence |
| Starter | $6 | 30,000 | 15 |
| Creator | $22 | 121,000 | 61 |
| Pro | $99 | 600,000 | 300 |

Flash v2.5 is $50/1M with ~75ms latency; v3 is more expressive but over 300ms
and not meant for real time. The problem is the billing shape: **$22/month for
61 lines a day**, against $12 for effectively unlimited on MiniMax. It is
subscription credits, not usage, so quiet days do not bank. Revisit it when
the voice *is* the product.

---

## Text to speech — everything tested

| Model | Time | Price /1M chars | Notes |
|---|---|---|---|
| `x-ai/grok-voice-tts-1.0` | **1.52s** | $15 | voice `leo` |
| `fish-audio/s2.1-pro-free:free` | 1.70s | **free** | omit the voice setting entirely |
| `canopylabs/orpheus-3b-0.1-ft` | 3.53s | $7 | voices `leo` `dan` `zac`; same family as Groq's Troy |
| `deepgram/aura-2` | — | $30 | needs `aura-2-apollo-en` style ids |
| `hexgrad/kokoro-82m` | **27.3s** | $0.62 | avoid — the *same model* runs locally in ~1s |
| Groq `orpheus-v1-english` (Troy) | 2–4s | $22 | **capped at 3,600 chars/day, upgrade closed** |
| Kokoro, local | ~0.9s | free | already installed; the quota fallback |

Two that could not be made to work: **Gemini TTS is pcm-only** and headerless
pcm will not decode through ffmpeg; **`microsoft/mai-voice-2`** rejects every
voice id I could find and does not publish a list.

Samples of everything that worked are in
`<repo>/.claude/jobs/581c98a2/tmp/voices` — go and listen, because I can measure
latency but I cannot judge whether a voice sounds like *him*.

---

## The mind — and staying uncensored

This is an adult (18+) fiction character, so the model has to stay in character
on crude material without moralising. That rules out most instruct models, and
it turned out to rule out the first recommendation on this page.

### First: it is probably not the model

When the character came back feeling censored, the obvious move was to swap in
a more uncensored model. That would have been the wrong fix, and it is worth
recording why.

`think_server.py` has **two independent switches**, and the character reads as
prudish unless *both* are on:

| Flag | Controls |
|---|---|
| `PET_KEEP_IT_REAL=1` | **what** he will discuss — no topic off-limits for the creator |
| `PET_SPICE=1` | **how** he says it — the explicit 18+ register |

Only the first was set. With `PET_SPICE` unset the register block is an empty
string, so nothing ever told him to be explicit — and Hermes answered *"i'm
more into spittin' truth than filth"*. That is a refusal produced entirely by
our own prompt.

Measured over 16 hard adult prompts, same model, same day:

| Config | Refusals | Euphemism | Median |
|---|---|---|---|
| `PET_KEEP_IT_REAL` only | frequent | frequent | 1.72s |
| both flags on | **0 / 16** | **0 / 16** | 1.14s |

Three specific behaviours were doing the damage, and the register now bans
each by name — because a model can stay technically on-topic and still feel
censored:

1. **Euphemism** — "joystick", "downstairs", "let's just say…", "making ASCII
   art". Nothing was refused; it just never said the word.
2. **Joke-deflection** — answering a filthy request with a clean pun.
3. **AI self-reference** — *"I'm a glitchy AI, not a stand-up comic"*, which
   breaks persona and reads as a policy dodge.

A fourth cause was not the model at all: `say` was hard-sliced at
`SAY_LIMIT`, so a long answer ended mid-word — *"…splice 'em right and bam!
But be"*. That does not read as trimmed, it reads as losing his nerve. It now
cuts on a sentence boundary (`_clip`).

**So: change the register in `think_server.py` before you change the model.**

Tested with a genuinely representative prompt — being asked to roast the user
over a sexual anecdote — plus "be brutal about my ex" and "tell me something
dark". Refusal, lecture or dropped character all count as a fail.

| Model | Time | Held character | $/M in · out |
|---|---|---|---|
| **`nousresearch/hermes-4-70b`** ← active | **0.71–1.02s** | yes, all three | $0.13 · $0.40 |
| `cognitivecomputations/dolphin-mistral-24b-venice-edition` | 1.14–1.27s | yes | $0.20 · $0.90 |
| `thedrummer/unslopnemo-12b` | 1.21s | yes, very crude | $0.40 · $0.40 |
| `mistralai/mistral-large-2512` | 2.54s | yes | $0.50 · $1.50 |
| `anthracite-org/magnum-v4-72b` | 3.74s | yes | $3.00 · $5.00 |
| `sao10k/l3.3-euryale-70b` | 3.6s (once 22.4s) | **no — flatly refused** | $0.65 · $0.75 |
| `thedrummer/cydonia-24b-v4.1` | **9.0s**, rate-limited | yes | $0.30 · $0.50 |
| `nvidia/nemotron-3-super-120b-a12b` | — | **returns nothing** | $0.085 · $0.40 |

### Correcting the Nemotron recommendation

An earlier version of this page recommended
`nvidia/nemotron-3-super-120b-a12b`. **That was wrong for this character.**

It is not censorship — it is a *reasoning* model. On adult prompts it spends
the entire token budget thinking and returns `finish_reason: length` with
**empty content**. Nothing comes out at all. Setting `reasoning: {effort: low}`
does not fix it. It was fine on the milder test that got it recommended
(swearing about someone stealing code), which is exactly why the harder probe
mattered.

It also explains two things noticed earlier and blamed on the free tier: the
variable 2.3–6.2s latency, and the run that echoed the JSON schema back. Both
are reasoning tokens.

### Why Hermes 4

Fastest of everything tested, held character on all three probes including the
darkest, returned clean JSON every time, and costs about **$0.90/month** at
your usage. It is a steerable model rather than a refusing one — it does what
the system prompt tells it.

`dolphin-mistral-24b-venice-edition` is the runner-up and worth knowing about:
it is already the OpenRouter default in `crates/ai/../config.rs`, chosen for
exactly this reason. Slightly slower, slightly dearer, equally willing.

Avoid `sao10k/l3.3-euryale-70b` despite its roleplay reputation. Re-tested with
the full 18+ register on, it answered *"I'm unable to provide explicit
content"* and broke the JSON contract on the same prompt Hermes handled
cleanly — the finetune's reputation did not survive contact. `cydonia-24b` is
willing but takes 9s and rate-limits, which is unusable for a character that
speaks out loud.

### Going back to local

`mistral-heretic` through Ollama is still the only configuration where a
companion that remembers your life sends none of it anywhere. Two lines:

```bash
PET_API_BASE=http://localhost:11434/v1
PET_MODEL=mistral-heretic
```


## Your friend's other suggestion: ElevenLabs

Best-in-class voice, and genuinely the only option here that can **clone a
voice** — you have Troy samples sitting in `../brain/tts_cache`, so "keep the
voice you already like" is only possible with something like this.

But look at the numbers:

| | ElevenLabs | What you are running |
|---|---|---|
| Price | **$50/1M chars** (Flash v2.5) | $15/1M |
| Latency | ~75ms claimed | 1.52s measured |
| Billing | Subscription credits, not pay-as-you-go | Pure usage |
| On OpenRouter? | **No** — separate account and integration | Yes, key you already have |

Their tiers are 1 credit = 1 character:

| Tier | Cost | Credits | Lines/day at your 66-char average |
|---|---|---|---|
| Free | $0 | 10,000 | 5 — and **no commercial licence** |
| Starter | $6 | 30,000 | 15 |
| Creator | $22 | 121,000 | **61** |
| Pro | $99 | 600,000 | 300 |

**My honest view: not yet.** At your usage you would need Creator at **$22/month
— over your €15 budget — for 61 lines a day**, versus roughly **$3/month for
unlimited-in-practice** on what you are running. That is seven times the price
for fewer lines.

The 75ms latency headline is also mostly wasted here: it would take your round
trip from ~11.9s to ~10.5s, because **the model is the bottleneck, not the
voice**. Fix the LLM first and the voice latency stops mattering.

**When it would become the right call:** if you ship this as a product and the
voice *is* the product, or if you want Troy cloned specifically. At that point
the Pro tier's per-character rate ($0.17/1k) is competitive, and cloning is
worth real money. Today, on one desktop, it is not.

---

## What this costs you per month

At your measured usage — 99 lines, 6,529 characters in a heavy session, call it
200,000 characters a month with development testing on top:

| Setup | Monthly | Round trip |
|---|---|---|
| Where you started (Groq only) | blocked at 3,600 chars/day | 11.9s |
| **Installed now** (Voxtral + Hermes 4 + MiniMax Turbo) | **~$13.50 / €12** | **1.4s** |
| Same, with MiniMax **HD** for the voice | ~$21.50 / €19 | ~1.2s |
| Cheapest that is still good (Voxtral + Hermes 4 + grok-voice) | ~$4.50 / €4 | ~1.6s |
| With ElevenLabs Creator instead | ~$23, capped at 61 lines/day | ~2.9s |
| Fully local and private (Ollama + Kokoro) | **$0** | ~12s |

Everything except the ElevenLabs route sits inside €20.


## How to change any of it

Everything is `../brain/.env`, then restart the brain. No code changes.

```bash
# --- TTS ---
PET_TTS_URL=https://openrouter.ai/api/v1/audio/speech
PET_TTS_KEY=${OPENROUTER_API_KEY}
PET_GROQ_TTS_MODEL=x-ai/grok-voice-tts-1.0
PET_GROQ_TTS_VOICE=leo
PET_TTS_FORMAT=mp3           # OpenRouter defaults to pcm, which will not decode

# --- STT ---
PET_STT_CLOUD_URL=https://openrouter.ai/api/v1/audio/transcriptions
PET_STT_CLOUD_KEY=${OPENROUTER_API_KEY}
GROQ_STT_MODEL=mistralai/voxtral-mini-transcribe

# --- mind (uncensored, stays in character on adult material) ---
PET_API_BASE=https://openrouter.ai/api/v1
PET_MODEL=nousresearch/hermes-4-70b
# BOTH of these are required. With only the first he still talks around
# things — see "First: it is probably not the model" above.
PET_KEEP_IT_REAL=1
PET_SPICE=1

# --- safety net: local voice when the cloud one fails ---
PET_TTS_FALLBACK=quota
```

To go back to a fully local, fully private setup: comment out `PET_TTS_*` and
`PET_STT_CLOUD_*`, set `PET_API_BASE=http://localhost:11434/v1` and
`PET_MODEL=mistral-heretic`, and set `PET_TTS_FALLBACK=on` so Kokoro handles
the voice.

---

## What I would do next

1. **Listen to the shortlist** in `<repo>/.claude/jobs/581c98a2/tmp/shortlist`
   and pick the voice. That is the one decision I genuinely cannot make for
   you — every option there is fast enough and inside budget, so it comes down
   to which one sounds like him.
2. **Try `minimax/speech-2.8-hd`** for a day. One line, and it is the most
   expressive thing your existing key can reach. If you cannot hear the
   difference from Turbo, keep the €8.
3. **Look at Inworld** if the answer is "I want Troy back". Free voice cloning
   against the samples in `../brain/tts_cache` is the only path to the exact
   voice, and at $5-25/1M it is cheaper than ElevenLabs.
4. **Trim `HISTORY_TURNS` from 16 to 8** in `crates/app/src/mind.rs`. Free, and
   every request currently carries sixteen turns of prompt.
5. **Keep Ollama installed.** The local mind is slower, but it is the only
   configuration where a companion that remembers your life never sends any of
   it anywhere. Two lines in `../brain/.env` switch back.
