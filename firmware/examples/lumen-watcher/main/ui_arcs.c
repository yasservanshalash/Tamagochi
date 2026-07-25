#include "ui_arcs.h"
#include "theme.h"

/* lv_arc_set_rotation takes uint16: negative values wrap and land the arc
 * in the wrong place (the "indicator not in place" bug on every screen). */
int rim_norm_deg(int d) { return ((d % 360) + 360) % 360; }
#define norm_deg rim_norm_deg

lv_obj_t *rim_arc_create(lv_obj_t *parent, int sweep, int center_deg, lv_color_t col)
{
    lv_obj_t *arc = lv_arc_create(parent);
    lv_obj_set_size(arc, ARC_R * 2 + ARC_W, ARC_R * 2 + ARC_W);
    lv_obj_center(arc);
    lv_obj_clear_flag(arc, LV_OBJ_FLAG_CLICKABLE);
    lv_arc_set_mode(arc, LV_ARC_MODE_NORMAL);
    lv_arc_set_bg_angles(arc, 0, sweep);
    lv_arc_set_rotation(arc, norm_deg(center_deg - sweep / 2));
    lv_arc_set_range(arc, 0, 100);
    lv_arc_set_value(arc, 100);

    lv_obj_remove_style(arc, NULL, LV_PART_KNOB);
    lv_obj_set_style_arc_width(arc, ARC_W, LV_PART_MAIN);
    lv_obj_set_style_arc_color(arc, C_TRACK, LV_PART_MAIN);
    lv_obj_set_style_arc_rounded(arc, true, LV_PART_MAIN);
    lv_obj_set_style_arc_width(arc, ARC_W, LV_PART_INDICATOR);
    lv_obj_set_style_arc_color(arc, col, LV_PART_INDICATOR);
    lv_obj_set_style_arc_rounded(arc, true, LV_PART_INDICATOR);
    return arc;
}

void rim_arc_set(lv_obj_t *arc, int pct)
{
    if (pct < 0) pct = 0;
    if (pct > 100) pct = 100;
    lv_arc_set_value(arc, pct);
}

/* ---- wifi glyph: three 90-deg arcs opening upward + a dot ---- */
lv_obj_t *icon_wifi_create(lv_obj_t *parent)
{
    lv_obj_t *box = lv_obj_create(parent);
    lv_obj_set_size(box, 48, 40);
    lv_obj_add_style(box, &st_screen, 0);
    lv_obj_set_style_bg_opa(box, LV_OPA_TRANSP, 0);
    lv_obj_clear_flag(box, LV_OBJ_FLAG_SCROLLABLE | LV_OBJ_FLAG_CLICKABLE);

    static const int radii[3] = { 10, 17, 24 };
    for (int i = 0; i < 3; i++) {
        lv_obj_t *a = lv_arc_create(box);
        lv_obj_set_size(a, radii[i] * 2, radii[i] * 2);
        lv_obj_align(a, LV_ALIGN_BOTTOM_MID, 0, radii[i] - 4);
        lv_obj_clear_flag(a, LV_OBJ_FLAG_CLICKABLE);
        lv_arc_set_bg_angles(a, 225, 315);          /* opens upward   */
        lv_arc_set_value(a, 0);
        lv_obj_remove_style(a, NULL, LV_PART_KNOB);
        lv_obj_remove_style(a, NULL, LV_PART_INDICATOR);
        lv_obj_set_style_arc_width(a, 4, LV_PART_MAIN);
        lv_obj_set_style_arc_rounded(a, true, LV_PART_MAIN);
        lv_obj_set_style_arc_color(a, C_TEXT_LO, LV_PART_MAIN);
    }
    lv_obj_t *dot = lv_obj_create(box);
    lv_obj_set_size(dot, 6, 6);
    lv_obj_set_style_radius(dot, LV_RADIUS_CIRCLE, 0);
    lv_obj_set_style_border_width(dot, 0, 0);
    lv_obj_set_style_bg_color(dot, C_TEXT_LO, 0);
    lv_obj_set_style_bg_opa(dot, LV_OPA_COVER, 0);
    lv_obj_align(dot, LV_ALIGN_BOTTOM_MID, 0, -2);
    return box;
}

void icon_wifi_set_state(lv_obj_t *icon, bool connected)
{
    lv_color_t c = connected ? C_ACCENT : C_TEXT_LO;
    lv_opa_t   o = connected ? LV_OPA_COVER : LV_OPA_50;
    uint32_t n = lv_obj_get_child_cnt(icon);
    for (uint32_t i = 0; i < n; i++) {
        lv_obj_t *ch = lv_obj_get_child(icon, i);
        lv_obj_set_style_arc_color(ch, c, LV_PART_MAIN);
        lv_obj_set_style_bg_color(ch, c, 0);
        lv_obj_set_style_opa(ch, o, 0);
    }
}

/* ---- rotating alert ring ---- */
static void ring_rot_anim(void *obj, int32_t v)
{
    lv_arc_set_rotation((lv_obj_t *)obj, v);
}

lv_obj_t *alert_ring_create(lv_obj_t *parent)
{
    /* base: full ring 30% opa */
    lv_obj_t *base = lv_arc_create(parent);
    lv_obj_set_size(base, ARC_R * 2 + ALERT_RING_W, ARC_R * 2 + ALERT_RING_W);
    lv_obj_center(base);
    lv_obj_clear_flag(base, LV_OBJ_FLAG_CLICKABLE);
    lv_arc_set_bg_angles(base, 0, 360);
    lv_obj_remove_style(base, NULL, LV_PART_KNOB);
    lv_obj_remove_style(base, NULL, LV_PART_INDICATOR);
    lv_obj_set_style_arc_width(base, ALERT_RING_W, LV_PART_MAIN);
    lv_obj_set_style_arc_color(base, C_ACCENT, LV_PART_MAIN);
    lv_obj_set_style_arc_opa(base, LV_OPA_30, LV_PART_MAIN);

    /* sweep: 65 deg accent segment rotating */
    lv_obj_t *sweep = lv_arc_create(parent);
    lv_obj_set_size(sweep, ARC_R * 2 + ALERT_RING_W, ARC_R * 2 + ALERT_RING_W);
    lv_obj_center(sweep);
    lv_obj_clear_flag(sweep, LV_OBJ_FLAG_CLICKABLE);
    lv_arc_set_bg_angles(sweep, 0, ALERT_SWEEP);
    lv_obj_remove_style(sweep, NULL, LV_PART_KNOB);
    lv_obj_remove_style(sweep, NULL, LV_PART_INDICATOR);
    lv_obj_set_style_arc_width(sweep, ALERT_RING_W, LV_PART_MAIN);
    lv_obj_set_style_arc_color(sweep, C_ACCENT, LV_PART_MAIN);
    lv_obj_set_style_arc_rounded(sweep, true, LV_PART_MAIN);

    lv_anim_t a;
    lv_anim_init(&a);
    lv_anim_set_var(&a, sweep);
    lv_anim_set_exec_cb(&a, ring_rot_anim);
    lv_anim_set_values(&a, 0, 360);
    lv_anim_set_time(&a, 2400);
    lv_anim_set_repeat_count(&a, LV_ANIM_REPEAT_INFINITE);
    lv_anim_start(&a);
    return base;
}
