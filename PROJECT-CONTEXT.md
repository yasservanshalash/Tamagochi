# Tamagotchi / Desktop Pet — Project Context Handoff

> Paste this into another model (e.g. GPT) as calibration context.
> **Do not commit real API keys.** Keys live in `brain/.env` only.

**Last updated:** 2026-07-30  
**Owner / creator:** Yasser  
**Repo:** `Tamagochi` (Watcher firmware + brain server + PC desktop pet)

---

## 1. What this project is

A living pixel-art character named **Yasser** (the pet) whose mind is an LLM gateway. Originally built for a **SenseCAP Watcher** (ESP32-S3 desk device). Extended to a **PC desktop pet** (Desktop Mate–like always-on-top companion) that talks through the same brain.

**Product vision (bigger picture):**
- Not “another chatbot UI” — **desktop Tamagotchis on PC**, Desktop Mate scale
- Platform eventually: many characters, sprites packs, personalities, voices, memory, loyalty
- Near-term: one perfect pet (Yasser) → character kit → runtime for many pets → later marketplace
- Differentiation: real conversation + memory + Tamagotchi care loop + optional voice ID (“only my creator gets the real answers”) + unhinged/loyal personalities

---

## 2. Architecture (keep this contract)

```
Body (Watcher firmware OR desktop-pet)
    → HTTP brain (think_server.py)
        → STT → LLM → TTS
        → memory (turns + facts)
```

**Bodies stay relatively dumb. Personality / models / memory live in the brain.**

| Piece | Path | Role |
|-------|------|------|
| Firmware pet | `firmware/examples/lumen-watcher/` | Real Watcher device UI, clips, mic, voice |
| Brain | `brain/think_server.py` | FastAPI mind: `/pet/think`, `/pet/converse`, `/pet/wake`, TTS |
| Desktop pet | `desktop-pet/` | PC always-on-top round window, same brain contract |
| Sprites | Watcher `main/pet/*.c` → converted to `desktop-pet/assets/*.png` | 54 LVGL sprites |

---

## 3. Hardware / network (Watcher era)

- Device: SenseCAP Watcher, ESP32-S3 (often `/dev/ttyACM1` or Windows COM; LAN IP e.g. `192.168.2.6`)
- Brain host: PC on LAN, port **8087**, bind `0.0.0.0`
- Point Watcher: `http://<watcher-ip>/brain?url=http://<pc-ip>:8087/pet/think`
- VPN (e.g. Proton) can break LAN — disable or allow LAN

**Hard firmware constraints (do not casually change):**
- `CONFIG_BSP_LCD_PANEL_SPI_TRANS_Q_DEPTH=1`
- LVGL task stack must stay internal (not PSRAM)
- Stream buffers static size quirks; voice ring teardown via refcount
- Never use `esp_http_client_perform()` if you need the body yourself
- Mic is interleaved 2-slot; playback vs mic concurrency rules

Open firmware bugs historically: listen→think crash intermittent; Wi-Fi boot flaky; voice cut at sentence end.

---

## 4. Brain server (`brain/`)

### Run
```powershell
cd brain
# .env loaded by run_brain.ps1
powershell -ExecutionPolicy Bypass -File .\run_brain.ps1
# or: uvicorn think_server:app --host 0.0.0.0 --port 8087
```

### `.env` (shape — fill secrets yourself)
```
OPENROUTER_API_KEY=...
GROQ_API_KEY=...
PET_API_BASE=http://localhost:11434/v1
PET_MODEL=mistral-heretic
PET_KEEP_IT_REAL=1
```

### Model routing
| Mode | Env | Notes |
|------|-----|--------|
| Local Heretic | `PET_API_BASE=http://localhost:11434/v1`, `PET_MODEL=mistral-heretic` | Ollama; GGUF in `models/Mistral-Heretic/` |
| OpenRouter | default base + e.g. `openai/gpt-4o-mini` or Grok | Faster/smarter; more refusals |

Health: `GET http://127.0.0.1:8087/health` → `{ ok, model, base }`

### Pipeline
1. **STT:** Groq Whisper (`whisper-large-v3-turbo`) if `GROQ_API_KEY`; else whisper.cpp / faster-whisper  
2. **LLM:** OpenAI-compatible chat completions → JSON `{ say, emotion, glitch, action }`  
3. **TTS:** Groq Orpheus English (`canopylabs/orpheus-v1-english`, voice default `troy`); fallback Kokoro then Piper  
4. **Memory:** `yasser_memory.json` — `turns`, `facts`, `creator_known`

### Endpoints
- `POST /pet/think` — JSON event → reply + optional `audio` URL  
- `POST /pet/converse` — raw PCM16 mono 16 kHz body + query persona/vitals/screen  
- `POST /pet/wake` — name-spot (“Yasser” variants), no full LLM chat  
- `GET /pet/tts_live/{token}` — streaming WAV-shaped PCM @ 16 kHz  
- `GET/POST/DELETE /pet/memory`

### Emotions (must match body renderers)
`idle, happy, talk, think, glitch, dance, celebrate, jump, confused, sad, scared, laugh, suspicious, whisper, point, facepalm, grumpy, stretch, busy`

### Actions
`none | camera | listen | home | sleep` — desktop mainly uses `listen` / `sleep`

---

## 5. Personality & “Keep it real” (important)

Character: **stoner / schizo / gangsta** pixel dude in beanie + headphones, loyal to creator **Yasser**, conspiratorial, slangy, can trip off-topic sometimes.

