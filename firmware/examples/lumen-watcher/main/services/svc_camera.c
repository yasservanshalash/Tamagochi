#include "svc_camera.h"
#include "lumen.h"
#include "sensecap-watcher.h"
#include "sscma_client.h"
#include "esp_log.h"
#include "esp_heap_caps.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "freertos/task.h"
#include "mbedtls/base64.h"
#include "esp_jpeg_dec.h"
#include "esp_lvgl_port.h"
#include "svc_ble.h"
#include <string.h>
#include <stdio.h>

/* Himax detection + live preview.
 *
 * Architecture (mirrors stock): the SSCMA event callback stays LEAN —
 * it parses detection boxes and hands the raw base64 frame to a
 * dedicated decode task (8 KB stack). Decoding in the event task was
 * the v0.9 camera crash: that task runs on a 4 KB stack by default
 * (factory firmware quietly raises it to 10 KB for the same reason).
 *
 * Frames are double-buffered: decode writes the back buffer, then the
 * front/back pointers swap under the mutex — the renderer never reads
 * a buffer being written (the v0.9 tearing / "bad quality").          */

static const char *TAG = "svc_camera";

#define SENSOR_RES_240   0     /* native display-sized frames */
#define B64_STAGE_SIZE   (96 * 1024)
#define JPEG_BUF_SIZE    (64 * 1024)
#define DECODE_MAX_W     240
#define DECODE_MAX_H     240
#define DECODE_BUF_SIZE  (DECODE_MAX_W * DECODE_MAX_H * 2)
#define FRAME_MAX_W      DECODE_MAX_W
#define FRAME_MAX_H      DECODE_MAX_H
#define FRAME_BUF_SIZE   (FRAME_MAX_W * FRAME_MAX_H * 2)

static sscma_client_handle_t client = NULL;

static uint8_t  *b64_stage = NULL;      /* event -> decode handoff      */
static volatile int b64_len = 0;
static SemaphoreHandle_t stage_lock = NULL;
static TaskHandle_t decode_task_h = NULL;

static uint8_t  *jpeg_buf  = NULL;
static uint8_t  *decode_buf = NULL;     /* full-res decode scratch      */
static uint8_t  *display_buf = NULL;    /* THE lv_img source (factory
                                           pattern: single buffer, only
                                           touched under the LVGL lock) */
static volatile uint32_t frame_seq = 0;

static jpeg_dec_handle_t       jdec  = NULL;
static jpeg_dec_io_t          *jio   = NULL;
static jpeg_dec_header_info_t *jinfo = NULL;

uint32_t svc_camera_frame_seq(void) { return frame_seq; }
const uint8_t *svc_camera_display_buf(void) { return display_buf; }

static void decode_task(void *arg)
{
    (void)arg;
    for (;;) {
        ulTaskNotifyTake(pdTRUE, portMAX_DELAY);

        xSemaphoreTake(stage_lock, portMAX_DELAY);
        size_t jlen = 0;
        bool ok = mbedtls_base64_decode(jpeg_buf, JPEG_BUF_SIZE, &jlen,
                                        b64_stage, b64_len) == 0 && jlen > 0;
        b64_len = 0;
        xSemaphoreGive(stage_lock);
        if (!ok) continue;

        jio->inbuf = jpeg_buf;
        jio->inbuf_len = (int)jlen;
        if (jpeg_dec_parse_header(jdec, jio, jinfo) < 0) continue;
        if (jinfo->width > DECODE_MAX_W || jinfo->height > DECODE_MAX_H) continue;
        jio->outbuf = decode_buf;
        int consumed = jio->inbuf_len - jio->inbuf_remain;
        jio->inbuf = jpeg_buf + consumed;
        jio->inbuf_len = jio->inbuf_remain;
        if (jpeg_dec_process(jdec, jio) < 0) continue;

        /* factory pattern: the display buffer changes ONLY while holding
         * the LVGL lock. Native 240px frames: straight copy, ~2 ms.     */
        if (!g_state.cam_preview_on) continue;
        int bytes = jinfo->width * jinfo->height * 2;
        if (bytes > FRAME_BUF_SIZE) continue;
        if (lvgl_port_lock(100)) {
            memcpy(display_buf, decode_buf, bytes);
            frame_seq++;
            lvgl_port_unlock();
        }
    }
}

