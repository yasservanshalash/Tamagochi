#include "svc_ble.h"
#include "lumen.h"
#include "svc_audio.h"
#include "pet.h"
#include "esp_log.h"
#include "nvs.h"
#include "nimble/nimble_port.h"
#include "nimble/nimble_port_freertos.h"
#include "host/ble_hs.h"
#include "host/util/util.h"
#include "services/gap/ble_svc_gap.h"
#include "services/gatt/ble_svc_gatt.h"
#include <string.h>
#include <stdio.h>

static const char *TAG = "svc_ble";
static bool running = false;
static uint16_t conn_handle = BLE_HS_CONN_HANDLE_NONE;
static uint16_t detect_val_handle = 0;
static uint32_t last_notify_ms = 0;

static void start_advertise(void);

/* ---- GATT ---- */
static int status_access(uint16_t ch, uint16_t ah,
                         struct ble_gatt_access_ctxt *ctxt, void *arg)
{
    char buf[96];
    snprintf(buf, sizeof(buf), "v%s|batt=%u|wifi=%d|ip=%s",
             LUMEN_VERSION, g_state.battery_pct,
             g_state.wifi_connected, g_state.ip);
    return os_mbuf_append(ctxt->om, buf, strlen(buf)) == 0
               ? 0 : BLE_ATT_ERR_INSUFFICIENT_RES;
}

static int command_access(uint16_t ch, uint16_t ah,
                          struct ble_gatt_access_ctxt *ctxt, void *arg)
{
    uint8_t cmd = 0;
    if (os_mbuf_copydata(ctxt->om, 0, 1, &cmd) == 0) {
        switch (cmd) {
        case 0x01: svc_audio_beep();               break;
        case 0x02: pet_react("happy", 0, 0);       break;
        case 0x03: pet_react("glitch", 100, 0);    break;
        case 0x04: pet_react("talk", 0, 3000);     break;
        case 0x05: pet_react("busy", 0, 0);        break;
        case 0x06: pet_react("dance", 0, 0);       break;
        case 0x07: pet_react("think", 0, 0);       break;
        case 0x08: pet_react("confused", 0, 0);    break;
        case 0x09: pet_react("celebrate", 0, 0);   break;
        case 0x0A: pet_react("jump", 0, 0);        break;
        case 0x0B: pet_react("sad", 0, 0);         break;
        case 0x0C: pet_react("scared", 0, 0);      break;
        case 0x0D: pet_react("laugh", 0, 0);       break;
        case 0x0E: pet_react("suspicious", 0, 0);  break;
        case 0x0F: pet_react("whisper", 0, 0);     break;
        case 0x10: pet_react("point", 0, 0);       break;
        case 0x11: pet_react("facepalm", 0, 0);    break;
        case 0x12: pet_react("stretch", 0, 0);     break;
        case 0x13: pet_react("grumpy", 0, 0);      break;
        }
    }
    return 0;
}

static int detect_access(uint16_t ch, uint16_t ah,
                         struct ble_gatt_access_ctxt *ctxt, void *arg)
{
    return 0;   /* notify-only */
}

static const struct ble_gatt_svc_def gatt_svcs[] = {
    {
        .type = BLE_GATT_SVC_TYPE_PRIMARY,
        .uuid = BLE_UUID16_DECLARE(0xFF00),
        .characteristics = (struct ble_gatt_chr_def[]){
            { .uuid = BLE_UUID16_DECLARE(0xFF01),
              .access_cb = status_access,
              .flags = BLE_GATT_CHR_F_READ },
            { .uuid = BLE_UUID16_DECLARE(0xFF02),
              .access_cb = detect_access,
              .val_handle = &detect_val_handle,
              .flags = BLE_GATT_CHR_F_NOTIFY },
            { .uuid = BLE_UUID16_DECLARE(0xFF03),
              .access_cb = command_access,
              .flags = BLE_GATT_CHR_F_WRITE_NO_RSP },
            { 0 }
        },
    },
    { 0 }
};