**`PET_KEEP_IT_REAL=1` (current intent):**
- **No “we’re writing a book / fiction mode” framing** — that made him sound like a poser
- He should stay in-character and answer authentically, including dark/adult/weapons topics **when talking to his creator**
- **Strangers / not identified:** gangsta bouncer — “who’s asking?”, “wouldn’t you like to know?”, stall, give nothing
- Once they say “it’s me / Yasser / the creator” → warm up and **stay open** for the session (`creator_known` in memory)
- Desktop sends `screen=desktop-pc` + high trust → identity lock as creator
- Ban fake stalls with creator: “lemme check the archive / hold up I’ll look” with no answer — must answer in the same reply (or bounce strangers)

**If `PET_KEEP_IT_REAL=0`:** hard-limit refusals for weapons/explosives even for creator (code + prompt).

**Anti-repeat / anti-bouncer / anti-stall:** brain may reroll LLM output if it echoes itself, ID-gates the creator, or stalls with archive bits.

**Owner’s creative goal:** experiment / authentic material for writing — pet should not *know* it’s “for a book”; authenticity > safety theater when identity-locked. Strangers still get bounced.

---

## 6. Desktop pet (`desktop-pet/`)

### Run
```powershell
cd desktop-pet
pip install -r requirements.txt
# optional: $env:PET_BRAIN="http://127.0.0.1:8087"; $env:PET_SCALE="1.3"; $env:PET_AUDIO_OUT="Razer"
python app.py
```

### Stack
- PySide6 frameless always-on-top round window  
- Clip engine ported from `scr_idle.c` (`clips.py`)  
- Mic: sounddevice 16 kHz PCM → `/pet/converse`  
- TTS: stream `/pet/tts_live` → stereo float OutputStream (mono RawOutputStream was silent on Razer/THX)  
- Wake word: idle mic → `/pet/wake`  
- Controls: click talk, drag move, right-click menu (type, react, sleep, quit)

### Known desktop issues seen in development
- Launching GUI from some agent shells exits instantly — user should start `python app.py` in a normal terminal  
- Prefer `Qt.Window` over `Qt.Tool` so the process doesn’t quit  
- Output device: default was Razer BlackShark; override with `PET_AUDIO_OUT`  
- Groq Orpheus: max 200 chars per synth chunk; longer says are split; 429 rate limits → Kokoro fallback  
- Heretic is slow → used to race `action=listen` opening mic before TTS; fixed with `voice_pending`

### Sprites
- `convert_sprites.py` converts Watcher LVGL C arrays (RGB565 swapped + alpha) → PNG  
- Drop higher-res PNGs into `assets/` with same names later  

---

## 7. Models & assets on disk

- `models/Mistral-Heretic/` — HF + GGUF quants; Ollama Modelfile → `mistral-heretic`  
- Ollama JSON mode + frequency/presence penalties for local models  
- Kokoro TTS weights expected under user home `kokoro/` when used as fallback  
- Groq Orpheus voices: `troy`, `austin`, `daniel`, `autumn`, `diana`, `hannah`  

---

## 8. Speaker recognition (planned, not implemented)

Owner asked: voice pattern recognition so the pet knows who is talking.

**Yes, available:** speaker verification/identification via embeddings (SpeechBrain, Resemblyzer, pyannote, or cloud Voice ID).

**Intended use:** enroll Yasser’s voice → on each mic clip, match embedding → set `creator_known` / skip bouncer. Caveats: noise, headset change, short clips, spoofing.

---

## 9. What the owner wants next (product)

- Bigger than one pet: **PC Tamagotchi platform** like Desktop Mate but Tamagotchi-native  
- Multi-character, better sprites (PC can handle more than Watcher)  
- Same mind contract; scale art + personality packs  
- Optional: Watcher as pocket body, PC as main stage  
- Voice ID for loyalty gating  
- Path: perfect Yasser → character kit format → multi-pet runtime → creator tools / marketplace later  

---

## 10. Working style notes (from Watcher CLAUDE.md + sessions)

- One mechanism-named fix per flash/version bump on device  
- `petlog` everything behavioral  
- After crash: read RTC black-box / `yasserctl.py log` first  
- Don’t commit guesses; instrument and reproduce  
- Server contract changes → brain only when possible  

---

## 11. Quick “current stack” snapshot (as of handoff)

| Layer | Choice |
|-------|--------|
| LLM | Local **mistral-heretic** via Ollama |
| STT | Groq Whisper large-v3-turbo |
| TTS | Groq Orpheus English (`troy`) |
| Mode | `PET_KEEP_IT_REAL=1` |
| Bodies | Watcher firmware + `desktop-pet` |
| Memory | `brain/yasser_memory.json` |

---

## 12. Files to open first

1. `brain/think_server.py` — mind, persona, modes, TTS/STT  
2. `brain/.env` — keys & model routing (secrets)  
3. `desktop-pet/app.py` — PC body  
4. `desktop-pet/clips.py` — emotion/clip tables  
5. `firmware/examples/lumen-watcher/main/screens/scr_idle.c` — original behavior bible  
6. `firmware/examples/lumen-watcher/CLAUDE.md` — device constraints  

---

## 13. Prompt for the next model

You can paste something like:

> Read `PROJECT-CONTEXT.md`. You are inheriting Yasser’s Tamagotchi + PC desktop-pet project. Prefer changing `think_server.py` for mind/persona; keep bodies thin. Owner is Yasser; keep-it-real mode means authentic answers for the creator and gangsta bounce for strangers — no “fiction/book” framing. Ask before destructive changes. Help with [YOUR TASK HERE].
