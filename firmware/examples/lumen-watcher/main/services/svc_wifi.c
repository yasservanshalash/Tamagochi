#include "svc_wifi.h"
#include "svc_portal.h"
#include "svc_mirror.h"
#include "lumen.h"
#include "sensecap-watcher.h"

#include "esp_wifi.h"
#include "esp_event.h"
#include "esp_netif.h"
#include "esp_sntp.h"
#include "esp_log.h"
#include "esp_mac.h"
#include "freertos/FreeRTOS.h"
#include "freertos/timers.h"
#include <string.h>
#include <stdio.h>
#include <time.h>

static const char *TAG = "svc_wifi";

static void sntp_synced(struct timeval *tv)
{
    (void)tv;
    g_state.time_valid = true;
    time_t now = time(NULL);
    struct tm ti;
    localtime_r(&now, &ti);
    bsp_rtc_set_time(&ti);
    ESP_LOGI(TAG, "time synced");
}

static void start_sntp(void)
{
    if (esp_sntp_enabled()) return;
    esp_sntp_setoperatingmode(SNTP_OPMODE_POLL);
    esp_sntp_setservername(0, "pool.ntp.org");
    sntp_set_time_sync_notification_cb(sntp_synced);
    esp_sntp_init();
}

static void portal_teardown_cb(TimerHandle_t t)
{
    (void)t;
    svc_portal_stop();          /* connected & settled: drop the hotspot */
}

static int fail_count = 0;
#define FAIL_FALLBACK 5     /* consecutive failures before opening the portal */

static void evt(void *arg, esp_event_base_t base, int32_t id, void *data)
{
    if (base == WIFI_EVENT && id == WIFI_EVENT_STA_DISCONNECTED) {
        wifi_event_sta_disconnected_t *d = (wifi_event_sta_disconnected_t *)data;
        g_state.wifi_connected = false;
        ESP_LOGW(TAG, "disconnected from %.32s, reason=%d (15=wrong password)",
                 (char *)d->ssid, d->reason);
        if (g_state.provisioning)
            return;         /* portal owns retries: user resubmits        */
        if (++fail_count >= FAIL_FALLBACK) {
            /* stored credentials keep failing (typically a bad password):
             * stop looping and open the captive portal so the device can
             * never be stranded by bad creds.                            */
            ESP_LOGW(TAG, "%d failures -> opening setup portal", fail_count);
            fail_count = 0;
            esp_wifi_disconnect();
            svc_portal_start();
            return;
        }
        esp_wifi_connect();
    } else if (base == IP_EVENT && id == IP_EVENT_STA_GOT_IP) {
        ip_event_got_ip_t *e = (ip_event_got_ip_t *)data;
        snprintf(g_state.ip, sizeof(g_state.ip), IPSTR, IP2STR(&e->ip_info.ip));
        wifi_config_t wc;
        if (esp_wifi_get_config(WIFI_IF_STA, &wc) == ESP_OK)
            strncpy(g_state.ssid, (char *)wc.sta.ssid, sizeof(g_state.ssid) - 1);
        g_state.wifi_connected = true;
        fail_count = 0;
        start_sntp();
        if (!g_state.provisioning)
            svc_mirror_start();      /* portal still owns :80 otherwise */
        if (g_state.provisioning) {
            /* give the phone time to load the success page, then drop AP */
            static TimerHandle_t t = NULL;
            if (!t) t = xTimerCreate("portal_dn", pdMS_TO_TICKS(8000),
                                     pdFALSE, NULL, portal_teardown_cb);
            xTimerStart(t, 0);
        }
    }
}

void svc_wifi_start(void)
{
    setenv("TZ", LUMEN_TZ, 1);
    tzset();

    struct tm ti = { 0 };
    if (bsp_rtc_get_time(&ti) == ESP_OK && ti.tm_year > 100) {
        time_t t = mktime(&ti);
        struct timeval tv = { .tv_sec = t };
        settimeofday(&tv, NULL);
        g_state.time_valid = true;
    }

    ESP_ERROR_CHECK(esp_netif_init());
    ESP_ERROR_CHECK(esp_event_loop_create_default());
    esp_netif_create_default_wifi_sta();
    esp_netif_create_default_wifi_ap();       /* portal AP + DHCP       */
    wifi_init_config_t cfg = WIFI_INIT_CONFIG_DEFAULT();
    ESP_ERROR_CHECK(esp_wifi_init(&cfg));

    esp_event_handler_register(WIFI_EVENT, ESP_EVENT_ANY_ID, evt, NULL);
    esp_event_handler_register(IP_EVENT, IP_EVENT_STA_GOT_IP, evt, NULL);

    uint8_t mac[6];
    esp_read_mac(mac, ESP_MAC_WIFI_STA);
    snprintf(g_state.device_name, sizeof(g_state.device_name),
             LUMEN_DEVICE_PREFIX "-%02X%02X", mac[4], mac[5]);

#if defined(LUMEN_WIFI_SSID) && defined(LUMEN_WIFI_PASS)
    wifi_config_t fixed = { 0 };
    strncpy((char *)fixed.sta.ssid, LUMEN_WIFI_SSID, sizeof(fixed.sta.ssid));
    strncpy((char *)fixed.sta.password, LUMEN_WIFI_PASS, sizeof(fixed.sta.password));
    ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_STA));
    ESP_ERROR_CHECK(esp_wifi_set_config(WIFI_IF_STA, &fixed));
    ESP_ERROR_CHECK(esp_wifi_start());
    esp_wifi_connect();
    ESP_LOGI(TAG, "compile-time credentials: %s", LUMEN_WIFI_SSID);
    return;
#endif

    /* stored credentials? (esp_wifi keeps its config in NVS) */
    ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_STA));
    ESP_ERROR_CHECK(esp_wifi_set_storage(WIFI_STORAGE_FLASH));
    wifi_config_t wc = { 0 };
    esp_wifi_get_config(WIFI_IF_STA, &wc);

    if (wc.sta.ssid[0] != 0) {
        ESP_ERROR_CHECK(esp_wifi_start());
        esp_wifi_connect();
        ESP_LOGI(TAG, "stored credentials: %s", wc.sta.ssid);
    } else {
        svc_portal_start();     /* no creds -> captive portal            */
    }
}

void svc_wifi_reset(void)
{
    esp_wifi_restore();
    bsp_system_reboot();
}
