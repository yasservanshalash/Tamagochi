"""Yasser on the desktop — the Watcher pet as an always-on-top PC buddy.

Same brain (think_server.py), same sprites, same behavior grammar:
clip engine + emotion table ported from scr_idle.c, speech choreography
(emotion intro -> RMS-driven mouth visemes), self-talk, night sleep,
"hey Yasser" wake word, click-to-talk conversations.

Run:  python app.py            (brain must be up, default 127.0.0.1:8087)
Env:  PET_BRAIN=http://host:8087   PET_SCALE=1.3
"""
import os
import random
import sys
import threading
import time
from collections import deque
from pathlib import Path

from PySide6.QtCore import QPoint, QRectF, Qt, QTimer, Signal
from PySide6.QtGui import (QAction, QColor, QFont, QFontMetrics, QPainter,
                           QPainterPath, QPen, QPixmap)
from PySide6.QtWidgets import (QApplication, QInputDialog, QMenu, QWidget)

import brain
from audio import Player, Recorder, WakeWatcher
from clips import (C_IDLE, C_BLINK, C_POSS, C_SCARE, C_SLEEP, C_TALK, C_THINK,
                   EMO, VISEMES)

ASSETS = Path(__file__).resolve().parent / "assets"
SC = float(os.environ.get("PET_SCALE", "1.3"))
SIZE = int(412 * SC)

ENGAGED_MS = 10 * 60 * 1000
NAP_AFTER_MS = 20 * 60 * 1000


def now_ms() -> int:
    return int(time.monotonic() * 1000)


def is_night() -> bool:
    h = time.localtime().tm_hour
    return h >= 22 or h < 8


