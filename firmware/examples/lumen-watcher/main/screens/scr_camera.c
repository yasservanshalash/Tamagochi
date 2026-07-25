/* CAMERA — LIVE viewfinder from the Himax (base64 JPEG -> RGB565 lv_img),
 * confidence arc (108 deg, top), detection chip, back affordances.      */
#include "router.h"
#include "theme.h"
#include "ui_arcs.h"
#include "svc_camera.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include <stdio.h>
#include <string.h>

static lv_obj_t *arc_conf, *lb_class, *lb_score, *lb_status, *img_view, *vf, *det_box;
static lv_img_dsc_t img_dsc;
static uint32_t shown_seq;

static void cam_start_task(void *arg)
{
    (void)arg;
    svc_camera_start();
    vTaskDelete(NULL);
}

static void refresh_cb(lv_timer_t *t)
{
    (void)t;
    if (!g_state.cam_ready) {
        lv_label_set_text(lb_status, "STARTING");
        return;
    }

    uint32_t seq = svc_camera_frame_seq();
    if (seq != shown_seq) {
        if (shown_seq == 0) {              /* first frame: bind the source */
            img_dsc.header.always_zero = 0;
            img_dsc.header.cf = LV_IMG_CF_TRUE_COLOR;
            img_dsc.header.w = 240;
            img_dsc.header.h = 240;
            img_dsc.data_size = 240 * 240 * 2;
            img_dsc.data = svc_camera_display_buf();
            lv_img_set_src(img_view, &img_dsc);
            lv_obj_clear_flag(img_view, LV_OBJ_FLAG_HIDDEN);
        }
        lv_obj_invalidate(img_view);       /* content changed under lock  */
        shown_seq = seq;
        lv_label_set_text(lb_status, "LIVE");
    }

    if (g_state.cam_score >= 40 && g_state.cam_w > 0) {
        int x = g_state.cam_x - g_state.cam_w / 2;   /* center -> top-left */
        int y = g_state.cam_y - g_state.cam_h / 2;
        if (x < 0) x = 0;
        if (y < 0) y = 0;
        lv_obj_set_pos(det_box, x + 2, y + 2);       /* vf border offset  */
        lv_obj_set_size(det_box, g_state.cam_w, g_state.cam_h);
        lv_obj_clear_flag(det_box, LV_OBJ_FLAG_HIDDEN);
    } else {
        lv_obj_add_flag(det_box, LV_OBJ_FLAG_HIDDEN);
    }
    lv_label_set_text(lb_class, g_state.cam_label);
    char s[8];
    snprintf(s, sizeof(s), "0.%02u", g_state.cam_score >= 100 ? 99 : g_state.cam_score);
    lv_label_set_text(lb_score, s);
    rim_arc_set(arc_conf, g_state.cam_score);
}

static void del_cb(lv_event_t *e)
{
    g_state.cam_preview_on = false;            /* stop decode work off-screen */
    lv_timer_del((lv_timer_t *)lv_event_get_user_data(e));
}

lv_obj_t *scr_camera_create(void)
{
    lv_obj_t *scr = theme_screen_create();
    shown_seq = 0;

    /* circular viewfinder clipping the 416px frame */
    vf = lv_obj_create(scr);
    lv_obj_set_size(vf, 244, 244);   /* 240px feed clipped circular */
    lv_obj_center(vf);
    lv_obj_set_style_radius(vf, LV_RADIUS_CIRCLE, 0);
    lv_obj_set_style_clip_corner(vf, true, 0);
    lv_obj_set_style_bg_color(vf, lv_color_hex(0x0B0A08), 0);
    lv_obj_set_style_bg_opa(vf, LV_OPA_COVER, 0);
    lv_obj_set_style_border_color(vf, C_LINE, 0);
    lv_obj_set_style_border_width(vf, 2, 0);
    lv_obj_set_style_pad_all(vf, 0, 0);
    lv_obj_clear_flag(vf, LV_OBJ_FLAG_SCROLLABLE | LV_OBJ_FLAG_CLICKABLE);

    img_view = lv_img_create(vf);
    lv_obj_center(img_view);
    lv_obj_add_flag(img_view, LV_OBJ_FLAG_HIDDEN);   /* until first frame */

    det_box = lv_obj_create(vf);
    lv_obj_set_style_bg_opa(det_box, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_color(det_box, C_ACCENT, 0);
    lv_obj_set_style_border_width(det_box, 3, 0);
    lv_obj_set_style_radius(det_box, 6, 0);
    lv_obj_add_flag(det_box, LV_OBJ_FLAG_HIDDEN);
    lv_obj_clear_flag(det_box, LV_OBJ_FLAG_SCROLLABLE | LV_OBJ_FLAG_CLICKABLE);

    lv_obj_t *feed = lv_label_create(vf);
    lv_obj_add_style(feed, &st_label, 0);
    lv_obj_set_style_opa(feed, LV_OPA_50, 0);
    lv_label_set_text(feed, "\xC2\xB7 live feed \xC2\xB7");
    lv_obj_center(feed);
    lv_obj_move_background(feed);

    arc_conf = rim_arc_create(scr, ARC_CONF_SWEEP, -90, C_ACCENT);
    rim_arc_set(arc_conf, 0);

    lb_status = theme_title(scr, "STARTING");

    lb_class = lv_label_create(scr);
    lv_obj_add_style(lb_class, &st_title, 0);
    lv_obj_align(lb_class, LV_ALIGN_BOTTOM_MID, -34, -48);
    lv_label_set_text(lb_class, "--");

    lb_score = lv_label_create(scr);
    lv_obj_add_style(lb_score, &st_data, 0);
    lv_obj_set_style_text_color(lb_score, C_ACCENT, 0);
    lv_obj_align(lb_score, LV_ALIGN_BOTTOM_MID, 62, -52);
    lv_label_set_text(lb_score, "0.00");

    lv_obj_t *back = router_attach_back(scr);
    lv_group_add_obj(router_group(), back);

    g_state.cam_preview_on = true;
    xTaskCreate(cam_start_task, "cam_start", 4096, NULL, 4, NULL);
    lv_timer_t *t = lv_timer_create(refresh_cb, 66, NULL);   /* ~15 fps UI */
    lv_obj_add_event_cb(scr, del_cb, LV_EVENT_DELETE, t);
    return scr;
}
