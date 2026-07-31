"""Microphone capture and TTS playback for the desktop pet.

Player  — streams 16 kHz mono PCM16 to the speakers (upmixed to stereo);
          exposes .playing and .level (0-100 RMS at playout) for mouth visemes.
Recorder— push-to-talk mic capture at 16 kHz mono PCM16 with automatic
          stop on trailing silence (adaptive noise floor).
"""
import os
import threading
import time

import numpy as np
import sounddevice as sd

RATE = 16000


def _out_device():
    """Optional override: PET_AUDIO_OUT=<device index or substring>."""
    spec = os.environ.get("PET_AUDIO_OUT", "").strip()
    if not spec:
        return None
    try:
        return int(spec)
    except ValueError:
        spec_l = spec.lower()
        for i, d in enumerate(sd.query_devices()):
            if d["max_output_channels"] > 0 and spec_l in d["name"].lower():
                return i
        print("PET_AUDIO_OUT: no match for", spec)
        return None


class Player:
    """Feed PCM in with feed(); callback drains it. done() marks the end so
    playing flips false once the buffer empties.

    Uses float32 stereo OutputStream — Raw mono streams were arriving on the
    Razer/THX stack with zero audible output even when PCM was valid.
    """

    def __init__(self):
        self._buf = bytearray()
        self._lock = threading.Lock()
        self._ended = True
        self._stream = None
        self._empty_ms = 0
        self.level = 0
        self.playing = False
        self.active = False
        self.device = _out_device()

    def start(self):
        self.stop()
        with self._lock:
            self._buf = bytearray()
            self._ended = False
            self._empty_ms = 0
        self.level = 0
        self.playing = True
        self.active = True
        dev = self.device if self.device is not None else sd.default.device[1]
        try:
            name = sd.query_devices(dev)["name"]
        except Exception:
            name = str(dev)
        print(f"player: opening output device {dev!r} ({name})")
        self._stream = sd.OutputStream(
            samplerate=RATE, channels=2, dtype="float32",
            device=self.device,
            blocksize=RATE // 20,
            callback=self._cb, finished_callback=self._on_finished)
        self._stream.start()

    def feed(self, pcm: bytes):
        if not pcm:
            return
        with self._lock:
            self._buf.extend(pcm)
            self._empty_ms = 0

    def done(self):
        with self._lock:
            self._ended = True

    def buffered_ms(self) -> int:
        with self._lock:
            return (len(self._buf) // 2) * 1000 // RATE

    def stop(self):
        s, self._stream = self._stream, None
        if s:
            try:
                s.abort()
                s.close()
            except Exception:
                pass
        self.playing = False
        self.active = False
        self.level = 0

    def _cb(self, outdata, frames, t, status):
        need = frames * 2                      # bytes of mono s16
        block_ms = frames * 1000 // RATE
        with self._lock:
            chunk = bytes(self._buf[:need])
            del self._buf[:need]
            ended = self._ended
            if chunk:
                self._empty_ms = 0
            else:
                self._empty_ms += block_ms
            stop_now = ended and self._empty_ms >= 250
        if chunk:
            mono = np.frombuffer(chunk, dtype="<i2").astype(np.float32) / 32768.0
            if len(mono) < frames:
                mono = np.pad(mono, (0, frames - len(mono)))
            outdata[:, 0] = mono
            outdata[:, 1] = mono
            self.level = min(100, int(np.sqrt(np.mean(mono * mono)) * 100))
        else:
            outdata.fill(0)
            self.level = 0
        if stop_now:
            raise sd.CallbackStop

    def _on_finished(self):
        self.playing = False
        self.active = False
        self.level = 0


class Recorder:
    """Push-to-talk capture. start() opens the mic; capture auto-stops after
    trailing silence once speech was heard (or max_s). on_done(pcm) is called
    from the audio thread with the full clip (b"" if nothing usable)."""

    def __init__(self, on_done, max_s=30):
        self.on_done = on_done
        self.max_s = max_s
        self._stream = None
        self._chunks = []
        self._t0 = 0.0
        self._noise = 300.0
        self._spoke = False
        self._quiet_ms = 0
        self.level = 0
        self.active = False

    def start(self):
        if self.active:
            return
        self._chunks = []
        self._t0 = time.time()
        self._spoke = False
        self._quiet_ms = 0
        self.active = True
        self._stream = sd.InputStream(
            samplerate=RATE, channels=1, dtype="int16",
            blocksize=RATE // 50,
            callback=self._cb)
        self._stream.start()

    def stop(self):
        self._finish()

    def _finish(self):
        if not self.active:
            return
        self.active = False
        s, self._stream = self._stream, None
        if s:
            try:
                s.stop()
                s.close()
            except Exception:
                pass
        pcm = b"".join(self._chunks)
        self._chunks = []
        if len(pcm) < int(0.4 * RATE) * 2 or not self._spoke:
            pcm = b""
        self.on_done(pcm)

    def _cb(self, indata, frames, t, status):
        if not self.active:
            return
        data = bytes(indata)
        self._chunks.append(data)
        a = np.frombuffer(data, dtype="<i2").astype(np.float32)
        rms = float(np.sqrt(np.mean(a * a)))
        self.level = min(100, int(rms / 100))
        thresh = max(250.0, self._noise * 3)
        if rms > thresh:
            self._spoke = True
            self._quiet_ms = 0
        else:
            self._noise = 0.95 * self._noise + 0.05 * rms
            if self._spoke:
                self._quiet_ms += 20
        too_long = time.time() - self._t0 > self.max_s
        if (self._spoke and self._quiet_ms >= 1400) or too_long:
            threading.Thread(target=self._finish, daemon=True).start()


class WakeWatcher:
    """Always-on 'hey Yasser' listener."""

    def __init__(self, on_burst):
        self.on_burst = on_burst
        self._stream = None
        self._buf = []
        self._state = 0
        self._quiet_ms = 0
        self._cap_ms = 0
        self._noise = 300.0
        self.enabled = False
        self.paused = False

    def start(self):
        if self._stream:
            return
        self.enabled = True
        self._state = 0
        self._stream = sd.InputStream(
            samplerate=RATE, channels=1, dtype="int16",
            blocksize=RATE // 50, callback=self._cb)
        self._stream.start()

    def stop(self):
        self.enabled = False
        s, self._stream = self._stream, None
        if s:
            try:
                s.stop()
                s.close()
            except Exception:
                pass

    def _cb(self, indata, frames, t, status):
        if self.paused:
            self._state = 0
            self._buf = []
            return
        data = bytes(indata)
        a = np.frombuffer(data, dtype="<i2").astype(np.float32)
        rms = float(np.sqrt(np.mean(a * a)))
        thresh = max(250.0, self._noise * 3)
        loud = rms > thresh
        if not loud:
            self._noise = 0.95 * self._noise + 0.05 * rms
        if self._state == 0:
            if loud:
                self._state = 1
                self._buf = [data]
                self._cap_ms = 20
                self._quiet_ms = 0
        else:
            self._buf.append(data)
            self._cap_ms += 20
            self._quiet_ms = 0 if loud else self._quiet_ms + 20
            if self._quiet_ms >= 700 or self._cap_ms >= 3000:
                pcm = b"".join(self._buf)
                self._buf = []
                self._state = 0
                if self._cap_ms - self._quiet_ms >= 350:
                    self.on_burst(pcm)
                self._cap_ms = 0
