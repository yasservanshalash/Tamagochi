/*
 * Lumen — minimal instrument-dark firmware for Seeed SenseCAP Watcher
 * Screens, app-wide state and events.
 */
#pragma once

#include <stdbool.h>
#include <stdint.h>
#include <time.h>

#define LUMEN_VERSION      "0.53.1"
#define LUMEN_DEVICE_PREFIX "watcher"   /* provisioning SSID / QR name prefix */

/* Timezone for the watch face. Examples:
 *   Gulf (UAE):        "GST-4"
 *   Netherlands:       "CET-1CEST,M3.5.0,M10.5.0/3"
 *   UTC:               "UTC0"
 */
#define LUMEN_TZ "GST-4"

/* Optional: bake Wi-Fi credentials in at compile time and skip the
 * provisioning app entirely. Uncomment and rebuild:                     */
#define LUMEN_WIFI_SSID "KPND991D6"   /* dual-band; ESP32-S3 uses the 2.4GHz side (ch 1) */
#define LUMEN_WIFI_PASS "NhJpZCC3xpkJh9Xv"

typedef enum {
    SCR_IDLE = 0,
    SCR_MENU,
    SCR_LISTEN,
    SCR_RESPONSE,
    SCR_CAMERA,
    SCR_SETTINGS,
    SCR_SETUP,
    SCR_COUNT
} lumen_screen_t;

/* ---- shared observable state (single writer per field, UI reads) ---- */
typedef struct {
    /* power */
    uint8_t  battery_pct;
    bool     charging;
    /* network */
    bool     wifi_connected;
    bool     provisioning;          /* SoftAP provisioning active        */
    char     ssid[33];
    char     ip[16];
    char     device_name[24];       /* "watcher-7F2C"                    */
    char     prov_qr_payload[128];  /* JSON for ESP SoftAP Prov app      */
    bool     time_valid;            /* SNTP or RTC has real time         */
    /* camera */
    bool     cam_ready;
    bool     cam_preview_on;        /* camera screen wants frames        */
    char     cam_label[24];         /* "PERSON"                          */
    uint8_t  cam_score;             /* 0..100                            */
    uint16_t cam_x, cam_y, cam_w, cam_h;  /* best box, 240px frame space */
    /* audio */
    uint8_t  mic_level;             /* live mic RMS 0..100               */
    bool     voice_playing;         /* TTS stream active                 */
    uint8_t  voice_level;           /* live playback RMS 0..100          */
    /* diagnostics */
    char     last_reset[20];        /* human reset reason at boot        */
    /* bluetooth */
    bool     bt_enabled;
    bool     bt_connected;
    /* settings */
    uint8_t  brightness;            /* 10..100                           */
    uint8_t  volume;                /* 0..100                            */
} lumen_state_t;

extern lumen_state_t g_state;

/* response screen payload (set before navigating to SCR_RESPONSE) */
void lumen_set_response_text(const char *txt);
const char *lumen_get_response_text(void);