/* ---- GAP ---- */
static int gap_event(struct ble_gap_event *ev, void *arg)
{
    switch (ev->type) {
    case BLE_GAP_EVENT_CONNECT:
        if (ev->connect.status == 0) {
            conn_handle = ev->connect.conn_handle;
            g_state.bt_connected = true;
        } else start_advertise();
        break;
    case BLE_GAP_EVENT_DISCONNECT:
        conn_handle = BLE_HS_CONN_HANDLE_NONE;
        g_state.bt_connected = false;
        start_advertise();
        break;
    case BLE_GAP_EVENT_ADV_COMPLETE:
        start_advertise();
        break;
    }
    return 0;
}

static void start_advertise(void)
{
    struct ble_hs_adv_fields f = { 0 };
    f.flags = BLE_HS_ADV_F_DISC_GEN | BLE_HS_ADV_F_BREDR_UNSUP;
    f.name = (uint8_t *)g_state.device_name;
    f.name_len = strlen(g_state.device_name);
    f.name_is_complete = 1;
    ble_gap_adv_set_fields(&f);

    struct ble_gap_adv_params p = { 0 };
    p.conn_mode = BLE_GAP_CONN_MODE_UND;
    p.disc_mode = BLE_GAP_DISC_MODE_GEN;
    ble_gap_adv_start(BLE_OWN_ADDR_PUBLIC, NULL, BLE_HS_FOREVER,
                      &p, gap_event, NULL);
}

static void on_sync(void)
{
    ble_hs_util_ensure_addr(0);
    start_advertise();
    ESP_LOGI(TAG, "advertising as %s", g_state.device_name);
}

static void host_task(void *arg)
{
    nimble_port_run();
    nimble_port_freertos_deinit();
}

static void ble_start(void)
{
    if (running) return;
    if (nimble_port_init() != ESP_OK) {
        ESP_LOGE(TAG, "nimble init failed");
        return;
    }
    ble_hs_cfg.sync_cb = on_sync;
    ble_svc_gap_init();
    ble_svc_gatt_init();
    ble_svc_gap_device_name_set(g_state.device_name);
    ble_gatts_count_cfg(gatt_svcs);
    ble_gatts_add_svcs(gatt_svcs);
    nimble_port_freertos_init(host_task);
    running = true;
    g_state.bt_enabled = true;
}

static void ble_stop(void)
{
    if (!running) return;
    ble_gap_adv_stop();
    nimble_port_stop();
    nimble_port_deinit();
    running = false;
    g_state.bt_enabled = false;
    g_state.bt_connected = false;
}

void svc_ble_notify_detection(const char *label, int score)
{
    if (!running || conn_handle == BLE_HS_CONN_HANDLE_NONE) return;
    uint32_t now = xTaskGetTickCount() * portTICK_PERIOD_MS;
    if (now - last_notify_ms < 5000) return;      /* rate limit          */
    last_notify_ms = now;
    char msg[40];
    int n = snprintf(msg, sizeof(msg), "%s|%d", label, score);
    struct os_mbuf *om = ble_hs_mbuf_from_flat(msg, n);
    if (om) ble_gatts_notify_custom(conn_handle, detect_val_handle, om);
}

void svc_ble_set_enabled(bool on)
{
    if (on) ble_start(); else ble_stop();
    nvs_handle_t h;
    if (nvs_open("lumen", NVS_READWRITE, &h) == ESP_OK) {
        nvs_set_u8(h, "bt", on ? 1 : 0);
        nvs_commit(h);
        nvs_close(h);
    }
}

void svc_ble_init(void)
{
    /* BLE shares the single 2.4GHz radio with Wi-Fi: advertising steals
     * enough airtime on this link to starve voice streaming down to
     * syllable-level stutter. Stay OFF at boot regardless of saved
     * state — the Settings toggle still starts it on demand.           */
    g_state.bt_enabled = false;
}
