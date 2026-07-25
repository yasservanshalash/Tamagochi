#include "router.h"

static const char *SCR_NAMES[] = { "home", "menu", "camera", "listen",
                                   "response", "settings", "about" };
static int cur_scr = 0;

const char *router_screen_name(void)
{
    return SCR_NAMES[cur_scr < 7 ? cur_scr : 0];
}
#include "theme.h"
#include "scr_alert.h"
#include "theme.h"
#include "sensecap-watcher.h"
#include <stdatomic.h>

static const scr_create_fn factories[SCR_COUNT] = {
    [SCR_IDLE]     = scr_idle_create,
    [SCR_MENU]     = scr_menu_create,
    [SCR_LISTEN]   = scr_listen_create,
    [SCR_RESPONSE] = scr_response_create,
    [SCR_CAMERA]   = scr_camera_create,
    [SCR_SETTINGS] = scr_settings_create,
    [SCR_SETUP]    = scr_setup_create,
};

static lumen_screen_t stack[8];
static int            sp = 0;            /* stack[0] is always SCR_IDLE */
static lv_group_t    *grp = NULL;
static lv_indev_t    *encoder = NULL;
static atomic_bool    back_pending = false;

static void load(lumen_screen_t id, bool back_anim)
{
    lv_group_remove_all_objs(grp);
    lv_obj_t *scr = factories[id]();
    lv_scr_load_anim(scr,
                     back_anim ? LV_SCR_LOAD_ANIM_FADE_OUT : LV_SCR_LOAD_ANIM_FADE_IN,
                     120, 0, true /* delete old screen */);
}

/* BSP long-press callback runs in the button task — never touch LVGL here.
 * Set a flag; a 30 ms lv_timer in the LVGL task performs the navigation. */
static void on_long_press(void)
{
    atomic_store(&back_pending, true);
}

static void back_poll_cb(lv_timer_t *t)
{
    (void)t;
    if (atomic_exchange(&back_pending, false)) {
        router_back();
    }
}

void router_init(lv_disp_t *disp)
{
    (void)disp;
    grp = lv_group_create();
    lv_group_set_default(grp);

    /* bind the knob encoder indev (registered by bsp_lvgl_init) */
    lv_indev_t *i = NULL;
    while ((i = lv_indev_get_next(i)) != NULL) {
        if (lv_indev_get_type(i) == LV_INDEV_TYPE_ENCODER) {
            encoder = i;
            lv_indev_set_group(i, grp);
        } else if (lv_indev_get_type(i) == LV_INDEV_TYPE_POINTER) {
            /* small round screen: finger wobble past 10px cancels taps in
             * scrollable lists ("can't interact" in Settings). 24px keeps
             * scrolling responsive while making taps land reliably.      */
            i->driver->scroll_limit = 24;
        }
    }

    bsp_set_btn_long_press_cb(on_long_press);
    lv_timer_create(back_poll_cb, 30, NULL);

    stack[0] = SCR_IDLE;
    sp = 0;
    load(SCR_IDLE, false);
}

lv_group_t *router_group(void) { return grp; }


void router_do_shutdown(void) { bsp_system_shutdown(); }

/* ---- universal back affordances ---- */
static void back_click_cb(lv_event_t *e) { (void)e; router_back(); }

static void back_gesture_cb(lv_event_t *e)
{
    if (lv_indev_get_gesture_dir(lv_indev_get_act()) == LV_DIR_RIGHT)
        router_back();
}

lv_obj_t *router_attach_back(lv_obj_t *scr)
{
    lv_obj_add_event_cb(scr, back_gesture_cb, LV_EVENT_GESTURE, NULL);

    /* ghost chevron: bare 4px round-cap stroke (icon spec), 80px hit
     * area, chrome appears only on press / wheel focus                 */
    lv_obj_t *btn = lv_btn_create(scr);
    lv_obj_set_size(btn, 80, 80);
    lv_obj_align(btn, LV_ALIGN_CENTER, -112, -112);   /* inside safe circle */
    lv_obj_set_style_radius(btn, LV_RADIUS_CIRCLE, 0);
    lv_obj_set_style_bg_opa(btn, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(btn, 0, 0);
    lv_obj_set_style_shadow_width(btn, 0, 0);
    lv_obj_set_style_bg_color(btn, C_SURFACE, LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_STATE_PRESSED);
    lv_obj_set_style_bg_color(btn, C_SURFACE, LV_STATE_FOCUS_KEY);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_STATE_FOCUS_KEY);
    lv_obj_set_style_bg_color(btn, C_SURFACE, LV_STATE_FOCUSED);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_STATE_FOCUSED);

    static const lv_point_t chev[] = { {14, 0}, {0, 14}, {14, 28} };
    lv_obj_t *ln = lv_line_create(btn);
    lv_line_set_points(ln, chev, 3);
    lv_obj_set_style_line_width(ln, 4, 0);
    lv_obj_set_style_line_rounded(ln, true, 0);
    lv_obj_set_style_line_color(ln, C_TEXT_LO, 0);
    lv_obj_center(ln);
    lv_obj_clear_flag(ln, LV_OBJ_FLAG_CLICKABLE);
    /* accent stroke while the button is active */
    lv_obj_set_style_line_color(ln, C_ACCENT, LV_STATE_PRESSED);

    lv_obj_add_event_cb(btn, back_click_cb, LV_EVENT_CLICKED, NULL);
    return btn;
}

void router_go(lumen_screen_t id)
{
    if (sp >= (int)(sizeof(stack) / sizeof(stack[0])) - 1) return;
    stack[++sp] = id;
    load(id, false);
}

lumen_screen_t router_current(void) { return stack[sp]; }

void router_back(void)
{
    /* "hold to dismiss" — an open alert consumes the long-press */
    if (alert_visible()) {
        alert_hide();
        return;
    }
    if (sp == 0) {
        /* long-press on idle: power off (also fully resets the Himax —
         * the recovery path for any wedge). Confirmed via overlay.      */
        alert_show_confirm("Power off?", "press the knob to boot again",
                           "Power off", router_do_shutdown);
        return;
    }
    sp--;
    load(stack[sp], true);
}

void router_home(void)
{
    sp = 0;
    load(SCR_IDLE, true);
}
