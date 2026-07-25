#pragma once
#include <stdbool.h>
#include <stdint.h>

/* Himax detection + live preview (see svc_camera.c for the pipeline).
 * Lazily started on first camera-screen entry; keeps running afterwards
 * so detection alerts stay armed. Preview decode is gated by
 * g_state.cam_preview_on.                                              */
bool            svc_camera_start(void);
void            svc_camera_stop(void);

/* single display buffer (208x208 RGB565, factory pattern): its content
 * only changes under the LVGL lock, so reading it from LVGL is safe.   */
uint32_t        svc_camera_frame_seq(void);
const uint8_t  *svc_camera_display_buf(void);
