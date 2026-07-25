/* MENU — radial, circular-native.
 * Wheel: rotation walks the ring; focused slot gets an accent ring and the
 *        36-deg rim focus arc slides to its angle; name enlarges at center.
 * Touch: pressed slot fills (88 px finger target); release opens.        */
#include "router.h"
#include "theme.h"
#include "ui_arcs.h"
#include <math.h>

typedef struct {
    const char    *name;
    lumen_screen_t target;
    int            angle_deg;   /* LVGL convention: 0 = 3 o'clock, cw    */
} slot_def_t;

static const slot_def_t slots[] = {
    { "LISTEN",   SCR_LISTEN,   -90 },
    { "CAMERA",   SCR_CAMERA,    30 },
    { "SETTINGS", SCR_SETTINGS, 150 },
};
#define NSLOTS (sizeof(slots) / sizeof(slots[0]))
#define RING_R 132

static lv_obj_t *lb_center, *arc_focus;

/* ---- tiny stroke icons (4 px, round caps, no fills) ---- */
static lv_obj_t *bar(lv_obj_t *p, int x, int h)
{
    lv_obj_t *b = lv_obj_create(p);
    lv_obj_set_size(b, 4, h);
    lv_obj_set_style_radius(b, 2, 0);
    lv_obj_set_style_border_width(b, 0, 0);
    lv_obj_set_style_bg_color(b, C_TEXT_HI, 0);
    lv_obj_set_style_bg_opa(b, LV_OPA_COVER, 0);
    lv_obj_align(b, LV_ALIGN_CENTER, x, 0);
    return b;
}

static void icon_for(lv_obj_t *slot_btn, int idx)
{
    if (idx == 0) {                       /* LISTEN: waveform bars       */
        bar(slot_btn, -15, 16); bar(slot_btn, -5, 32);
        bar(slot_btn, 5, 44);   bar(slot_btn, 15, 24);
    } else if (idx == 1) {                /* CAMERA: lens rings          */
        for (int r = 0; r < 2; r++) {
            lv_obj_t *c = lv_obj_create(slot_btn);
            int d = r == 0 ? 44 : 18;
            lv_obj_set_size(c, d, d);
            lv_obj_set_style_radius(c, LV_RADIUS_CIRCLE, 0);
            lv_obj_set_style_bg_opa(c, LV_OPA_TRANSP, 0);
            lv_obj_set_style_border_width(c, 4, 0);
            lv_obj_set_style_border_color(c, C_TEXT_HI, 0);
            lv_obj_center(c);
        }
    } else {                              /* SETTINGS: slider strokes    */
        for (int r = 0; r < 3; r++) {
            lv_obj_t *l = lv_obj_create(slot_btn);
            lv_obj_set_size(l, 40, 4);
            lv_obj_set_style_radius(l, 2, 0);
            lv_obj_set_style_border_width(l, 0, 0);
            lv_obj_set_style_bg_color(l, C_TEXT_HI, 0);
            lv_obj_set_style_bg_opa(l, LV_OPA_COVER, 0);
            lv_obj_align(l, LV_ALIGN_CENTER, 0, (r - 1) * 14);
            lv_obj_t *d = lv_obj_create(l);
            lv_obj_set_size(d, 10, 10);
            lv_obj_set_style_radius(d, LV_RADIUS_CIRCLE, 0);
            lv_obj_set_style_border_width(d, 0, 0);
            lv_obj_set_style_bg_color(d, C_TEXT_HI, 0);
            lv_obj_set_style_bg_opa(d, LV_OPA_COVER, 0);
            lv_obj_align(d, LV_ALIGN_CENTER, (r - 1) * 10, 0);
        }
    }
    /* icons are decorative — keep touches on the slot button itself */
    uint32_t n = lv_obj_get_child_cnt(slot_btn);
    for (uint32_t i = 0; i < n; i++) {
        lv_obj_t *ch = lv_obj_get_child(slot_btn, i);
        lv_obj_clear_flag(ch, LV_OBJ_FLAG_CLICKABLE | LV_OBJ_FLAG_SCROLLABLE);
    }
}

static void slot_size_exec(void *var, int32_t v)
{
    lv_obj_t *o = (lv_obj_t *)var;
    lv_obj_set_size(o, v, v);
    /* keep the ring position: re-align from stored index */
    int idx = (int)(intptr_t)lv_obj_get_user_data(o);
    float a = slots[idx].angle_deg * (float)M_PI / 180.0f;
    lv_obj_align(o, LV_ALIGN_CENTER,
                 (lv_coord_t)(RING_R * cosf(a)),
                 (lv_coord_t)(RING_R * sinf(a)));
}

