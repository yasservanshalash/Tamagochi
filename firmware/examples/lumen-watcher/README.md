# Lumen v0.7.1 — minimal firmware for the SenseCAP Watcher

**v0.7:** COMPILED AND VERIFIED — this zip ships prebuilt binaries in
`prebuilt/`; flash with `./flash_prebuilt.sh PORT` using only esptool (no
ESP-IDF needed). Also: codec init made thread-safe (a race between the
boot task and an early Sound tap could crash), and the portal's password
copy fixed for -Werror builds.

**v0.6:** wrong-password resilience — disconnect reasons are logged, and
after 5 consecutive failures the device stops looping and opens the setup
portal by itself (bad credentials can never strand it again); the portal
gained a show-password toggle and prefills the failing SSID; the flash
script now BUILDS before flashing and prints the version, so a failed
build can never silently flash stale firmware again (which is exactly
what the logs show happened); boot banner prints "Lumen vX.Y.Z".

**v0.5:** live screen mirror at `http://<watcher-ip>/` (auto-refreshing,
right-click to save screenshots) and `/shot.bmp` for stills; portal
hardened — networks are scanned BEFORE the hotspot exists so loading the
page can never kick your phone off, manual SSID entry added, failed
passwords no longer retry-loop (which was destabilising the hotspot);
codec initialises at boot so Sound works on the first tap; every settings
action now logs to serial for diagnosis.

**v0.4:** the Espressif provisioning app is GONE — the Watcher is now a
captive portal (join `watcher-XXXX` or scan the standard Wi-Fi QR with any
camera; the themed setup page pops up on the phone: pick network, type
password, done). Sound row now initialises the codec on demand and beeps
at the new volume; Bluetooth row explains itself instead of looking
broken; screen titles truly centered (bad nudge removed); pulse rings
even-sized (no center wobble); focused menu slot enlarges per the design;
response edge fades restored via alpha-gradient strips.

**v0.3:** provisioning fixed at the root (missing AP netif — the app was
connecting to a hotspot with no DHCP), optional compile-time Wi-Fi
credentials in `lumen.h` to skip the app entirely, Reset Wi-Fi now behind a
confirmation overlay, crash-safe alert dismissal (async delete), ghost
chevron back button matching the 4px-stroke icon spec, unified title
alignment on every screen, JPEG decode pinned to CPU0 (LVGL owns CPU1),
15ms display refresh.

**v0.2:** live camera viewfinder (Himax JPEG stream decoded on-device),
mic-reactive Listen rings, visible back button + swipe-right-to-go-back on
every screen, Reset Wi-Fi row (long-press), charging pulse on the battery
arc, and all build fixes discovered in the field (CMake 4 policy, BSP's
missing public deps, the display SPI/DMA config that stock silently pins).

Instrument-dark, circular-native, zero cloud accounts. Built from your
Claude Design mockups on top of Seeed's open BSP; everything else from the
stock factory firmware is gone.

```
boot -> idle watch face          (clock · date · battery rim arc · wifi glyph)
tap / press wheel -> radial menu (LISTEN · CAMERA · SETTINGS)
rotate = focus · press = select · long-press = back / dismiss
long-press on idle = display off
```

## ⚠ First: fix your nvsfactory backup

The backup you made earlier used the wrong address (my mistake — `0x1e0000`
is inside the app partition). The factory credentials partition is at
**`0x9000`, 200 KiB**. Your device's factory data is still intact on flash
(nothing we wrote overlapped it), so just re-run the backup properly:

```bash
esptool --chip esp32s3 -p /dev/ttyACM1 --no-stub \
  read-flash 0x9000 204800 nvsfactory.bin
```

Keep that file safe. It is the only thing on the device you cannot
regenerate (EUI + SenseCraft server credentials).

## Build

Lumen lives inside the Seeed SDK so the BSP resolves as a local component:

