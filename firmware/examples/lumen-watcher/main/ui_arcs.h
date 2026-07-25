/* Rim arcs and stroke icons per the style guide.
 * LVGL angle convention: 0 deg = 3 o'clock, clockwise positive.
 * "@ 0 deg (right)" = centered at 3 o'clock; "@ -90 deg" = centered at top.
 */
#pragma once
#include "lvgl.h"

/* full-screen rim arc; sweep degrees, centered at center_deg.
 * returns lv_arc with range 0..100, track C_TRACK, indicator `col`. */
lv_obj_t *rim_arc_create(lv_obj_t *parent, int sweep, int center_deg, lv_color_t col);
int       rim_norm_deg(int d);   /* normalize to 0..359 for lv_arc rotation */
void      rim_arc_set(lv_obj_t *arc, int pct);

/* stroke-style Wi-Fi glyph (three nested arcs + dot), 48px grid, 4px stroke */
lv_obj_t *icon_wifi_create(lv_obj_t *parent);
void      icon_wifi_set_state(lv_obj_t *icon, bool connected);

/* rotating alert ring: 5px @30% base + 65deg accent sweep animation */
lv_obj_t *alert_ring_create(lv_obj_t *parent);
