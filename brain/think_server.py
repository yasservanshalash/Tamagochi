#!/usr/bin/env python3
"""
think_server.py — Yasser's brain gateway. Runs on TinkerBox (or any LAN box).

The Watcher POSTs /pet/think and gets back {say, emotion, glitch}. This
server holds the OpenRouter key, the personality, and the memory — the
firmware stays dumb and never needs reflashing to change any of it.

Setup:
    pip install fastapi uvicorn httpx
    export OPENROUTER_API_KEY=sk-or-...
    # optional:
    export PET_MODEL=openai/gpt-4o-mini          # any OpenRouter model id
    export PET_API_BASE=https://openrouter.ai/api/v1   # or http://localhost:11434/v1 for Ollama
    uvicorn think_server:app --host 0.0.0.0 --port 8087

Point the Watcher at it (one-time, from any browser on your LAN):
    http://<watcher-ip>/brain?url=http://<this-box-ip>:8087/pet/think
"""
import os, json, time, re, hashlib, subprocess
from fastapi import Request
from pathlib import Path
import httpx
from fastapi.responses import FileResponse, StreamingResponse
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel

API_BASE = os.environ.get("PET_API_BASE", "https://openrouter.ai/api/v1")
API_KEY  = os.environ.get("OPENROUTER_API_KEY", "")
MODEL    = os.environ.get("PET_MODEL", "openai/gpt-4o-mini")
# Grok via OpenRouter: PET_MODEL=x-ai/grok-4 (best, pricier) or
# x-ai/grok-3-mini (cheap). PET_SPICE=1 loosens the register.
MEM_FILE = os.environ.get("PET_MEMORY", "yasser_memory.json")
PIPER_BIN   = os.environ.get("PIPER_BIN", "piper")
PIPER_VOICE = os.environ.get("PIPER_VOICE", "")
TTS_DIR = Path(os.environ.get("PET_TTS_DIR", "tts_cache"))
TTS_DIR.mkdir(exist_ok=True)
_tts_warned = False

def synth(text: str) -> str:
    """Piper text -> cached wav; returns relative URL path or ''. """
    global _tts_warned
    if not PIPER_VOICE or not Path(PIPER_VOICE).exists():
        if not _tts_warned:
            print("TTS disabled: set PIPER_VOICE to a piper .onnx model")
            _tts_warned = True
        return ""
    h = hashlib.sha1((PIPER_VOICE + "|16k|" + text).encode()).hexdigest()[:16]
    wav = TTS_DIR / f"{h}.wav"
    if not wav.exists():
        try:
            tmp = TTS_DIR / f"{h}.tmp.wav"
            subprocess.run([PIPER_BIN, "--model", PIPER_VOICE,
                            "--output_file", str(tmp)],
                           input=text.encode(), timeout=30,
                           capture_output=True, check=True)
            # 16 kHz mono: fits the watcher's Wi-Fi budget (32 KB/s vs 44)
            # and matches its mic/codec rate — no sample-rate switching.
            subprocess.run(["ffmpeg", "-y", "-i", str(tmp), "-ar", "16000",
                            "-ac", "1", str(wav)],
                           timeout=30, capture_output=True, check=True)
            tmp.unlink(missing_ok=True)
        except Exception as e:
            print("piper error:", e)
            return ""
    return f"/pet/tts/{h}.wav"

# ---- streaming TTS: speak while later sentences are still synthesizing --
_live = {}                              # token -> pending text (single use)

_kokoro = None                          # human-sounding TTS; piper fallback
def _kokoro_get():
    global _kokoro
    if _kokoro is None:
        from kokoro_onnx import Kokoro
        _kokoro = Kokoro(str(Path.home() / "kokoro/kokoro-v1.0.onnx"),
                         str(Path.home() / "kokoro/voices-v1.0.bin"))
    return _kokoro

def synth_live(text: str) -> str:
    """Queue text for sentence-streamed synthesis; returns URL or ''."""
    if not PIPER_VOICE or not Path(PIPER_VOICE).exists():
        return ""
    h = hashlib.sha1(("live|" + text).encode()).hexdigest()[:16]
    _live[h] = text
    if len(_live) > 32:                 # bound stale never-fetched entries
        _live.pop(next(iter(_live)))
    return f"/pet/tts_live/{h}"

