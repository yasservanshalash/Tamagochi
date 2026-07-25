/* Screen router.
 * Input model (from the design):  rotate = focus/scroll,
 *                                 press  = select,
 *                                 long-press = back / dismiss.
 * The router owns the single lv_group_t bound to the knob encoder indev.
 * Screens are created on entry and destroyed on exit (low steady-state RAM).
 */
#pragma once
#include "lvgl.h"
#include "lumen.h"

void router_init(lv_disp_t *disp);
void router_go(lumen_screen_t id);       /* push                       */
void router_back(void);                  /* pop (no-op at idle)        */
void router_home(void);
const char *router_screen_name(void);                  /* unwind to idle             */

lv_group_t *router_group(void);          /* screens add focusables     */
lumen_screen_t router_current(void);     /* top of the nav stack       */

/* Attach the universal back affordances to a screen:
 *  - swipe right anywhere -> back
 *  - a visible 64px chevron button (top-left, inside the safe zone)
 * Every screen except idle calls this. Returns the button (or NULL). */
lv_obj_t *router_attach_back(lv_obj_t *scr);
void router_do_shutdown(void);

/* screen factory signature; each screen implements one */
typedef lv_obj_t *(*scr_create_fn)(void);

lv_obj_t *scr_idle_create(void);
lv_obj_t *scr_menu_create(void);
lv_obj_t *scr_listen_create(void);
lv_obj_t *scr_response_create(void);
lv_obj_t *scr_camera_create(void);
lv_obj_t *scr_settings_create(void);
lv_obj_t *scr_setup_create(void);