class PetWindow(QWidget):
    sig_reply = Signal(object, bool)     # (reply dict | None, from_ai)
    sig_rec_done = Signal(bytes)
    sig_wake = Signal()
    sig_voice_done = Signal()

    def __init__(self):
        super().__init__()
        self.setWindowTitle("Yasser")
        # Qt.Tool often lets the app quit instantly on Windows when launched
        # from a non-interactive shell; use a real top-level Window instead.
        self.setWindowFlags(Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint
                            | Qt.Window)
        self.setAttribute(Qt.WA_TranslucentBackground)
        self.setFixedSize(SIZE, SIZE)
        self._place_default()

        # sprites (pre-scaled, nearest-neighbor to keep the pixel art crisp)
        self.pix, self.dim = {}, {}
        for f in ASSETS.glob("*.png"):
            pm = QPixmap(str(f))
            self.dim[f.stem] = (pm.width(), pm.height())
            self.pix[f.stem] = pm.scaled(
                int(pm.width() * SC), int(pm.height() * SC),
                Qt.IgnoreAspectRatio, Qt.FastTransformation)

        # ---- clip engine state (port of scr_idle.c) ----
        self.base, self.shot = C_IDLE, None
        self.fidx, self.fms = 0, C_IDLE.frames[0].ms
        self.fx_idx, self.fx_ms_left = 0, 0
        self.hold = 0
        self.blink_in = 3000
        self.last_emo = "idle"
        self.intro_ms = 0
        self.prev_voice = False
        self.talk_fx = ()
        self.glitch_flash = 0
        self.viseme = None           # overrides frame while speaking

        # ---- behavior state ----
        self.sub_text, self.sub_ms = "", 0
        self.state = "idle"          # idle | listening | thinking | speaking
        self.busy = False            # brain request in flight
        self.voice_pending = False   # True from audio URL until playout ends
        self.pending_action = "none"
        self.user_touch = now_ms()
        self.sleep_since = 0
        self.self_talk_in = 90_000
        self.self_talk_on = True
        self.brain_ok = False
        self.log = deque(maxlen=12)
        self._drag = None
        self._moved = False

        # ---- audio ----
        self.player = Player()
        self.recorder = Recorder(on_done=self.sig_rec_done.emit)
        self.wake = WakeWatcher(on_burst=self._wake_burst)

        self.sig_reply.connect(self._on_reply)
        self.sig_rec_done.connect(self._on_rec_done)
        self.sig_wake.connect(self._on_wake)

        self.t_anim = QTimer(self, interval=30, timeout=self._anim_tick)
        self.t_brain = QTimer(self, interval=100, timeout=self._brain_tick)
        self.t_slow = QTimer(self, interval=1000, timeout=self._slow_tick)
        self.t_anim.start(); self.t_brain.start(); self.t_slow.start()

        threading.Thread(target=self._boot, daemon=True).start()

    # ---------------- boot ----------------
    def _boot(self):
        self.brain_ok = brain.health()
        if self.brain_ok:
            time.sleep(1.5)
            self.petlog("boot")
            self._think_async("wake_greet", from_ai=False)
        try:
            self.wake.start()
        except Exception as e:
            print("wake watcher mic error:", e)

    def _place_default(self):
        g = QApplication.primaryScreen().availableGeometry()
        self.move(g.right() - SIZE - 24, g.bottom() - SIZE - 24)

    def petlog(self, s: str):
        line = f"{time.strftime('%H:%M')} {s}"
        self.log.append(line)
        print("pet:", line)

    # ---------------- clip engine ----------------
    def set_base(self, c):
        self.base, self.shot = c, None
        self.fidx = self.fx_idx = 0
        self._set_frame()

    def play_shot(self, c):
        self.shot = c
        self.fidx = self.fx_idx = 0
        self._set_frame()

    def _set_frame(self):
        c = self.shot or self.base
        self.fidx = min(self.fidx, len(c.frames) - 1)
        self.fms = c.frames[self.fidx].ms
        self.fx_ms_left = c.fx_ms
        self.viseme = None
        self.update()

    def _anim_tick(self):
        if (not self.shot and self.base is C_TALK and self.player.playing):
            lv = self.player.level
            v = 0 if lv < 8 else 1 if lv < 30 else 2 if lv < 60 else 3
            if self.viseme != v:
                self.viseme = v
                self.update()
            return
        c = self.shot or self.base
        if c.fx and len(c.fx) > 1:
            self.fx_ms_left -= 30
            if self.fx_ms_left <= 0:
                self.fx_ms_left = c.fx_ms
                self.fx_idx += 1
                self.update()
        if self.glitch_flash > 0:
            self.glitch_flash -= 30
            if self.glitch_flash <= 0:
                self.update()
        self.fms -= 30
        if self.fms > 0:
            return
        self.fidx += 1
        if self.fidx >= len(c.frames):
            if self.shot:
                self.shot = None
                self.fidx = 0
            elif c.loop:
                self.fidx = 0
            else:
                self.fidx = len(c.frames) - 1
        self._set_frame()

    # ---------------- behavior scheduler (100 ms) ----------------
    def _brain_tick(self):
        # Mouth / talk clip ONLY while speakers are actually playing.
        # voice_pending = TTS still cooking — keep think face, block mic,
        # but don't lip-flap on silent text (Orpheus 429 / empty stream).
        audible = self.player.active or self.player.playing
        fetching = self.voice_pending and not audible
        gate = audible or self.voice_pending
        if audible != self.prev_voice:
            self.prev_voice = audible
            if audible:
                ec = EMO.get(self.last_emo, EMO["confused"]).clip
                emotive = ec is not C_TALK and ec is not C_IDLE
                self.intro_ms = 1500 if emotive else 0
                self.talk_fx = ec.fx if emotive else ()
                self.state = "speaking"
            else:
                self.talk_fx = ()
                if self.state == "speaking":
                    self.state = "idle"
        if audible:
            self.sub_ms = max(self.sub_ms, 1500)
            if self.intro_ms > 0:
                self.intro_ms -= 100
            elif not self.shot and self.base not in (C_TALK, C_POSS):
                self.set_base(C_TALK)
            if self.base in (C_TALK, C_POSS):
                self.hold = 500
        elif fetching:
            # Waiting on TTS — think, don't talk-animate
            if not self.shot and self.base is not C_THINK:
                self.set_base(C_THINK)
            self.hold = max(self.hold, 300)

        if self.hold > 0:
            self.hold -= 100
            if self.hold <= 0:
                if audible:
                    self.set_base(C_TALK)
                elif fetching:
                    self.set_base(C_THINK)
                else:
                    # Never nap on hold-end if ears are about to open or the
                    # user just poked him — that used to eat action=listen at
                    # night (sleep -> wake_greet loop, never opens mic).
                    self.set_base(C_IDLE)
            return

        # blink only when properly idle
        if self.base is C_IDLE and not self.shot and not gate:
            self.blink_in -= 100
            if self.blink_in <= 0:
                self.blink_in = 3000 + random.randint(0, 5000)
                self.play_shot(C_BLINK)

        # act on the mind's verb once he's done speaking
        if not gate and not self.busy and self.pending_action != "none":
            act, self.pending_action = self.pending_action, "none"
            self.petlog(f"mind chose: {act}")
            if act == "listen" and self.state == "idle":
                self.start_listen()
            elif act == "sleep":
                self.sleep_since = now_ms()
                self.set_base(C_SLEEP)

        # presence: self-talk while engaged, nap when ignored
        since = now_ms() - self.user_touch
        want_nap = (
            not gate and not self.shot and not self.busy
            and self.base is not C_SLEEP and self.state == "idle"
            and self.pending_action == "none"
        )
        if want_nap and since > NAP_AFTER_MS:
            self.petlog(f"ignored {since // 60000} min -> nap")
            self.sleep_since = now_ms()
            self.set_base(C_SLEEP)
            return
        # night nap only after he's been left alone a bit (not mid-chat)
        if want_nap and is_night() and since > 90_000:
            self.petlog("night -> nap")
            self.sleep_since = now_ms()
            self.set_base(C_SLEEP)
            return
        if (self.self_talk_on and since < ENGAGED_MS and self.brain_ok
                and not gate and not self.busy and self.state == "idle"
                and self.base is not C_SLEEP):
            self.self_talk_in -= 100
            if self.self_talk_in <= 0:
                self.self_talk_in = 60_000 + random.randint(0, 90_000)
                self.petlog("self-talk")
                self._think_async("self_talk", from_ai=True)

        # Wake word stays armed while napping so "Yasser" can wake him.
        # Still paused during speak / TTS fetch / listen (mic is busy).
        self.wake.paused = (gate or self.busy or self.state != "idle")

    def _slow_tick(self):
        if self.sub_ms > 0:
            self.sub_ms -= 1000
            if self.sub_ms <= 0:
                self.update()
        if random.random() < 0.05:
            threading.Thread(target=self._health_check, daemon=True).start()
        self.update()          # clock refresh

    def _health_check(self):
        self.brain_ok = brain.health()

    # ---------------- emotions ----------------
    def do_emotion(self, name, glitch=0, hold_ms=0, from_ai=False):
        e = EMO.get(name, EMO["confused"])
        self.last_emo = name if name in EMO else "confused"
        self.petlog(f"react: {self.last_emo} (glitch={glitch})")
        if self.base is C_SLEEP:
            if from_ai:
                return
            self._wake_up()
            return
        if glitch >= 60 and e.clip is not C_POSS:
            self.play_shot(C_POSS)
            self.glitch_flash = 450
        if e.is_base:
            self.set_base(e.clip)
            self.hold = hold_ms or e.def_hold
        else:
            self.play_shot(e.clip)

    def _soft_wake(self):
        """Leave sleep without an LLM wake_greet (so listen can open)."""
        if self.base is C_SLEEP:
            slept = now_ms() - self.sleep_since
            self.set_base(C_IDLE)
            self.hold = 0
            self.petlog(f"soft-wake after {slept // 60000} min")

    def _wake_up(self):
        slept = now_ms() - self.sleep_since
        self.set_base(C_IDLE)
        self.play_shot(C_SCARE)
        self.hold = 8000
        self.petlog(f"woken after {slept // 60000} min")
        self._think_async("wake_greet", from_ai=False)

    def say(self, text, ms=0):
        self.sub_text = text
        self.sub_ms = ms or max(2600, 60 * len(text))
        self.update()

    # ---------------- conversation flow ----------------
    def start_listen(self):
        if self.busy or self.player.active or self.voice_pending:
            self.petlog(f"listen blocked (busy/voice, state={self.state})")
            self.say("hold up — still talking...", 1800)
            return
        if self.state == "listening":
            return
        if self.state not in ("idle", "speaking"):
            self.petlog(f"listen blocked (state={self.state})")
            return
        self.user_touch = now_ms()
        # Soft-wake only. Full wake_greet here ate the mic open and looped.
        self._soft_wake()
        self.player.stop()
        self.wake.paused = True
        try:
            self.recorder.start()
        except Exception as e:
            self.say("mic error. check input device.")
            print("mic error:", e)
            self.state = "idle"
            return
        self.state = "listening"
        self.petlog("listening")
        self.say("listening... (click me to send)", 60_000)

    def _on_rec_done(self, pcm: bytes):
        if self.state != "listening":
            return
        if not pcm:
            self.state = "idle"
            self.say("didn't catch anything.", 1800)
            self.do_emotion("confused")
            return
        self.state = "thinking"
        self.busy = True
        self.petlog(f"heard {len(pcm) // 32} ms, asking the mind")
        self.say("...", 30_000)
        self.set_base(C_THINK)
        self.hold = 0
        threading.Thread(target=self._converse_worker, args=(pcm,),
                         daemon=True).start()

    def _converse_worker(self, pcm):
        out = brain.converse(pcm)
        self.sig_reply.emit(out, False)

    def _think_async(self, event, text="", from_ai=True):
        if self.busy:
            return
        self.busy = True
        if not from_ai:
            pass

        def run():
            out = brain.think(event, text, log=" | ".join(list(self.log)[-6:]))
            self.sig_reply.emit(out, from_ai)
        threading.Thread(target=run, daemon=True).start()

    def _on_reply(self, out, from_ai):
        self.busy = False
        if self.state == "thinking":
            self.state = "idle"
        if not out:
            self.brain_ok = False
            self.say("brain unreachable. is the server up?", 4000)
            self.do_emotion("sad", 0, 3000, from_ai)
            return
        self.brain_ok = True
        say = out.get("say", "")
        emotion = out.get("emotion", "confused")
        glitch = int(out.get("glitch", 0) or 0)
        self.pending_action = out.get("action", "none")
        heard = out.get("heard")
        if heard:
            self.petlog(f'heard: "{heard[:60]}"')
        audio = out.get("audio", "")
        if say:
            self.say(say)
        if audio:
            # Remember emo for when audio actually starts; don't mouth-flap
            # on talk/idle while TTS is still fetching (silent mime looks bad).
            self.last_emo = emotion if emotion in EMO else "confused"
            if emotion not in ("talk", "whisper", "idle", "busy"):
                self.do_emotion(emotion, glitch, 0, from_ai)
            else:
                self.set_base(C_THINK)
                self.hold = 0
            self.voice_pending = True
            threading.Thread(target=self._voice_worker, args=(audio,),
                             daemon=True).start()
        else:
            self.do_emotion(emotion, glitch, 0, from_ai)

    def _voice_worker(self, audio_path):
        try:
            # Pre-buffer ~400 ms before opening the speaker so we don't
            # start playing into a hole while Orpheus is still cooking.
            it = brain.stream_tts(audio_path)
            primed = bytearray()
            for chunk in it:
                primed.extend(chunk)
                if len(primed) >= int(0.4 * 16000) * 2:
                    break
            if len(primed) < 1600:
                raise RuntimeError(
                    f"TTS stream empty/short ({len(primed)} B) — Orpheus fail?")
            import numpy as np
            peak = int(np.max(np.abs(
                np.frombuffer(bytes(primed[:96000]), dtype="<i2"))))
            print(f"voice: primed {len(primed)} B peak={peak}", flush=True)
            if peak < 50:
                raise RuntimeError(f"TTS stream silent (peak={peak})")
            # Talk animation arms only once player.start() sets .playing
            self.player.start()
            self.player.feed(bytes(primed))
            total = len(primed)
            for chunk in it:
                self.player.feed(chunk)
                total += len(chunk)
            self.player.done()
            print(f"voice: fed {total} B, draining...", flush=True)
            # Wait until the speaker actually drains so listen can't race.
            while self.player.active:
                time.sleep(0.05)
            print("voice: done", flush=True)
        except Exception as e:
            print("voice error:", e, flush=True)
            self.player.stop()
            self.petlog("tts failed — no talk anim")
        finally:
            self.voice_pending = False

    # ---------------- wake word ----------------
    def _wake_burst(self, pcm):
        if self.wake.paused or not self.wake.enabled:
            return

        def run():
            if brain.wake_check(pcm):
                self.sig_wake.emit()
        threading.Thread(target=run, daemon=True).start()

    def _on_wake(self):
        if self.state != "idle" or self.busy or self.player.active or self.voice_pending:
            return
        self.petlog("wake word heard")
        self.user_touch = now_ms()
        # Soft-wake + listen. Full wake_greet blocked the mic and made him
        # monologue the same Discord bit on repeat.
        self._soft_wake()
        self.do_emotion("confused", 0, 800)
        QTimer.singleShot(400, self.start_listen)

    # ---------------- input ----------------
    def mousePressEvent(self, ev):
        if ev.button() == Qt.LeftButton:
            self._drag = ev.globalPosition().toPoint() - self.pos()
            self._moved = False

    def mouseMoveEvent(self, ev):
        if self._drag is not None:
            p = ev.globalPosition().toPoint() - self._drag
            if (ev.globalPosition().toPoint() - (self.pos() + self._drag)) \
                    .manhattanLength() > 6 or self._moved:
                self._moved = True
                self.move(p)

    def mouseReleaseEvent(self, ev):
        if ev.button() != Qt.LeftButton:
            return
        drag, self._drag = self._drag, None
        if self._moved or drag is None:
            return
        self.user_touch = now_ms()
        if self.base is C_SLEEP:
            self._wake_up()
        elif self.state == "listening":
            self.recorder.stop()
        elif self.player.playing:
            self.player.stop()
        elif self.state == "idle" and not self.busy:
            self.start_listen()

    def contextMenuEvent(self, ev):
        m = QMenu(self)
        m.addAction("Listen / talk", self.start_listen)
        m.addAction("Type to Yasser...", self._type_dialog)
        m.addSeparator()
        a_wake = QAction("Wake word \"Yasser\"", m, checkable=True,
                         checked=self.wake.enabled)
        a_wake.toggled.connect(self._toggle_wake)
        m.addAction(a_wake)
        a_self = QAction("Self-talk", m, checkable=True,
                         checked=self.self_talk_on)
        a_self.toggled.connect(lambda v: setattr(self, "self_talk_on", v))
        m.addAction(a_self)
        a_top = QAction("Always on top", m, checkable=True,
                        checked=bool(self.windowFlags()
                                     & Qt.WindowStaysOnTopHint))
        a_top.toggled.connect(self._toggle_top)
        m.addAction(a_top)
        rx = m.addMenu("React")
        for name in EMO:
            rx.addAction(name, lambda n=name: self.do_emotion(n))
        m.addSeparator()
        if self.base is C_SLEEP:
            m.addAction("Wake up", self._wake_up)
        else:
            m.addAction("Sleep", lambda: (setattr(self, "sleep_since",
                                                  now_ms()),
                                          self.set_base(C_SLEEP)))
        m.addAction("Quit", QApplication.quit)
        m.exec(ev.globalPos())

    def _type_dialog(self):
        text, ok = QInputDialog.getText(self, "Yasser", "say something:")
        if ok and text.strip():
            self.user_touch = now_ms()
            self.state = "thinking"
            self.say("...", 30_000)
            self.set_base(C_THINK)
            self._think_async("user_speech", text.strip(), from_ai=False)

    def _toggle_wake(self, on):
        if on:
            try:
                self.wake.start()
            except Exception as e:
                print("wake mic error:", e)
        else:
            self.wake.stop()

    def _toggle_top(self, on):
        f = self.windowFlags()
        f = f | Qt.WindowStaysOnTopHint if on else f & ~Qt.WindowStaysOnTopHint
        self.setWindowFlags(f)
        self.show()

    # ---------------- painting ----------------
    def paintEvent(self, ev):
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        clip = QPainterPath()
        clip.addEllipse(QRectF(0, 0, SIZE, SIZE))
        p.setClipPath(clip)

        p.drawPixmap(0, 0, self.pix["img_pet_bg"].scaled(SIZE, SIZE))

        # character, bottom-mid anchored like the firmware
        c = self.shot or self.base
        fr = c.frames[min(self.fidx, len(c.frames) - 1)]
        img = fr.img
        if (self.viseme is not None and not self.shot and self.base is C_TALK
                and self.player.playing):
            img = VISEMES[self.viseme]
        w0, h0 = self.dim[img]
        cx = 206 + fr.dx
        top = 412 - 26 - fr.dy - h0
        p.drawPixmap(int((cx - w0 / 2) * SC), int(top * SC), self.pix[img])

        # FX bubble above-right of the head
        fx_list = self.talk_fx if (self.talk_fx and self.base is C_TALK) \
            else c.fx
        if fx_list:
            fx = fx_list[self.fx_idx % len(fx_list)]
            fw, fh = self.dim[fx]
            p.drawPixmap(int((cx + 44 - fw / 2) * SC),
                         int((top + 34 - fh) * SC), self.pix[fx])

        if self.glitch_flash > 0:
            st = "img_fx_static1" if self.glitch_flash % 200 < 100 \
                else "img_fx_static2"
            sw, sh = self.dim[st]
            p.setOpacity(0.75)
            p.drawPixmap(int((206 - sw / 2) * SC), int((190 - sh / 2) * SC),
                         self.pix[st])
            p.setOpacity(1.0)

        # chrome: clock + brain status dot
        p.setPen(QColor(225, 228, 220))
        f = QFont("Segoe UI", int(13 * SC), QFont.DemiBold)
        p.setFont(f)
        p.drawText(QRectF(0, 52 * SC, SIZE, 30 * SC), Qt.AlignHCenter,
                   time.strftime("%H:%M"))
        dot = QColor(90, 200, 120) if self.brain_ok else QColor(220, 80, 80)
        p.setBrush(dot)
        p.setPen(Qt.NoPen)
        p.drawEllipse(QRectF(SIZE / 2 - 4 * SC, 36 * SC, 8 * SC, 8 * SC))

        # subtitle
        if self.sub_text and self.sub_ms > 0:
            fs = QFont("Segoe UI", int(11 * SC))
            p.setFont(fs)
            fm = QFontMetrics(fs)
            wpx = int(320 * SC)
            rect = fm.boundingRect(0, 0, wpx - int(12 * SC), 1000,
                                   Qt.TextWordWrap, self.sub_text)
            bh = rect.height() + int(12 * SC)
            bx = (SIZE - wpx) / 2
            by = SIZE - 34 * SC - bh
            p.setBrush(QColor(14, 18, 22, 178))
            p.setPen(Qt.NoPen)
            p.drawRoundedRect(QRectF(bx, by, wpx, bh), 10 * SC, 10 * SC)
            p.setPen(QColor(230, 233, 226))
            p.drawText(QRectF(bx + 6 * SC, by + 6 * SC, wpx - 12 * SC, bh),
                       Qt.AlignHCenter | Qt.TextWordWrap, self.sub_text)

        # state ring
        ring = None
        if self.state == "listening":
            pulse = 120 + min(120, self.recorder.level * 3)
            ring = QColor(235, 80, 70, pulse)
        elif self.state == "thinking" or self.busy:
            ring = QColor(240, 180, 60, 170)
        elif self.player.playing:
            ring = QColor(90, 200, 120, 150)
        p.setBrush(Qt.NoBrush)
        if ring:
            p.setPen(QPen(ring, 5 * SC))
            p.drawEllipse(QRectF(3 * SC, 3 * SC, SIZE - 6 * SC,
                                 SIZE - 6 * SC))
        # bezel
        p.setPen(QPen(QColor(8, 10, 12, 230), 2))
        p.drawEllipse(QRectF(1, 1, SIZE - 2, SIZE - 2))


def main():
    app = QApplication(sys.argv)
    app.setQuitOnLastWindowClosed(True)
    w = PetWindow()
    w.show()
    sys.exit(app.exec())


if __name__ == "__main__":
    main()
