#!/usr/bin/env bash
# Return to Seeed stock firmware. Point STOCK at the OSHW repo's
# ESP32/V1.1.7 directory (you already cloned it).
set -euo pipefail

PORT="${1:-/dev/ttyACM1}"
STOCK="${2:-$HOME/OSHW-SenseCAP-Watcher/Firmware/ESP32/V1.1.7}"

esptool --chip esp32s3 -p "$PORT" -b 2000000 \
  --before default_reset --after hard_reset write-flash \
  --flash_mode dio --flash_size 32MB --flash_freq 80m \
  0x0        "$STOCK/bootloader/bootloader.bin" \
  0x8000     "$STOCK/partition_table/partition-table.bin" \
  0x10d000   "$STOCK/ota_data_initial.bin" \
  0x110000   "$STOCK/factory_firmware.bin" \
  0x1910000  "$STOCK/srmodels/srmodels.bin" \
  0x1a10000  "$STOCK/storage.bin"

echo "Stock V1.1.7 restored."