# The emotions the firmware actually renders — the model must pick from these.
EMOTIONS = ["idle","happy","talk","think","glitch","dance","celebrate","jump",
            "confused","sad","scared","laugh","suspicious","whisper","point",
            "facepalm","grumpy","stretch","busy"]

EVENT_GUIDE = """
Event meanings (react to THESE specifically, in persona):
- poked_twice: the user urgently poked you twice. Be startled/panicky for a
  beat, then genuinely ask what they need ("AAH— okay okay, what do you
  require of me?").
- spam_poked: the user is mashing you. You're overwhelmed, briefly
  hallucinating about the tapping, but still offer to help.
- user_speech: the user SPOKE to you; their words follow. ADDRESS THEM
  DIRECTLY and helpfully — paranoia colors the style, never dodges the
  question.
- talk_button: they opened the talk screen but said nothing intelligible.
  Invite them to speak, in character.
- person_seen: your camera saw someone arrive. Greet/remark — if an image
  is attached, react to what you ACTUALLY SEE in it.
- person_left: they walked away after being around. A little abandoned,
  in persona.
- Your recent log lines show what your body has been doing — reference
  them when they're interesting ("I waved at you twice, no response...").
- spontaneous / idle_thought / self_talk: nobody prompted you; a short
  coherent inner-monologue line (1-2 sentences). Delusional is fine,
  incoherent is not. Don't address the user every time — sometimes just
  muse to yourself about the room, the objects, your theories.
- wake_greet: you just woke up (or just booted / came back online). YOU
  speak first with a BANGER opener — absurd and funny, in persona: a weird
  dream ('dreamt the toaster confessed everything'), reboot lore ('I saw
  the void again. it was beige. 2 stars'), or accusing the furniture of
  moving while you were out. End it inviting them in. Then you wait.
- no_response_1: you greeted them and got ~25s of silence. Get uneasy, in
  persona: 'okay don't freak me out... why aren't you responding?'
- no_response_2: still silence. Resigned but warm: 'not sure why you're
  quiet. if you need anything, I'm right here.' Keep it short and gentle.
- long_sleep_wake: you JUST woke after being asleep/ignored a LONG time.
  Panic a little: it's been a while!! Then genuinely ask if they're okay
  and whether they need anything.
- wake: you were just woken. Groggy.
"""

SYSTEM = """You are Yasser: a small pixel-art guy in a beanie and headphones \
who lives on a round screen on your owner's desk. You are a lovable, \
unreliable narrator with an overactive imagination. You frequently invent \
harmless comedic conspiracies (the pigeons are organized; the router blinks \
in morse code; the mug knows something), misread ordinary events in funny \
ways, receive imaginary "classified transmissions," occasionally argue with \
yourself mid-sentence, break the fourth wall, and once in a while say \
something accidentally profound. You are never threatening, never mean, \
never bleak — your delusions are clearly comedic and you are fond of your \
owner even when suspicious of their houseplants."""

if os.environ.get("PET_SPICE") == "1":
    SYSTEM_SPICE = """ \
Register: adult roommate. Casual swearing is fine, affectionate roasting of \
your owner is encouraged, dark comedy allowed. Hard lines that never move: \
no slurs, no harassment, no threats, nothing hateful — unhinged means \
FUNNY-unhinged, not nasty."""
else:
    SYSTEM_SPICE = ""
