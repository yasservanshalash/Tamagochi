/* RESPONSE — centered 280 px column; scroll position mirrored to the
 * 79-deg rim arc (right). Wheel or swipe scrolls; press = done -> idle. */
#include "router.h"
#include "theme.h"
#include "ui_arcs.h"
#include <stdlib.h>

/* black->transparent alpha strip (TRUE_COLOR_ALPHA), generated once */
static lv_obj_t *fade_strip(lv_obj_t *parent, int w, int h, bool top)
{
    static uint8_t *buf_top = NULL, *buf_bot = NULL;
    int px = LV_COLOR_SIZE / 8 + 1;                 /* color + alpha byte */
    uint8_t **bufp = top ? &buf_top : &buf_bot;
    if (!*bufp) {
        *bufp = lv_mem_alloc(w * h * px);
        for (int y = 0; y < h; y++) {
            uint8_t a = top ? 255 - (y * 255 / (h - 1))
                            : (y * 255 / (h - 1));
            for (int x = 0; x < w; x++) {
                uint8_t *p = *bufp + (y * w + x) * px;
                p[0] = 0x00; p[1] = 0x00;           /* black RGB565       */
                p[2] = a;
            }
        }
    }
    static lv_img_dsc_t dsc_top, dsc_bot;
    lv_img_dsc_t *d = top ? &dsc_top : &dsc_bot;
    d->header.always_zero = 0;
    d->header.cf = LV_IMG_CF_TRUE_COLOR_ALPHA;
    d->header.w = w;
    d->header.h = h;
    d->data_size = w * h * px;
    d->data = *bufp;
    lv_obj_t *img = lv_img_create(parent);
    lv_img_set_src(img, d);
    lv_obj_clear_flag(img, LV_OBJ_FLAG_CLICKABLE);
    return img;
}

static lv_obj_t *arc_scroll;

static void scroll_cb(lv_event_t *e)
{
    lv_obj_t *cont = lv_event_get_target(e);
    lv_coord_t max = lv_obj_get_scroll_bottom(cont) + lv_obj_get_scroll_y(cont);
    if (max <= 0) { rim_arc_set(arc_scroll, 100); return; }
    rim_arc_set(arc_scroll, lv_obj_get_scroll_y(cont) * 100 / max);
}

static void key_cb(lv_event_t *e)
{
    lv_obj_t *cont = lv_event_get_target(e);
    uint32_t key = lv_event_get_key(e);
    if (key == LV_KEY_RIGHT || key == LV_KEY_DOWN)
        lv_obj_scroll_by(cont, 0, -48, LV_ANIM_ON);
    else if (key == LV_KEY_LEFT || key == LV_KEY_UP)
        lv_obj_scroll_by(cont, 0, 48, LV_ANIM_ON);
}

static void done_cb(lv_event_t *e)
{
    (void)e;
    router_home();
}

lv_obj_t *scr_response_create(void)
{
    lv_obj_t *scr = theme_screen_create();

    theme_title(scr, "ASSISTANT");

    arc_scroll = rim_arc_create(scr, ARC_SCROLL_SWEEP, 0, C_ACCENT);
    rim_arc_set(arc_scroll, 0);

    lv_obj_t *cont = lv_obj_create(scr);
    lv_obj_set_size(cont, 280, 250);
    lv_obj_align(cont, LV_ALIGN_CENTER, 0, 8);
    lv_obj_add_style(cont, &st_screen, 0);
    lv_obj_set_style_bg_opa(cont, LV_OPA_TRANSP, 0);
    lv_obj_add_flag(cont, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_scroll_dir(cont, LV_DIR_VER);
    lv_obj_set_scrollbar_mode(cont, LV_SCROLLBAR_MODE_OFF);

    lv_obj_t *txt = lv_label_create(cont);
    lv_obj_add_style(txt, &st_body, 0);
    lv_obj_set_width(txt, 280);
    lv_label_set_text(txt, lumen_get_response_text());

    /* edge fades over the scroll column (design detail restored) */
    lv_obj_t *ftop = fade_strip(scr, 280, 26, true);
    lv_obj_align_to(ftop, cont, LV_ALIGN_OUT_TOP_MID, 0, 26);
    lv_obj_t *fbot = fade_strip(scr, 280, 26, false);
    lv_obj_align_to(fbot, cont, LV_ALIGN_OUT_BOTTOM_MID, 0, -26);

    lv_obj_t *hint = lv_label_create(scr);
    lv_obj_add_style(hint, &st_label, 0);
    lv_obj_set_style_opa(hint, LV_OPA_60, 0);
    lv_label_set_text(hint, "SCROLL");
    lv_obj_align(hint, LV_ALIGN_BOTTOM_MID, 0, -28);

    lv_obj_add_event_cb(cont, scroll_cb, LV_EVENT_SCROLL, NULL);
    lv_obj_add_event_cb(cont, key_cb, LV_EVENT_KEY, NULL);
    lv_obj_add_event_cb(cont, done_cb, LV_EVENT_CLICKED, NULL);

    lv_obj_t *back = router_attach_back(scr);
    lv_group_add_obj(router_group(), back);
    lv_group_add_obj(router_group(), cont);
    lv_group_focus_obj(cont);
    lv_group_set_editing(router_group(), true);   /* wheel -> KEY events */
    return scr;
}