/* SSCMA process task: keep it lean — parse boxes, stage the frame, go. */
static void on_event(sscma_client_handle_t c, const sscma_client_reply_t *reply, void *ctx)
{
    (void)c; (void)ctx;

    sscma_client_box_t *boxes = NULL;
    int n = 0;
    if (sscma_utils_fetch_boxes_from_reply(reply, &boxes, &n) == ESP_OK) {
        if (n > 0) {
            int best = 0;
            for (int i = 1; i < n; i++)
                if (boxes[i].score > boxes[best].score) best = i;
            g_state.cam_score = boxes[best].score;
            g_state.cam_x = boxes[best].x; g_state.cam_y = boxes[best].y;
            g_state.cam_w = boxes[best].w; g_state.cam_h = boxes[best].h;
            if (boxes[best].score >= 88)
                svc_ble_notify_detection(
                    boxes[best].target == 0 ? "PERSON" : "OBJ",
                    boxes[best].score);
            if (boxes[best].target == 0) strcpy(g_state.cam_label, "PERSON");
            else snprintf(g_state.cam_label, sizeof(g_state.cam_label),
                          "OBJ %u", boxes[best].target);
        } else if (g_state.cam_score > 0) {
            g_state.cam_score = g_state.cam_score > 4 ? g_state.cam_score - 4 : 0;
        }
        free(boxes);
    }

    if (!g_state.cam_preview_on || !decode_task_h) return;
    char *img = NULL;
    int img_len = 0;
    if (sscma_utils_fetch_image_from_reply(reply, &img, &img_len) == ESP_OK && img) {
        /* latest-wins: if the decoder is busy, drop this frame */
        if (img_len <= B64_STAGE_SIZE &&
            xSemaphoreTake(stage_lock, 0) == pdTRUE) {
            memcpy(b64_stage, img, img_len);
            b64_len = img_len;
            xSemaphoreGive(stage_lock);
            xTaskNotifyGive(decode_task_h);
        }
        free(img);
    }
}

bool svc_camera_start(void)
{
    if (client) return g_state.cam_ready;

    stage_lock  = xSemaphoreCreateMutex();
    b64_stage = heap_caps_malloc(B64_STAGE_SIZE, MALLOC_CAP_SPIRAM);
    jpeg_buf  = heap_caps_malloc(JPEG_BUF_SIZE, MALLOC_CAP_SPIRAM);
    decode_buf = heap_caps_aligned_alloc(16, DECODE_BUF_SIZE, MALLOC_CAP_SPIRAM);
    display_buf = heap_caps_aligned_alloc(16, FRAME_BUF_SIZE, MALLOC_CAP_SPIRAM);
    jio   = heap_caps_calloc(1, sizeof(jpeg_dec_io_t), MALLOC_CAP_SPIRAM);
    jinfo = heap_caps_aligned_alloc(16, sizeof(jpeg_dec_header_info_t), MALLOC_CAP_SPIRAM);
    jpeg_dec_config_t jcfg = { .output_type = JPEG_RAW_TYPE_RGB565_BE,
                               .rotate = JPEG_ROTATE_0D };
    jdec = jpeg_dec_open(&jcfg);
    if (!b64_stage || !jpeg_buf || !decode_buf || !display_buf || !jio || !jinfo || !jdec) {
        ESP_LOGE(TAG, "preview alloc failed");
        return false;
    }
    memset(jinfo, 0, sizeof(*jinfo));

    if (xTaskCreatePinnedToCore(decode_task, "cam_decode", 8192, NULL, 5,
                                &decode_task_h, 0) != pdPASS) {
        ESP_LOGE(TAG, "decode task failed");
        return false;
    }

    client = bsp_sscma_client_init();
    if (!client) { ESP_LOGE(TAG, "sscma io init failed"); return false; }

    static const sscma_client_callback_t cb = { .on_event = on_event };
    if (sscma_client_register_callback(client, &cb, NULL) != ESP_OK) return false;

    /* the Himax keeps running across ESP reflashes; a wedged invoke
     * session here is what froze camera entry. Hard-reset it first.    */
    sscma_client_reset(client);
    sscma_client_init(client);
    sscma_client_break(client);
    if (sscma_client_set_sensor(client, 1, SENSOR_RES_240, true) != ESP_OK)
        ESP_LOGW(TAG, "set_sensor failed (continuing)");
    if (sscma_client_invoke(client, -1, false, true) != ESP_OK) {
        ESP_LOGE(TAG, "invoke failed");
        return false;
    }
    g_state.cam_ready = true;
    ESP_LOGI(TAG, "himax invoking @240x240 native");
    return true;
}

void svc_camera_stop(void)
{
    if (!client) return;
    sscma_client_break(client);
    g_state.cam_ready = false;
}
