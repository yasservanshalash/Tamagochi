#include "theme.h"

lv_style_t st_screen;
lv_style_t st_clock;
lv_style_t st_title;
lv_style_t st_body;
lv_style_t st_body_lo;
lv_style_t st_data;
lv_style_t st_label;
lv_style_t st_label_acc;

void theme_init(void)
{
    lv_style_init(&st_screen);
    lv_style_set_bg_color(&st_screen, C_BG);
    lv_style_set_bg_opa(&st_screen, LV_OPA_COVER);
    lv_style_set_border_width(&st_screen, 0);
    lv_style_set_pad_all(&st_screen, 0);
    lv_style_set_radius(&st_screen, 0);

    lv_style_init(&st_clock);
    lv_style_set_text_font(&st_clock, &font_sg_104);
    lv_style_set_text_color(&st_clock, C_TEXT_HI);
    lv_style_set_text_letter_space(&st_clock, -4);

    lv_style_init(&st_title);
    lv_style_set_text_font(&st_title, &font_sg_34);
    lv_style_set_text_color(&st_title, C_TEXT_HI);

    lv_style_init(&st_body);
    lv_style_set_text_font(&st_body, &font_sg_30);
    lv_style_set_text_color(&st_body, C_TEXT_HI);
    lv_style_set_text_line_space(&st_body, 12);   /* 30px * 1.4 lh */

    lv_style_init(&st_body_lo);
    lv_style_set_text_font(&st_body_lo, &font_sg_30);
    lv_style_set_text_color(&st_body_lo, C_TEXT_LO);
    lv_style_set_text_line_space(&st_body_lo, 12);

    lv_style_init(&st_data);
    lv_style_set_text_font(&st_data, &font_mono_24);
    lv_style_set_text_color(&st_data, C_TEXT_HI);

    lv_style_init(&st_label);
    lv_style_set_text_font(&st_label, &font_mono_20);
    lv_style_set_text_color(&st_label, C_TEXT_LO);
    lv_style_set_text_letter_space(&st_label, 5);

    lv_style_init(&st_label_acc);
    lv_style_set_text_font(&st_label_acc, &font_mono_20);
    lv_style_set_text_color(&st_label_acc, C_ACCENT);
    lv_style_set_text_letter_space(&st_label_acc, 5);
}

lv_obj_t *theme_screen_create(void)
{
    lv_obj_t *scr = lv_obj_create(NULL);
    lv_obj_add_style(scr, &st_screen, 0);
    lv_obj_clear_flag(scr, LV_OBJ_FLAG_SCROLLABLE);
    return scr;
}

lv_obj_t *theme_title(lv_obj_t *parent, const char *txt)
{
    lv_obj_t *l = lv_label_create(parent);
    lv_obj_add_style(l, &st_label_acc, 0);
    lv_label_set_text(l, txt);
    lv_obj_align(l, LV_ALIGN_TOP_MID, 0, 40);
    return l;
}
