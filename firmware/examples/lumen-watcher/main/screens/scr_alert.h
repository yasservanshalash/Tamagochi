#pragma once
#include <stdbool.h>

/* Full-screen alert overlay on lv_layer_top() — the design's
 * "Person detected / back door · just now / View / hold to dismiss". */
void alert_show(const char *title, const char *sub);

/* confirmation variant: btn_text runs on_confirm; tap outside cancels */
void alert_show_confirm(const char *title, const char *sub,
                        const char *btn_text, void (*on_confirm)(void));
void alert_hide(void);
bool alert_visible(void);
