"""HTTP client for the Yasser brain (think_server.py).

Speaks the exact same contract as the Watcher firmware:
  POST /pet/think     JSON {event,text,vitals,persona,senses} -> reply
  POST /pet/converse  raw PCM16 mono 16k body + persona query -> reply
  POST /pet/wake      raw PCM16 body -> {"wake": bool, "heard": str}
  GET  /pet/tts_live/{token}  streaming WAV (16k mono s16le)
"""
import os
import time
from typing import Iterator, Optional

import httpx

BRAIN_URL = os.environ.get("PET_BRAIN", "http://127.0.0.1:8087").rstrip("/")

# Yasser-on-PC persona baseline: he's home, plugged in, talking to his maker.
# High trust / lower paranoia so Heretic doesn't stick in bouncer mode.
PERSONA = {"mood": 65, "paranoia": 45, "curiosity": 65,
           "energy": 60, "boredom": 40, "trust": 95}


def _vitals():
    return {"battery": 100, "charging": True,
            "hour": time.localtime().tm_hour}


def health(timeout=3.0) -> bool:
    try:
        return httpx.get(f"{BRAIN_URL}/health", timeout=timeout).status_code == 200
    except Exception:
        return False


def think(event: str, text: str = "", log: str = "") -> Optional[dict]:
    """Fire a think event. Returns reply dict or None on failure."""
    body = {"event": event, "text": text,
            "vitals": _vitals(), "persona": PERSONA,
            "senses": {"screen": "desktop-pc", "person": False,
                       "person_score": 0, "ip": "", "log": log}}
    try:
        r = httpx.post(f"{BRAIN_URL}/pet/think", json=body, timeout=40)
        r.raise_for_status()
        return r.json()
    except Exception as e:
        print("brain think error:", e)
        return None


def converse(pcm: bytes) -> Optional[dict]:
    """Send raw mic PCM (16k mono s16le); brain does STT + LLM + TTS."""
    q = {"battery": 100, "charging": 1, "hour": time.localtime().tm_hour,
         "screen": "desktop-pc", **PERSONA}
    try:
        r = httpx.post(f"{BRAIN_URL}/pet/converse", params=q, content=pcm,
                       headers={"Content-Type": "application/octet-stream"},
                       timeout=60)
        r.raise_for_status()
        return r.json()
    except Exception as e:
        print("brain converse error:", e)
        return None


def wake_check(pcm: bytes) -> bool:
    try:
        r = httpx.post(f"{BRAIN_URL}/pet/wake", content=pcm,
                       headers={"Content-Type": "application/octet-stream"},
                       timeout=20)
        r.raise_for_status()
        return bool(r.json().get("wake"))
    except Exception as e:
        print("wake check error:", e)
        return False


def stream_tts(audio_path: str, chunk=3200) -> Iterator[bytes]:
    """Yield raw PCM chunks from a /pet/tts_live/... URL (44-byte WAV
    header stripped). The server synthesizes sentence-by-sentence, so
    chunks arrive while later sentences are still being generated."""
    url = audio_path if audio_path.startswith("http") else BRAIN_URL + audio_path
    skipped = 0
    with httpx.stream("GET", url, timeout=httpx.Timeout(10, read=120)) as r:
        r.raise_for_status()
        for data in r.iter_bytes(chunk_size=chunk):
            if skipped < 44:
                cut = min(44 - skipped, len(data))
                skipped += cut
                data = data[cut:]
            if data:
                yield data
