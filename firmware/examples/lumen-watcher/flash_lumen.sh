#!/usr/bin/env bash
# Build + flash Lumen. Building here is deliberate: flashing a stale
# binary after a failed build is how "fixed" firmware stays broken.
set -euo pipefail
PORT="${1:-/dev/ttyACM1}"

idf.py build   # abort on any failure — never flash stale bits

V=$(grep -oP 'LUMEN_VERSION\s+"\K[0-9.]+' main/lumen.h)
echo ">>> flashing Lumen v${V} to ${PORT}"

esptool --chip esp32s3 -p "$PORT" -b 2000000 \
  --before default_reset --after hard_reset write-flash \
  --flash_mode dio --flash_size 32MB --flash_freq 80m \
  0x0       build/bootloader/bootloader.bin \
  0x8000    build/partition_table/partition-table.bin \
  0x110000  build/lumen-watcher.bin

echo ">>> done. Boot log should read: 'Lumen v${V}'"
