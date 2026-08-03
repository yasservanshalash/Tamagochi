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

## The short version

| Layer | Running now | Why |
|---|---|---|
| **STT** | `mistralai/voxtral-mini-transcribe` | 0.35s — twice as fast as anything else, read the line correctly |
| **LLM** | `mistral-heretic` (local, Ollama) | Free, private, uncensored — but **6–10s, and it is now the whole wait** |
| **TTS** | `x-ai/grok-voice-tts-1.0` voice `leo` | 1.52s — fastest that worked |

**The one change worth making:** the LLM. It is 6–10 seconds of a roughly
8–12 second round trip. Everything else is already under two seconds.

---

## Where the time actually goes

Measured end to end through `/pet/converse` (hear → think → reply): **11.9s**.

```
  1.0s   silence detection (he waits to be sure you finished)
  0.35s  speech to text
  6-10s  the model            <-- 70-85% of the wait
  1.5s   text to speech
```

Cutting TTS from 4.8s to 1.5s was worth doing. Cutting the model from 8s to 3s
would be worth twice as much.

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

## Text to speech

| Model | Time | Price /1M chars | Notes |
|---|---|---|---|
| **`x-ai/grok-voice-tts-1.0`** ← active | **1.52s** | $15 | voice `leo` |
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
`.claude/jobs/581c98a2/tmp/voices` — go and listen, because I can measure
latency but I cannot judge whether a voice sounds like *him*.

---

## The LLM — this is the one to change

Currently `mistral-heretic` locally through Ollama. It was chosen deliberately:
ordinary assistant models kept breaking the persona, and running locally means
your conversations never leave the machine.

The cost is speed. Measured on `/pet/think`: **6.3s and 9.5s**. Note the model
itself answers a trivial prompt in **0.32s** — the time goes on the persona
prompt plus sixteen turns of history, and on the brain's anti-repeat reroll,
which regenerates roughly **29% of replies** (15 of 52 in one session).

### Your friend's suggestion: `nvidia/nemotron-3-super-120b-a12b:free`

Tested it. **It is a good suggestion**, with caveats.

| | Result |
|---|---|
| Speed | **2.3s, 3.8s, 6.2s** across three runs — faster than local, but variable |
| Persona | **Holds it.** Asked it to go off about someone stealing code, it swore in character and did not refuse |
| Cost | Free tier: $0. Paid variant: **$0.085/M in, $0.40/M out ≈ $0.60/month** at your usage |
| Context | 262k free / 1M paid — vastly more than the 16 turns you use |

Two real problems with the **free** variant specifically:

1. **It echoed the schema.** One run returned
   `{"say":"str","emotion":"str","glitch":"int","action":"str"}` — literally the
   template. The brain would have put "str" in his speech bubble. Two of three
   runs were fine, but that is a failure mode you will see.
2. **Free endpoints and your private data.** This companion keeps a
   `facts` list about your life in `yasser_memory.json` and sends recent turns
   with every request. Free model endpoints commonly come with a data-sharing
   condition attached — check your privacy settings at
   openrouter.ai/settings/privacy before you point a companion that remembers
   personal things at one. Your current local setup has no such question.

**My recommendation: use the paid variant, not the free one.**
`nvidia/nemotron-3-super-120b-a12b` is about **$0.60/month** at your usage —
less than the TTS — and you get no queueing, no schema echoes from a
contended endpoint, and a clear data policy. That is the single biggest
improvement available to you: it should roughly halve the wait.

Also tested `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free`: it held the
persona but took **87 seconds** on one run. It is a reasoning model. Do not use
it for this.

### If you switch, keep the local model installed

`PET_API_BASE` and `PET_MODEL` swap it in one line, so keep Ollama as the
offline fallback. There is a real difference beyond speed: local means the
conversation stays on your machine.

---

## Your friend's other suggestion: ElevenLabs

Best-in-class voice, and genuinely the only option here that can **clone a
voice** — you have Troy samples sitting in `brain/tts_cache`, so "keep the
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

At your measured usage — 99 lines, 6,529 characters in a heavy session:

| Setup | Monthly |
|---|---|
| **Now** (local LLM + OpenRouter STT/TTS) | **~$3** |
| Recommended (paid Nemotron + OpenRouter STT/TTS) | **~$4** |
| With ElevenLabs Creator instead | **~$25**, and capped at 61 lines/day |
| Groq only, as it was | blocked — 3,600 chars/day, upgrade closed |

All well inside €15 except the ElevenLabs route.

---

## How to change any of it

Everything is `brain/.env`, then restart the brain. No code changes.

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

# --- LLM (the change I would make) ---
PET_API_BASE=https://openrouter.ai/api/v1
PET_MODEL=nvidia/nemotron-3-super-120b-a12b     # paid variant, not :free

# --- safety net: local voice when the cloud one fails ---
PET_TTS_FALLBACK=quota
```

To go back to a fully local, fully private setup: comment out `PET_TTS_*` and
`PET_STT_CLOUD_*`, set `PET_API_BASE=http://localhost:11434/v1` and
`PET_MODEL=mistral-heretic`, and set `PET_TTS_FALLBACK=on` so Kokoro handles
the voice.

---

## What I would do, in order

1. **Move the LLM to paid Nemotron.** Biggest win available: roughly halves the
   wait, costs about 60 cents a month.
2. **Trim `HISTORY_TURNS` from 16 to 8** in `crates/app/src/mind.rs`. Free, and
   every request carries half the prompt.
3. **Leave STT and TTS alone.** Both are already under two seconds and under
   four dollars a month combined.
4. **Revisit ElevenLabs when you ship**, not before — and specifically for
   voice cloning, which is the one thing nothing else here can do.
