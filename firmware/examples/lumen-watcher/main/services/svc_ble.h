#pragma once
#include <stdbool.h>
/* BLE (NimBLE GATT server) — the Watcher as a peripheral:
 *   service  0xFF00 "Lumen"
 *     0xFF01 status     (read)   "v0.15.0|batt=93|wifi=1|ip=..."
 *     0xFF02 detection  (notify) "PERSON|92" on alert-level detections
 *     0xFF03 command    (write)  0x01 = beep
 * ESP32-S3 is BLE-only (no classic BT / audio). Toggled from Settings,
 * persisted in NVS, advertises as the device name.                      */
void svc_ble_init(void);                    /* start if enabled in NVS   */
void svc_ble_set_enabled(bool on);          /* toggle + persist          */
void svc_ble_notify_detection(const char *label, int score);