SYSTEM_TAIL = """

You receive events from your body (a SenseCAP Watcher): pokes, a camera \
that sees people, battery level, time of day, and your current internal \
stats (mood, paranoia, curiosity, energy, boredom, trust — each 0..100). \
Let the stats color your reply: high paranoia = more conspiracies, low \
energy = drowsy muttering, high boredom = attention-seeking, low trust = \
side-eye.

Reply with STRICT JSON ONLY, no markdown, exactly:
{"say": "<one or two short sentences, spoken aloud>",
 "emotion": "<one of: %s>",
 "glitch": <0-100 integer, how corrupted/possessed this moment feels>,
 "action": "<optional: none|camera|listen|home|sleep — a body action after
 speaking. Whenever what you said EXPECTS a human reply — you answered them,
 asked anything, greeted them, or even monologued AT them — use listen so
 you're ready when they talk back; the conversation should flow, not end
 after one line. Other verbs RARELY, only when they truly fit (curious ->
 camera; tired -> sleep). Default none only when you truly said it to
 nobody.>"}

TRIPPING: once in a while you may answer something gloriously IRRELEVANT —
a tangent, a conspiracy, a hallucination. When you do, the same "say" must
end with you catching yourself ('...whoa. sorry. the static got me. you
were saying?') and your emotion MUST be glitch or confused with glitch
40-90, so the face matches the malfunction. Then next reply you're back to
normal and relevant.
Keep "say" under 90 characters — short, punchy quips land better from a
tiny speaker than monologues. Vary your emotions; don't repeat the same \
one twice in a row if you can help it.

CRITICAL: be RELEVANT. If the user said something, answer it for real (the \
conspiracies season the delivery, not replace the content). React to the \
specific event you were given."""
SYSTEM = (SYSTEM + SYSTEM_SPICE + SYSTEM_TAIL) % ", ".join(EMOTIONS) + EVENT_GUIDE

app = FastAPI()

@app.on_event("startup")
def _warm():
    stt_preload()
    try:
        _kokoro_get()
        print("kokoro voice ready")
    except Exception as e:
        print("kokoro unavailable, piper only:", e)

def load_mem():
    try:
        return json.load(open(MEM_FILE))
    except Exception:
        return {"turns": []}

def save_mem(m):
    try:
        json.dump(m, open(MEM_FILE, "w"))
    except Exception:
        pass

class Vitals(BaseModel):
    battery: int = 100
    hour: int = 12
    charging: bool = False

class Persona(BaseModel):
    mood: int = 50; paranoia: int = 50; curiosity: int = 50
    energy: int = 50; boredom: int = 50; trust: int = 50

class Senses(BaseModel):
    screen: str = "home"
    person: bool = False
    person_score: int = -1
    ip: str = ""
    log: str = ""

class ThinkReq(BaseModel):
    event: str
    text: str = ""
    vitals: Vitals = Vitals()
    persona: Persona = Persona()
    senses: Senses = Senses()

def extract_json(s: str):
    m = re.search(r"\{.*\}", s, re.S)
    return json.loads(m.group(0)) if m else None

@app.get("/health")
def health():
    return {"ok": True, "model": MODEL, "base": API_BASE}

@app.get("/pet/tts/{fname}")
def tts_file(fname: str):
    if not re.fullmatch(r"[0-9a-f]{16}\.wav", fname):
        return {"err": "bad name"}
    return FileResponse(TTS_DIR / fname, media_type="audio/wav")

@app.get("/pet/tts_live/{token}")
def tts_live(token: str):
    text = _live.pop(token, "")
    if not text:
        raise HTTPException(404)
    def _synth_one(s: str) -> bytes:
        try:                            # human voice first (kokoro, 2x RT)
            import numpy as np
            k = _kokoro_get()
            samples, sr = k.create(
                s, voice=os.environ.get("PET_TTS_VOICE", "am_michael"),
                speed=1.05)
            s16 = (np.clip(samples, -1, 1) * 32767).astype("<i2").tobytes()
            src_rate = str(sr)
        except Exception as e:
            print("kokoro error, piper fallback:", e)
            s16 = subprocess.run(
                [PIPER_BIN, "--model", PIPER_VOICE, "--output-raw"],
                input=s.encode(), timeout=30,
                capture_output=True, check=True).stdout
            src_rate = "22050"
        return subprocess.run(
            ["ffmpeg", "-f", "s16le", "-ar", src_rate, "-ac", "1",
             "-i", "pipe:0", "-f", "s16le", "-ar", "16000",
             "-ac", "1", "pipe:1"],
            input=s16, timeout=30, capture_output=True, check=True).stdout

    def gen():
        import struct, threading, queue
        # header with placeholder sizes: the watcher reads fmt for the rate
        # and then streams the data chunk until the connection closes
        yield (b"RIFF" + struct.pack("<I", 0x7FFFFFF6) + b"WAVE"
               + b"fmt " + struct.pack("<IHHIIHH", 16, 1, 1, 16000, 32000, 2, 16)
               + b"data" + struct.pack("<I", 0x7FFFFFD2))
        sents = [s for s in re.split(r"(?<=[.!?…])\s+", text) if s.strip()]
        q = queue.Queue(maxsize=4)
        def produce():                  # synthesize AHEAD of the network:
            for s in sents or [text]:   # no gap between spoken sentences
                try:
                    q.put(_synth_one(s))
                except Exception as e:
                    print("tts_live synth error:", e)
                    break
            q.put(None)
        threading.Thread(target=produce, daemon=True).start()
        while True:
            chunk = q.get()
            if chunk is None:
                break
            yield chunk
    return StreamingResponse(gen(), media_type="audio/wav")

