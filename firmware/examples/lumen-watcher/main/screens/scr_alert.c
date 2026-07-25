#include "scr_alert.h"
#include "theme.h"
#include "ui_arcs.h"
#include "router.h"

static lv_obj_t *overlay = NULL;
static void (*confirm_fn)(void) = NULL;

static void dismiss_cb(lv_event_t *e) { (void)e; alert_hide(); }

static bool is_view_alert = false;      /* alert_show -> View jumps to camera */

static void action_cb(lv_event_t *e)
{
    (void)e;
    void (*fn)(void) = confirm_fn;
    bool view = is_view_alert;
    alert_hide();
    if (fn) fn();
    else if (view) router_go(SCR_CAMERA);
}

static void build(const char *title, const char *sub,
                  const char *btn_text, void (*on_confirm)(void))
{
    if (overlay) return;
    confirm_fn = on_confirm;

    overlay = lv_obj_create(lv_layer_top());
    lv_obj_set_size(overlay, SCREEN_D, SCREEN_D);
    lv_obj_add_style(overlay, &st_screen, 0);
    lv_obj_set_style_bg_opa(overlay, LV_OPA_90, 0);
    lv_obj_clear_flag(overlay, LV_OBJ_FLAG_SCROLLABLE);

    alert_ring_create(overlay);

    lv_obj_t *t = lv_label_create(overlay);
    lv_obj_add_style(t, &st_title, 0);
    lv_label_set_text(t, title);
    lv_obj_align(t, LV_ALIGN_CENTER, 0, -60);

    lv_obj_t *s = lv_label_create(overlay);
    lv_obj_add_style(s, &st_label, 0);
    lv_label_set_text(s, sub);
    lv_obj_align(s, LV_ALIGN_CENTER, 0, -18);

    lv_obj_t *btn = lv_btn_create(overlay);
    lv_obj_set_size(btn, 180, ROW_H);
    lv_obj_align(btn, LV_ALIGN_CENTER, 0, 70);
    lv_obj_set_style_radius(btn, 40, 0);
    lv_obj_set_style_bg_color(btn, C_SURFACE, 0);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, 0);
    lv_obj_set_style_border_color(btn, C_ACCENT, 0);
    lv_obj_set_style_border_width(btn, 2, 0);
    lv_obj_set_style_shadow_width(btn, 0, 0);
    lv_obj_add_event_cb(btn, action_cb, LV_EVENT_CLICKED, NULL);

    lv_obj_t *bl = lv_label_create(btn);
    lv_obj_add_style(bl, &st_title, 0);
    lv_label_set_text(bl, btn_text);
    lv_obj_center(bl);

    lv_obj_t *hint = lv_label_create(overlay);
    lv_obj_add_style(hint, &st_label, 0);
    lv_label_set_text(hint, "tap outside to dismiss");
    lv_obj_align(hint, LV_ALIGN_BOTTOM_MID, 0, -26);

    lv_obj_add_flag(overlay, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(overlay, dismiss_cb, LV_EVENT_CLICKED, NULL);

    lv_group_add_obj(router_group(), btn);
    lv_group_focus_obj(btn);
}

void alert_show(const char *title, const char *sub)
{
    is_view_alert = true;
    build(title, sub, "View", NULL);
}

void alert_show_confirm(const char *title, const char *sub,
                        const char *btn_text, void (*on_confirm)(void))
{
    is_view_alert = false;
    build(title, sub, btn_text, on_confirm);
}

void alert_hide(void)
{
    if (!overlay) return;
    lv_obj_t *o = overlay;
    overlay = NULL;              /* re-entrancy guard first             */
    confirm_fn = NULL;
    lv_obj_del_async(o);         /* SAFE from within o's own event cb   */
}

bool alert_visible(void) { return overlay != NULL; }