```bash
# 1. SDK + toolchain (once)
git clone https://github.com/Seeed-Studio/SenseCAP-Watcher-Firmware
# install ESP-IDF v5.2.1+ per Espressif's docs, then:  . $IDF_PATH/export.sh

# 2. drop Lumen in as an example
cp -r lumen-watcher SenseCAP-Watcher-Firmware/examples/
cd SenseCAP-Watcher-Firmware/examples/lumen-watcher

# 3. build + flash
idf.py set-target esp32s3
idf.py build
./flash_lumen.sh /dev/ttyACM1        # or: idf.py -p /dev/ttyACM1 flash
```

`./restore_stock.sh /dev/ttyACM1` puts Seeed V1.1.7 back any time —
the partition table is byte-identical to stock, so both firmwares are
mutually flash-compatible and neither disturbs `nvsfactory`.
**Never run `idf.py erase-flash`** — that is the one command that would
wipe your factory credentials.

## What was removed vs stock (and what it buys)

| Stock subsystem | Cost in stock | Lumen |
|---|---|---|
| SquareLine UI + PNG assets | 27 MB flash, PNG decode, PSRAM cache | vector-only LVGL, ~200 KB fonts |
| esp-sr wake-word engine | continuous AFE on a core, srmodels partition | gone; push-to-talk via wheel |
| SenseCraft cloud (MQTT/TLS) | background handshakes, account required | gone; local provisioning |
| BLE stack + AT protocol | ~100 KB RAM resident, radio time | gone |
| Task-flow JSON engine | ~7 k lines, cJSON churn | gone |
| iperf (yes, really) | shipped in factory image | gone |
| 14-subsystem init before UI | slow, chatty boot | display-only boot, net async |
| `-Og` default build | slower LVGL render | `-O2` (`COMPILER_OPTIMIZATION_PERF`) |

## What works out of the box

- **Idle** — RTC-seeded clock (SNTP refines it), battery arc, wifi glyph.
- **Menu** — radial, both wheel-focus and touch-pressed states from the mockups.
- **Listen → Response** — full flow with the correct worker-task pattern;
  demo transcript/answer until you point `LUMEN_ASSISTANT_URL`
  (`services/svc_assistant.h`) at a TinkerBox route that accepts
  `{"q":"…"}` → `{"a":"…"}`.
- **Camera** — LIVE viewfinder: the Himax streams JPEG frames which are
  base64+JPEG decoded on the ESP32 into the circular view, with detection
  chip and confidence arc. Same pipeline stock uses, minus the cloud.
- **Alerts** — person ≥ 0.88 while you're elsewhere pops the overlay
  (30 s cooldown). View → camera; hold to dismiss.
- **Settings** — brightness applies live and persists; Wi-Fi row → setup.
- **Setup** — QR provisions Wi-Fi via the offline **ESP SoftAP Prov** app
  (Play Store / App Store, by Espressif). No Seeed account, ever.

## Honest caveats (v0.1)

- **Not yet compiled** — written against LVGL 8.4 + the BSP headers in this
  SDK, but I couldn't run ESP-IDF here. Expect the usual first-build round
  of include/`sdkconfig` nits, not architectural problems.
- **Speech-to-text** is the remaining stub: the mic is live (rings react
  to your voice) but transcription needs a backend — stream PCM to a
  TinkerBox/whisper route and set LUMEN_ASSISTANT_URL.
- Response-column edge fades from the mockup are omitted (LVGL 8 has no
  opacity-gradient style; LVGL 9 does).

## Layout

```
main/
├─ main.c            boot: NVS → BSP → LVGL → idle, net async
├─ theme.{h,c}       design tokens 1:1 (colors, fonts, arcs, targets)
├─ router.{h,c}      screen stack, encoder group, long-press = back
├─ ui_arcs.{h,c}     rim arcs, wifi glyph, alert ring
├─ fonts/            Space Grotesk 104/34/30 · IBM Plex Mono 24/20 (4bpp)
├─ screens/          idle · menu · listen · response · camera · settings · setup · alert
└─ services/         state/NVS · wifi+SNTP+provisioning · assistant hook · sscma camera
```
