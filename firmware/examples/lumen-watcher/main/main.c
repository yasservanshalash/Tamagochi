/*
 * Lumen — main entry.
 *
 * Boot philosophy vs stock: the display pipeline is the ONLY thing that
 * happens before the first frame. Stock factory firmware initialises
 * audio player/recorder, RGB, SenseCraft cloud, OTA, task-flow engine,
 * voice interaction, Wi-Fi, AT commands, BLE and sensors before the UI
 * is fully alive. Lumen: NVS -> BSP -> LVGL -> idle face, then Wi-Fi in
 * a background task. Camera (Himax) initialises lazily on first use.
 */
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "esp_log.h"
#include "esp_system.h"
#include "nvs_flash.h"
#include "sensecap-watcher.h"
#include <stdio.h>

#include "lumen.h"
#include "svc_petlog.h"
#include "theme.h"
#include "router.h"
#include "scr_alert.h"
#include "svc_state.h"
#include "svc_wifi.h"
#include "svc_audio.h"
#include "svc_ble.h"
#include "freertos/task.h"

static const char *TAG = "lumen";

/* -------- detection -> alert watcher (LVGL task context) -------- */
#define ALERT_THRESHOLD   88      /* score 0..100     */
#define ALERT_COOLDOWN_MS 30000

static void alert_watch_cb(lv_timer_t *t)
{
    (void)t;
    static uint32_t last = 0;
    if (!g_state.cam_ready) return;
    if (router_current() == SCR_CAMERA) return;          /* already watching */
    if (alert_visible()) return;
    if (g_state.cam_score < ALERT_THRESHOLD) return;
    uint32_t now = lv_tick_get();
    if (last && now - last < ALERT_COOLDOWN_MS) return;
    last = now;
    alert_show("Person detected", "camera \xC2\xB7 just now");
}

/* ---- UI-stall watchdog: if the LVGL task stops beating for 4 s, dump
 * every task's state to serial (the freeze fingerprint) and reboot.    */
static volatile uint32_t ui_beat = 0;
static void beat_cb(lv_timer_t *t) { (void)t; ui_beat++; }

static void stall_watch_task(void *arg)
{
    (void)arg;
    uint32_t last = 0;
    int      still = 0;
    for (;;) {
        vTaskDelay(pdMS_TO_TICKS(1000));
        if (ui_beat == last) {
            if (++still >= 4) {
                ESP_LOGE(TAG, "UI STALLED — task states follow");
                UBaseType_t n = uxTaskGetNumberOfTasks();
                TaskStatus_t *ts = malloc(n * sizeof(TaskStatus_t));
                if (ts) {
                    n = uxTaskGetSystemState(ts, n, NULL);
                    for (UBaseType_t i = 0; i < n; i++)
                        ESP_LOGE(TAG, "  %-16s state=%d prio=%u hwm=%lu",
                                 ts[i].pcTaskName, ts[i].eCurrentState,
                                 (unsigned)ts[i].uxCurrentPriority,
                                 (unsigned long)ts[i].usStackHighWaterMark);
                    free(ts);
                }
                petlog("!! UI STALL -> reboot (see serial for task table)");
    ESP_LOGE(TAG, "rebooting to recover");
                vTaskDelay(pdMS_TO_TICKS(200));
                esp_restart();
            }
        } else {
            last = ui_beat;
            still = 0;
        }
    }
}

static void net_task(void *arg)
{
    (void)arg;
    svc_wifi_start();
    vTaskDelete(NULL);
}

static void audio_boot_task(void *arg)
{
    (void)arg;
    svc_audio_apply_volume();      /* inits codec + applies saved volume */
    vTaskDelete(NULL);
}

void app_main(void)
{
    {
        static const char *rr[] = {
            [ESP_RST_UNKNOWN]="unknown", [ESP_RST_POWERON]="power-on",
            [ESP_RST_EXT]="ext", [ESP_RST_SW]="sw-reset",
            [ESP_RST_PANIC]="PANIC", [ESP_RST_INT_WDT]="int-wdt",
            [ESP_RST_TASK_WDT]="task-wdt", [ESP_RST_WDT]="wdt",
            [ESP_RST_DEEPSLEEP]="deepsleep", [ESP_RST_BROWNOUT]="brownout",
            [ESP_RST_SDIO]="sdio",
        };
        esp_reset_reason_t r = esp_reset_reason();
        const char *name = (r < sizeof(rr)/sizeof(rr[0]) && rr[r]) ? rr[r] : "?";
        snprintf(g_state.last_reset, sizeof(g_state.last_reset), "%s", name);
        ESP_LOGW(TAG, "last reset: %s", name);
    }

    /* NVS: 'nvs' partition only — 'nvsfactory' is never touched */
    esp_err_t err = nvs_flash_init();
    if (err == ESP_ERR_NVS_NO_FREE_PAGES || err == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        ESP_ERROR_CHECK(nvs_flash_erase());
        ESP_ERROR_CHECK(nvs_flash_init());
    }

    /* board bring-up: expander (knob button), RTC, display+touch+encoder */
    assert(bsp_io_expander_init() != NULL);
    bsp_rtc_init();
    bsp_rgb_init();
    bsp_rgb_set(0, 0, 0);                      /* LED dark by default */

    svc_audio_init();

    lv_disp_t *disp = bsp_lvgl_init();
    assert(disp != NULL);

    if (lvgl_port_lock(0)) {
        theme_init();
        svc_state_init();                      /* creates lv timers   */
        bsp_lcd_brightness_set(g_state.brightness);
        router_init(disp);                     /* -> idle face        */
        lv_timer_create(alert_watch_cb, 500, NULL);
        lv_timer_create(beat_cb, 250, NULL);
        lvgl_port_unlock();
    }
    ESP_LOGI(TAG, "Lumen v%s | UI up, free heap %lu",
             LUMEN_VERSION, esp_get_free_heap_size());
    petlog_boot_recover();

    /* everything network-flavoured happens after the first frame */
    xTaskCreatePinnedToCore(net_task, "lumen_net", 6144, NULL, 3, NULL, 0);
    xTaskCreatePinnedToCore(audio_boot_task, "lumen_aud", 4096, NULL, 3, NULL, 0);
    xTaskCreatePinnedToCore(stall_watch_task, "lumen_wdt", 4096, NULL, 22, NULL, 0);
    svc_ble_init();                        /* restores saved BLE state  */

    /* reserve every worker stack NOW, while internal heap is plentiful —
     * runtime task creation loses races against voice/Wi-Fi pressure    */
    extern void scr_listen_prewarm(void);
    scr_listen_prewarm();
    svc_audio_prewarm_voice();
}
