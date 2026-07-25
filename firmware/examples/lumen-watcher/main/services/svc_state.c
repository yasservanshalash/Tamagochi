#include "svc_state.h"
#include "sensecap-watcher.h"
#include "nvs_flash.h"
#include "nvs.h"
#include <string.h>

lumen_state_t g_state = {
    .battery_pct = 100,
    .brightness  = 80,
    .volume      = 60,
    .cam_label   = "--",
};

static char response_text[1024] =
    "Studio lights are off. Standby power dropped to 12 W.\n\n"
    "Camera on the bench is still recording - want me to stop it as well?";

void lumen_set_response_text(const char *txt)
{
    strncpy(response_text, txt, sizeof(response_text) - 1);
    response_text[sizeof(response_text) - 1] = '\0';
}
const char *lumen_get_response_text(void) { return response_text; }

static void battery_timer_cb(lv_timer_t *t)
{
    (void)t;
    g_state.battery_pct = bsp_battery_get_percent();
    g_state.charging    = bsp_system_is_charging();
}

void svc_state_init(void)
{
    nvs_handle_t h;
    if (nvs_open("lumen", NVS_READONLY, &h) == ESP_OK) {
        uint8_t v;
        if (nvs_get_u8(h, "bright", &v) == ESP_OK) g_state.brightness = v;
        if (nvs_get_u8(h, "vol",    &v) == ESP_OK) g_state.volume    = v;
        nvs_close(h);
    }
    battery_timer_cb(NULL);
    lv_timer_create(battery_timer_cb, 5000, NULL);
}

void svc_state_save_settings(void)
{
    nvs_handle_t h;
    if (nvs_open("lumen", NVS_READWRITE, &h) == ESP_OK) {
        nvs_set_u8(h, "bright", g_state.brightness);
        nvs_set_u8(h, "vol",    g_state.volume);
        nvs_commit(h);
        nvs_close(h);
    }
}
