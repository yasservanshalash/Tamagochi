/*
 * Lumen theme — the Claude Design style guide mapped 1:1 to LVGL.
 *
 *  COLOR                       TYPE
 *  bg        #000000           clock  Space Grotesk 500 104px  ls -4
 *  surface   #14120E           title  Space Grotesk 500  34px
 *  line      #2B2925           body   Space Grotesk 500  30px  lh 1.4
 *  track     #23211D           data   IBM Plex Mono 400 24px
 *  text-hi   #F5F2EA           label  IBM Plex Mono 400 20px  ls 4-6 caps
 *  text-lo   #8A8478
 *  accent    #FFB000           ARCS  r196 w6 round caps, track #23211D
 *
 *  TARGETS  min tap 80x80 · row 80 · pill r40 · slot 88 · safe zone d370
 */
#pragma once

#include "lvgl.h"

/* ---- colors ---- */
#define C_BG       lv_color_hex(0x000000)
#define C_SURFACE  lv_color_hex(0x14120E)
#define C_LINE     lv_color_hex(0x2B2925)
#define C_TRACK    lv_color_hex(0x23211D)
#define C_TEXT_HI  lv_color_hex(0xF5F2EA)
#define C_TEXT_LO  lv_color_hex(0x8A8478)
#define C_ACCENT   lv_color_hex(0xFFB000)

/* ---- geometry ---- */
#define SCREEN_D        412
#define SAFE_D          370
#define ARC_R           196     /* rim arc radius            */
#define ARC_W           6       /* rim arc stroke            */
#define ROW_H           80      /* settings row / min target */
#define SLOT_D          88      /* menu slot circle          */

/* rim arc sweeps (degrees) — from the style guide */
#define ARC_BATTERY_SWEEP    72   /* centered right (0 deg)   */
#define ARC_CONF_SWEEP      108   /* centered top  (-90 deg)  */
#define ARC_SCROLL_SWEEP     79   /* centered right           */
#define ARC_FOCUS_SWEEP      36   /* follows menu slot        */
#define ALERT_RING_W          5
#define ALERT_SWEEP          65

/* ---- fonts (generated from the design's exact faces/sizes) ---- */
LV_FONT_DECLARE(font_sg_104);   /* clock  */
LV_FONT_DECLARE(font_sg_34);    /* titles */
LV_FONT_DECLARE(font_sg_30);    /* body   */
LV_FONT_DECLARE(font_mono_24);  /* data   */
LV_FONT_DECLARE(font_mono_20);  /* labels */

/* ---- shared styles (init once in theme_init) ---- */
extern lv_style_t st_screen;    /* black bg, no pad/scroll         */
extern lv_style_t st_clock;     /* 104px hi, ls -4                 */
extern lv_style_t st_title;     /* 34px hi                         */
extern lv_style_t st_body;      /* 30px hi, lh 1.4                 */
extern lv_style_t st_body_lo;   /* 30px lo                         */
extern lv_style_t st_data;      /* mono 24 hi                      */
extern lv_style_t st_label;     /* mono 20 lo, ls 5, caps by usage */
extern lv_style_t st_label_acc; /* mono 20 accent, ls 5            */

void theme_init(void);

/* helper: bare screen with theme applied */
lv_obj_t *theme_screen_create(void);

/* helper: the standard top screen title (mono accent caps, one position
 * for every screen). Returns the label for dynamic screens.          */
lv_obj_t *theme_title(lv_obj_t *parent, const char *txt);
