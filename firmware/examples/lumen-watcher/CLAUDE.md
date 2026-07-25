# Lumen Watcher — Yasser AI Tamagotchi

Custom firmware replacing SenseCAP Watcher stock: a living pixel-art character
(Yasser) whose mind is an LLM gateway (`think_server.py` on the LAN). You are
inheriting this at v0.43 after 43 debugging iterations. Your mission: make it
stable and bug-free. The device is REAL HARDWARE on the desk — treat flashes
and reboots as physical acts.

## Environment
- Device: SenseCAP Watcher, ESP32-S3, usually /dev/ttyACM1, IP 192.168.1.62
- Brain server: laptop 192.168.1.59:8087 (`~/run_brain.sh`; think_server.py in ~/Downloads)
- This dir builds INSIDE SenseCAP-Watcher-Firmware/examples/

## Commands
- Build: `source ~/watcher-dev/esp-idf/export.sh && idf.py build`
- Flash: `idf.py -p /dev/ttyACM1 flash` (or `./flash_prebuilt.sh /dev/ttyACM1` for prebuilts)
- Serial: `idf.py -p /dev/ttyACM1 monitor` (Ctrl+] exits) — panics/backtraces appear here
- Remote debug (device must have Wi-Fi): `python3 yasserctl.py info|log|follow|shot|say|react|nav|reboot`
  - `follow` = live log tail; survives reboots; after a crash the first lines
    served are the RTC black-box `was:` trail (last 8 pet-log lines pre-death)
- Version lives in `main/lumen.h` (LUMEN_VERSION). Bump on every build you flash.

## HARD CONSTRAINTS — violating these re-introduces solved catastrophes
1. `CONFIG_BSP_LCD_PANEL_SPI_TRANS_Q_DEPTH=1`. NEVER raise it. Depth 2 =
   white-screen boot loop (this SPD2010's chunked QSPI cannot overlap
   transactions). The audio-vs-LCD collision is mitigated instead by: voice
   playout task on core 0, display refresh 15ms→60ms while `voice_playing`,
   no chrome redraws during speech.
2. `CONFIG_LVGL_PORT_TASK_STACK_ALLOC_EXTERNAL` must stay UNSET (internal).
   PSRAM task stacks + any NVS/flash write = cache-disabled stack assert
   (`esp_task_stack_is_sane_cache_disabled`). This was a 20-version ghost.
3. FreeRTOS stream buffers created STATIC need storage of size+1 (kernel adds
   the slack byte only in its own alloc path — see stream_buffer.c:374). The
   voice ring allocates VRING_SIZE+8 for this reason.
4. `esp_http_client_perform()` CONSUMES the response body. All request code
   uses open→write→fetch_headers→read-loop. Never "fix" it back.
5. The mic record stream is 2-slot interleaved (`channel:2`). svc_audio
   auto-detects the live slot and compacts to true mono 16k. Don't treat raw
   i2s reads as mono.
6. Voice ring teardown is single-owner via `vring_owners` refcount +
   `voice_release()`. Never free `vring` anywhere else (double-free race).
7. UI objects: screens are destroyed on navigation. Widget pointers are
   nulled in DELETE callbacks; cross-thread UI text goes through the
   pet_say pending-queue, never direct lv_* calls from workers.
8. NVS writes from the LVGL task are legal ONLY because of (2). Keep it so.
9. Codec cannot serve mic and non-16k playback simultaneously:
   playback path calls mic_wait_stopped() first; listen quiesces mic on send.

## Architecture map
- `main/screens/scr_idle.c` — the creature: clip engine (frames+FX overlays),
  pet_react() 19-emotion table, brain_cb (100ms) = behavior scheduler:
  speech choreography (emotion intro→visemes by voice_level), presence engine
  (user_touch-based; self-talk 1-2.5min when engaged; nap at 20min; panic on
  wake after 45min sleep), outreach ritual (greet→25s→uneasy→25s→standby;
  AI replies must NOT count as user touch or wake — pend_ai flag),
  action verbs from the model (camera/listen/sleep) executed post-speech.
- `main/services/svc_audio.c` — mic task (slot-compacting) + record buffer;
  voice: dl task → 96KB PSRAM stream-ring → play task (RMS→voice_level at
  playout). Wi-Fi PS_NONE during stream, MIN_MODEM after.
- `main/services/svc_assistant.c` — the mind contract. think: POST
  {event,text,vitals,persona,senses{screen,person,ip,log-tail}} →
  {say,emotion,glitch,audio,action}. converse: raw PCM16 body + context
  query-string. Brain URL in NVS ("lumen"/"brain_url"), set via /brain.
- `main/services/svc_mirror.c` — http: / (mirror UI), /shot.bmp, /log
  (+ pre-crash `was:` trail), /info, /brain, /ctl (nav/react/say/reboot).
- `main/services/svc_petlog.c` — ring log + RTC_NOINIT black box. petlog()
  EVERY behavioral decision; that discipline is why bugs die here.
- `main/pet/` — 54 sprite assets (regenerate via ~/tamagotchi pipeline if
  ever needed), pet_persona.c (6 drifting stats).
- Server contract changes = edit think_server.py only; firmware stays dumb.

## OPEN BUGS (your first missions)
1. CRASH: Listen → tap send → "thinking" → reboots within seconds,
   intermittently (sometimes completes fine). Protocol: run
   `python3 yasserctl.py follow` in one terminal, reproduce until one crash,
   read the `was:` trail it prints after reconnect. Prior eliminations:
   stack size (fixed 8192), dangling widgets (queued), self-talk collision
   (gated home-only), mic concurrency (quiesced on send). Suspect space:
   converse upload path, response handling, or another UI-stall flavor
   (trail will say `!! UI STALL` if watchdog class).
2. Wi-Fi sometimes fails to connect at boot (creds are in NVS and were
   working). Check svc_wifi retry/backoff logic and serial `svc_wifi:` lines.
3. Voice occasionally cuts at sentence end (distinct from crashes) — verify
   ring drain logic sends the final partial chunk.

## Working style
- One mechanism-named fix per flash. petlog new behaviors. Bump version.
- Verify each fix ON DEVICE via yasserctl (shot/log/say/react are your senses
  and hands). Ask the human to physically poke/speak only when unavoidable.
- After any crash: read the black box FIRST (`yasserctl.py log`, top lines).
- Never commit guesses: if the trail doesn't name it, add instrumentation
  (petlog) and reproduce again.
