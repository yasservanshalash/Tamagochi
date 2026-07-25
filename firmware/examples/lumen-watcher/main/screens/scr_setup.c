/* WI-FI SETUP — captive portal. The QR is a standard Wi-Fi join code
 * (any camera app understands it): scanning joins the open hotspot,
 * the setup page then pops up automatically on the phone.             */
#include "router.h"
#include "theme.h"
#include <string.h>

lv_obj_t *scr_setup_create(void)
{
    lv_obj_t *scr = theme_screen_create();
    theme_title(scr, "SETUP");

    if (g_state.provisioning && g_state.prov_qr_payload[0]) {
        lv_obj_t *card = lv_obj_create(scr);
        lv_obj_set_size(card, 232, 232);
        lv_obj_align(card, LV_ALIGN_CENTER, 0, -8);
        lv_obj_set_style_radius(card, 24, 0);
        lv_obj_set_style_bg_color(card, C_TEXT_HI, 0);
        lv_obj_set_style_bg_opa(card, LV_OPA_COVER, 0);
        lv_obj_set_style_border_width(card, 0, 0);
        lv_obj_clear_flag(card, LV_OBJ_FLAG_SCROLLABLE);

#if LV_USE_QRCODE
        lv_obj_t *qr = lv_qrcode_create(card, 200, C_BG, C_TEXT_HI);
        lv_qrcode_update(qr, g_state.prov_qr_payload,
                         strlen(g_state.prov_qr_payload));
        lv_obj_center(qr);
#endif
        lv_obj_t *hint = lv_label_create(scr);
        lv_obj_add_style(hint, &st_label, 0);
        lv_label_set_text(hint, "scan Â· setup page pops up");
        lv_obj_align(hint, LV_ALIGN_BOTTOM_MID, 0, -66);
    } else {
        lv_obj_t *state = lv_label_create(scr);
        lv_obj_add_style(state, &st_body, 0);
        lv_obj_set_style_text_align(state, LV_TEXT_ALIGN_CENTER, 0);
        lv_label_set_text_fmt(state,
                              g_state.wifi_connected
                                  ? "Connected to\n%s\n\n%s"
                                  : "Connecting to\n%s",
                              g_state.ssid[0] ? g_state.ssid : "saved network",
                              g_state.ip);
        lv_obj_align(state, LV_ALIGN_CENTER, 0, -4);
    }

    lv_obj_t *name = lv_label_create(scr);
    lv_obj_add_style(name, &st_data, 0);
    lv_obj_set_style_text_color(name, C_TEXT_LO, 0);
    lv_label_set_text(name, g_state.device_name);
    lv_obj_align(name, LV_ALIGN_BOTTOM_MID, 0, -32);

    lv_obj_t *back = router_attach_back(scr);
    lv_group_add_obj(router_group(), back);
    lv_group_add_obj(router_group(), scr);
    return scr;
}
