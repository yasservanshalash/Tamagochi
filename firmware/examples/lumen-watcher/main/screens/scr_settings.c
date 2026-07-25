/* SETTINGS — wheel-scrollable rows adapted to the circle.
 * Rows: Wi-Fi (ssid / setup) · Sound · Display · Bluetooth (off) · About. */
#include "router.h"
#include "theme.h"
#include "sensecap-watcher.h"
#include "svc_state.h"
#include "scr_alert.h"
#include "svc_audio.h"
#include "svc_wifi.h"
#include "svc_ble.h"
#include <stdio.h>
#include <string.h>
#include "esp_log.h"

enum { ROW_WIFI, ROW_SOUND, ROW_DISPLAY, ROW_BT, ROW_ABOUT, ROW_RESET, ROW_N };
static lv_obj_t *val_lbl[ROW_N];
static lv_obj_t *row_obj[ROW_N];
static int edit_row = -1;                 /* wheel edit mode target      */

static void set_edit_style(int r, bool on)
{
    lv_obj_set_style_text_color(val_lbl[r], on ? C_ACCENT : C_TEXT_LO, 0);
    lv_obj_set_style_border_color(row_obj[r], on ? C_ACCENT : C_LINE,
                                  LV_STATE_FOCUS_KEY);
    lv_obj_set_style_border_color(row_obj[r], on ? C_ACCENT : C_LINE,
                                  LV_STATE_FOCUSED);
}

static void edit_exit(void)
{
    if (edit_row < 0) return;
    if (edit_row == ROW_SOUND) svc_audio_beep();  /* hear the new level */
    set_edit_style(edit_row, false);
    edit_row = -1;
    lv_group_set_editing(router_group(), false);
    svc_state_save_settings();
}

static void set_val(int r, const char *v) { lv_label_set_text(val_lbl[r], v); }

static void refresh_values(void)
{
    char buf[36];
    if (g_state.wifi_connected)      set_val(ROW_WIFI, g_state.ssid);
    else if (g_state.provisioning)   set_val(ROW_WIFI, "setup");
    else                             set_val(ROW_WIFI, "...");
    snprintf(buf, sizeof(buf), "%u%%", g_state.volume);     set_val(ROW_SOUND, buf);
    snprintf(buf, sizeof(buf), "%u%%", g_state.brightness); set_val(ROW_DISPLAY, buf);
    if (g_state.bt_connected)     set_val(ROW_BT, "linked");
    else if (g_state.bt_enabled)  set_val(ROW_BT, "on");
    else                          set_val(ROW_BT, "off");
    set_val(ROW_ABOUT, "v" LUMEN_VERSION);
    set_val(ROW_RESET, "");
}

static void do_wifi_reset(void) { svc_wifi_reset(); }

static void row_key(lv_event_t *e)
{
    int r = (int)(intptr_t)lv_event_get_user_data(e);
    if (edit_row != r) return;
    uint32_t k = lv_event_get_key(e);
    int d = (k == LV_KEY_RIGHT || k == LV_KEY_DOWN) ? 1 :
            (k == LV_KEY_LEFT  || k == LV_KEY_UP)  ? -1 : 0;
    if (!d) return;
    if (r == ROW_SOUND) {
        int v = (int)g_state.volume + d * 5;
        g_state.volume = v < 0 ? 0 : (v > 100 ? 100 : v);
        svc_audio_apply_volume();
    } else {
        int v = (int)g_state.brightness + d * 10;
        g_state.brightness = v < 10 ? 10 : (v > 100 ? 100 : v);
        bsp_lcd_brightness_set(g_state.brightness);   /* live */
    }
    refresh_values();
}