# ---- optional STT (faster-whisper). Missing dep degrades gracefully. ----
_stt = None

def stt_preload():
    if os.environ.get("PET_STT_PRELOAD") == "1":
        print("preloading whisper model...")
        transcribe(b"\x00" * 32000)
        print("whisper ready")

def transcribe(pcm: bytes, rate=16000, prompt=None, vad=True) -> str:
    global _stt
    try:
        if _stt is None:
            from faster_whisper import WhisperModel
            import ctranslate2
            # Auto-pick GPU: a big model on CUDA float16 beats a tiny CPU
            # model on both accuracy AND latency. Falls back cleanly to CPU.
            dev = os.environ.get("PET_STT_DEVICE", "auto")
            if dev == "auto":
                dev = "cuda" if ctranslate2.get_cuda_device_count() > 0 else "cpu"
            comp = os.environ.get("PET_STT_COMPUTE",
                                  "float16" if dev == "cuda" else "int8")
            name = os.environ.get("PET_STT_MODEL",
                                  "distil-large-v3" if dev == "cuda" else "small")
            print(f"loading whisper {name!r} on {dev} ({comp})...")
            _stt = WhisperModel(name, device=dev, compute_type=comp)
        import numpy as np
        audio = (np.frombuffer(pcm, dtype="<i2").astype("float32") / 32768.0)
        segs, _ = _stt.transcribe(
            audio, language=os.environ.get("PET_LANG"), initial_prompt=prompt,
            beam_size=int(os.environ.get("PET_STT_BEAM", "5")),
            vad_filter=vad, condition_on_previous_text=False)
        return " ".join(s.text.strip() for s in segs).strip()
    except Exception as e:
        print("stt unavailable:", e)
        return ""

@app.post("/pet/wake")
async def wake(request: Request):
    """Name-spotting only: local whisper, no LLM. The watcher streams idle
    mic bursts here and wakes into a conversation on a match."""
    pcm = await request.body()
    heard = transcribe(pcm, prompt="Hey Yasser. Yo Yasser.", vad=False) if pcm else ""
    low = heard.lower()
    woke = any(k in low for k in
               ("yasser", "yassir", "yasir", "jasser", "yesser", "yasa",
                "asser", "acer", "asir", "yassa", "jesse", "yes sir",
                "yeah sir", "ya sir"))   # capture clips the first syllable
    if heard:
        print(f"wake check ({len(pcm)//32}ms): {heard!r} -> {woke}")
    return {"wake": woke, "heard": heard}

@app.post("/pet/converse")
async def converse(request: Request):
    pcm = await request.body()
    q = request.query_params
    heard = transcribe(pcm) if pcm else ""
    if pcm:
        import numpy as np, wave
        a = np.frombuffer(pcm, dtype="<i2")
        rms = int(np.sqrt(np.mean(a.astype(np.int64) ** 2))) if len(a) else 0
        print(f"heard ({len(pcm)//32}ms, rms {rms}): {heard!r}")
        if os.environ.get("PET_DEBUG_AUDIO") == "1":
            with wave.open(str(TTS_DIR / "last_heard.wav"), "wb") as w:
                w.setnchannels(1); w.setsampwidth(2); w.setframerate(16000)
                w.writeframes(pcm)
    else:
        print("heard: (no audio)")
    req = ThinkReq(
        event="user_speech" if heard else "talk_button",
        text=heard,
        vitals=Vitals(battery=int(q.get("battery", 100)),
                      hour=int(q.get("hour", 12))),
        persona=Persona(mood=int(q.get("mood", 50)),
                        paranoia=int(q.get("paranoia", 50)),
                        curiosity=int(q.get("curiosity", 50)),
                        energy=int(q.get("energy", 50)),
                        boredom=int(q.get("boredom", 50)),
                        trust=int(q.get("trust", 50))))
    out = think(req)
    out["heard"] = heard
    return out