static void slot_anim_to(lv_obj_t *b, int target)
{
    lv_anim_t a;
    lv_anim_init(&a);
    lv_anim_set_var(&a, b);
    lv_anim_set_exec_cb(&a, slot_size_exec);
    lv_anim_set_values(&a, lv_obj_get_width(b), target);
    lv_anim_set_time(&a, 140);
    lv_anim_set_path_cb(&a, lv_anim_path_ease_out);
    lv_anim_start(&a);
}

static void defocus_cb(lv_event_t *e)
{
    slot_anim_to(lv_event_get_target(e), SLOT_D);
}

static void fade_exec(void *var, int32_t v)
{
    lv_obj_set_style_opa((lv_obj_t *)var, (lv_opa_t)v, 0);
}

static void focus_cb(lv_event_t *e)
{
    int idx = (int)(intptr_t)lv_event_get_user_data(e);
    lv_label_set_text(lb_center, slots[idx].name);
    lv_arc_set_rotation(arc_focus,
                        rim_norm_deg(slots[idx].angle_deg - ARC_FOCUS_SWEEP / 2));
    lv_anim_t a;
    lv_anim_init(&a);
    lv_anim_set_var(&a, lb_center);
    lv_anim_set_exec_cb(&a, fade_exec);
    lv_anim_set_values(&a, LV_OPA_30, LV_OPA_COVER);
    lv_anim_set_time(&a, 160);
    lv_anim_start(&a);
    slot_anim_to(lv_event_get_target(e), SLOT_D + 14);
}

static void click_cb(lv_event_t *e)
{
    int idx = (int)(intptr_t)lv_event_get_user_data(e);
    router_go(slots[idx].target);
}

lv_obj_t *scr_menu_create(void)
{
    lv_obj_t *scr = theme_screen_create();

    arc_focus = rim_arc_create(scr, ARC_FOCUS_SWEEP,
                               slots[0].angle_deg, C_ACCENT);

    lb_center = lv_label_create(scr);
    lv_obj_add_style(lb_center, &st_title, 0);
    lv_label_set_text(lb_center, slots[0].name);
    lv_obj_align(lb_center, LV_ALIGN_CENTER, 0, -6);

    lv_obj_t *hint = lv_label_create(scr);
    lv_obj_add_style(hint, &st_label, 0);
    lv_label_set_text(hint, "press to open");
    lv_obj_align(hint, LV_ALIGN_CENTER, 0, 34);

    for (int i = 0; i < (int)NSLOTS; i++) {
        lv_obj_t *b = lv_btn_create(scr);
        lv_obj_set_size(b, SLOT_D, SLOT_D);
        float a = slots[i].angle_deg * (float)M_PI / 180.0f;
        lv_obj_align(b, LV_ALIGN_CENTER,
                     (lv_coord_t)(RING_R * cosf(a)),
                     (lv_coord_t)(RING_R * sinf(a)));
        lv_obj_set_style_radius(b, LV_RADIUS_CIRCLE, 0);
        lv_obj_set_style_bg_color(b, C_BG, 0);
        lv_obj_set_style_bg_opa(b, LV_OPA_COVER, 0);
        lv_obj_set_style_border_width(b, 2, 0);
        lv_obj_set_style_border_color(b, C_LINE, 0);
        lv_obj_set_style_shadow_width(b, 0, 0);
        /* touch: pressed slot fills (design 02b) */
        lv_obj_set_style_bg_color(b, C_SURFACE, LV_STATE_PRESSED);
        lv_obj_set_style_border_color(b, C_ACCENT, LV_STATE_PRESSED);
        /* wheel: focused slot ringed (design 02a) */
        lv_obj_set_style_border_color(b, C_ACCENT, LV_STATE_FOCUS_KEY);
        lv_obj_set_style_border_color(b, C_ACCENT, LV_STATE_FOCUSED);

        icon_for(b, i);
        lv_obj_set_user_data(b, (void *)(intptr_t)i);
        lv_obj_add_event_cb(b, focus_cb, LV_EVENT_FOCUSED, (void *)(intptr_t)i);
        lv_obj_add_event_cb(b, defocus_cb, LV_EVENT_DEFOCUSED, NULL);
        lv_obj_add_event_cb(b, click_cb, LV_EVENT_CLICKED, (void *)(intptr_t)i);
        lv_group_add_obj(router_group(), b);
    }

    lv_obj_t *back = router_attach_back(scr);       /* swipe/tap -> idle */
    lv_group_add_obj(router_group(), back);
    return scr;
}
