#!/usr/bin/env bash
# Flash the PREBUILT Lumen binaries — no ESP-IDF, no build, just esptool.
# Writes bootloader + partition table + otadata (points boot at ota_0)
# + the app. Never touches nvsfactory (0x9000) or saved settings.
set -euo pipefail
PORT="${1:-/dev/ttyACM1}"
cd "$(dirname "$0")/prebuilt"

esptool --chip esp32s3 -p "$PORT" -b 2000000 \
  --before default_reset --after hard_reset write-flash \
  --flash_mode dio --flash_size 32MB --flash_freq 80m \
  0x0       bootloader.bin \
  0x8000    partition-table.bin \
  0x10d000  ota_data_initial.bin \
  0x110000  lumen-watcher.bin

echo ">>> done. 'idf.py monitor' (or any 115200 serial monitor) should show: Lumen v0.7.0"