static void row_click(lv_event_t *e)
{
    int r = (int)(intptr_t)lv_event_get_user_data(e);
    ESP_LOGI("settings", "row %d activated (vol=%u bright=%u wifi=%d)",
             r, g_state.volume, g_state.brightness, g_state.wifi_connected);
    bool from_wheel =
        lv_indev_get_act() &&
        lv_indev_get_type(lv_indev_get_act()) == LV_INDEV_TYPE_ENCODER;

    /* wheel press on Sound/Display toggles fine-adjust mode */
    if (from_wheel && (r == ROW_SOUND || r == ROW_DISPLAY)) {
        if (edit_row == r) { edit_exit(); }
        else {
            edit_exit();
            edit_row = r;
            set_edit_style(r, true);
            lv_group_set_editing(router_group(), true);
        }
        refresh_values();
        return;
    }
    if (edit_row >= 0) edit_exit();

    switch (r) {
    case ROW_WIFI:
        router_go(SCR_SETUP);
        return;
    case ROW_SOUND:
        g_state.volume = (g_state.volume + 20) % 120;        /* 0..100 step 20 */
        ESP_LOGI("settings", "sound: apply");
        svc_audio_apply_volume();
        ESP_LOGI("settings", "sound: beep");
        svc_audio_beep();
        ESP_LOGI("settings", "sound: save");
        svc_state_save_settings();
        ESP_LOGI("settings", "sound: done");
        break;
    case ROW_DISPLAY: {
        static const uint8_t steps[] = { 40, 60, 80, 100 };
        int i = 0;
        while (i < 3 && steps[i] <= g_state.brightness) i++;
        g_state.brightness = steps[g_state.brightness >= 100 ? 0 : i];
        ESP_LOGI("settings", "display: set %u", g_state.brightness);
        bsp_lcd_brightness_set(g_state.brightness);
        ESP_LOGI("settings", "display: save");
        svc_state_save_settings();
        ESP_LOGI("settings", "display: done");
        break;
    }
    case ROW_BT:
        svc_ble_set_enabled(!g_state.bt_enabled);
        break;
    case ROW_RESET:
        alert_show_confirm("Reset Wi-Fi?",
                           "wipes credentials \xC2\xB7 reboots to setup",
                           "Reset", do_wifi_reset);
        return;   /* confirm overlay owns the flow now */
    case ROW_ABOUT: {
        static char info[72];
        snprintf(info, sizeof(info), "%s \xC2\xB7 %s \xC2\xB7 rst:%s",
                 g_state.device_name,
                 g_state.wifi_connected ? g_state.ip : "offline",
                 g_state.last_reset);
        alert_show("Lumen " LUMEN_VERSION, info);
        break;
    }
    }
    refresh_values();
}

lv_obj_t *scr_settings_create(void)
{
    static const char *names[ROW_N] =
        { "Wi-Fi", "Sound", "Display", "Bluetooth", "About", "Reset Wi-Fi" };

    lv_obj_t *scr = theme_screen_create();

    lv_obj_t *list = lv_obj_create(scr);
    lv_obj_set_size(list, 340, SCREEN_D);
    lv_obj_center(list);
    lv_obj_add_style(list, &st_screen, 0);
    lv_obj_set_style_bg_opa(list, LV_OPA_TRANSP, 0);
    lv_obj_set_flex_flow(list, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(list, LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_top(list, 120, 0);       /* first row lands in   */
    lv_obj_set_style_pad_bottom(list, 120, 0);    /* the circular safe zone */
    lv_obj_set_style_pad_row(list, 6, 0);
    lv_obj_set_scrollbar_mode(list, LV_SCROLLBAR_MODE_OFF);

    for (int r = 0; r < ROW_N; r++) {
        lv_obj_t *row = lv_btn_create(list);
        lv_obj_set_size(row, 320, ROW_H);
        lv_obj_set_style_radius(row, 40, 0);              /* pill r40 */
        lv_obj_set_style_bg_color(row, C_BG, 0);
        lv_obj_set_style_bg_opa(row, LV_OPA_COVER, 0);
        lv_obj_set_style_border_width(row, 2, 0);
        lv_obj_set_style_border_color(row, C_BG, 0);
        lv_obj_set_style_shadow_width(row, 0, 0);
        lv_obj_set_style_bg_color(row, C_SURFACE, LV_STATE_PRESSED);
        lv_obj_set_style_bg_color(row, C_SURFACE, LV_STATE_FOCUS_KEY);
        lv_obj_set_style_border_color(row, C_LINE, LV_STATE_FOCUS_KEY);
        lv_obj_set_style_bg_color(row, C_SURFACE, LV_STATE_FOCUSED);
        lv_obj_set_style_border_color(row, C_LINE, LV_STATE_FOCUSED);

        lv_obj_t *n = lv_label_create(row);
        lv_obj_add_style(n, &st_title, 0);
        lv_label_set_text(n, names[r]);
        lv_obj_align(n, LV_ALIGN_LEFT_MID, 18, 0);

        val_lbl[r] = lv_label_create(row);
        lv_obj_add_style(val_lbl[r], &st_data, 0);
        lv_obj_set_style_text_color(val_lbl[r], C_TEXT_LO, 0);
        lv_obj_align(val_lbl[r], LV_ALIGN_RIGHT_MID, -18, 0);

        row_obj[r] = row;
        lv_obj_add_event_cb(row, row_click, LV_EVENT_CLICKED, (void *)(intptr_t)r);
        lv_obj_add_event_cb(row, row_key, LV_EVENT_KEY, (void *)(intptr_t)r);
        lv_group_add_obj(router_group(), row);
    }

    lv_obj_t *back = router_attach_back(scr);
    lv_group_add_obj(router_group(), back);
    edit_row = -1;
    refresh_values();
    return scr;
}