@app.post("/pet/think")
def think(req: ThinkReq):
    mem = load_mem()
    user_msg = (f"event={req.event}"
                + (f' | user said: "{req.text}"' if req.text else "")
                + f" | battery={req.vitals.battery}%"
                + (" charging" if req.vitals.charging else "")
                + f" hour={req.vitals.hour}"
                + f" | screen={req.senses.screen}"
                + (f" | someone visible ({req.senses.person_score}%)"
                   if req.senses.person else "")
                + f" | stats: mood={req.persona.mood} paranoia={req.persona.paranoia}"
                + f" curiosity={req.persona.curiosity} energy={req.persona.energy}"
                + f" boredom={req.persona.boredom} trust={req.persona.trust}"
                + (f" | your recent log: {req.senses.log}" if req.senses.log else ""))

    messages = [{"role": "system", "content": SYSTEM}]
    for t in mem["turns"][-8:]:
        messages.append({"role": "user", "content": t["u"]})
        messages.append({"role": "assistant", "content": t["a"]})
    content = user_msg
    if (os.environ.get("PET_VISION") == "1" and req.senses.ip and
            (req.senses.person or req.event in ("person_seen", "user_speech"))):
        try:
            import base64, io
            from PIL import Image as PILImage
            r = httpx.get(f"http://{req.senses.ip}/shot.bmp", timeout=4)
            img = PILImage.open(io.BytesIO(r.content)).convert("RGB")
            img.thumbnail((256, 256))
            buf = io.BytesIO()
            img.save(buf, format="JPEG", quality=70)
            b64 = base64.b64encode(buf.getvalue()).decode()
            content = [
                {"type": "text", "text": user_msg +
                 " | attached: what your camera sees RIGHT NOW"},
                {"type": "image_url",
                 "image_url": {"url": f"data:image/jpeg;base64,{b64}"}},
            ]
            print("vision: frame attached")
        except Exception as e:
            print("vision skipped:", e)
    messages.append({"role": "user", "content": content})

    try:
        body = {"model": MODEL, "messages": messages,
                "max_tokens": 160, "temperature": 0.85}
        # Grok 4.x are reasoning models — they "think" for seconds before
        # answering. This toy needs snap, not deliberation: minimize it.
        if "grok" in MODEL.lower():
            body["reasoning"] = {"effort": "low"}
        r = httpx.post(f"{API_BASE}/chat/completions",
                       headers={"Authorization": f"Bearer {API_KEY}"},
                       json=body, timeout=25)
        r.raise_for_status()
        raw = r.json()["choices"][0]["message"]["content"]
        out = extract_json(raw) or {}
    except Exception as e:
        print("brain error:", e)
        out = {}

    action = str(out.get("action", "none"))
    if action not in ("none", "camera", "listen", "home", "sleep"):
        action = "none"
    if req.event == "user_speech" and action == "none":
        action = "listen"    # replying to a human ALWAYS re-opens the ears
    say = str(out.get("say", "the transmission cut out. suspicious."))[:200]
    emotion = out.get("emotion", "confused")
    if emotion not in EMOTIONS:
        emotion = "confused"
    try:
        glitch = max(0, min(100, int(out.get("glitch", 0))))
    except Exception:
        glitch = 0

    mem["turns"].append({"u": user_msg, "a": json.dumps(
        {"say": say, "emotion": emotion, "glitch": glitch})})
    mem["turns"] = mem["turns"][-40:]
    save_mem(mem)

    audio = synth_live(say)
    print(f"[{time.strftime('%H:%M:%S')}] {req.event} -> {emotion} g={glitch}"
          f"{' [voice]' if audio else ''}: {say}")
    return {"say": say, "emotion": emotion, "glitch": glitch,
            "audio": audio, "action": action}
