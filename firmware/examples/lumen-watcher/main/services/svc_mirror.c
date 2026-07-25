#include "svc_mirror.h"
#include "lumen.h"
#include "theme.h"
#include "esp_http_server.h"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "esp_system.h"
#include "svc_petlog.h"
#include "svc_assistant.h"
#include "pet.h"
#include "router.h"
#include "esp_lvgl_port.h"
#include "esp_wifi.h"
#include "esp_lvgl_port.h"
#include "lvgl.h"
#include <string.h>

static const char *TAG = "mirror";
static httpd_handle_t srv = NULL;
static uint8_t *snap_buf = NULL;
static uint32_t snap_buf_size = 0;

static const char MIRROR_HTML[] =
"<!doctype html><html><head><meta charset=utf-8><title>Lumen mirror</title>"
"<style>body{background:#000;color:#8A8478;font-family:ui-monospace,monospace;"
"display:flex;flex-direction:column;align-items:center;padding:24px}"
"img{width:412px;height:412px;border-radius:50%;border:1px solid #2B2925}"
"h1{color:#FFB000;font-size:14px;letter-spacing:5px;font-weight:400}</style>"
"</head><body><h1>LUMEN v" LUMEN_VERSION " \xc2\xb7 MIRROR</h1>"
"<img id=s src=/shot.bmp>"
"<p>right-click the image to save a screenshot</p>"
"<pre id=lg style='width:412px;height:220px;overflow-y:auto;"
"background:#14120E;border:1px solid #2B2925;border-radius:12px;"
"padding:10px;font-size:12px;color:#B5AFA2;text-align:left'></pre>"
"<p id=rst></p><script>fetch('/info').then(r=>r.text())"
".then(t=>rst.textContent=t);"
"setInterval(()=>fetch('/log').then(r=>r.text()).then(t=>{"
"lg.textContent=t;lg.scrollTop=lg.scrollHeight}),1500)</script>"
"<script>setInterval(()=>{s.src='/shot.bmp?t='+Date.now()},800)</script>"
"</body></html>";

static esp_err_t mirror_get(httpd_req_t *req)
{
    httpd_resp_set_type(req, "text/html");
    return httpd_resp_send(req, MIRROR_HTML, HTTPD_RESP_USE_STRLEN);
}

static esp_err_t shot_get(httpd_req_t *req)
{
    lv_obj_t *scr = lv_scr_act();
    if (!snap_buf) {
        snap_buf_size = lv_snapshot_buf_size_needed(scr, LV_IMG_CF_TRUE_COLOR);
        snap_buf = heap_caps_malloc(snap_buf_size, MALLOC_CAP_SPIRAM);
        if (!snap_buf) return ESP_FAIL;
    }

    lv_img_dsc_t dsc;
    lvgl_port_lock(0);
    lv_res_t res = lv_snapshot_take_to_buf(scr, LV_IMG_CF_TRUE_COLOR,
                                           &dsc, snap_buf, snap_buf_size);
    lvgl_port_unlock();
    if (res != LV_RES_OK) return ESP_FAIL;

    const int W = dsc.header.w, H = dsc.header.h;
    const int stride = W * 2;                       /* 824B, 4-aligned    */

    /* BMP: 14B file hdr + 40B info + 12B RGB565 bitfield masks           */
    uint8_t hdr[66] = { 0 };
    uint32_t data_sz = stride * H, file_sz = sizeof(hdr) + data_sz;
    hdr[0]='B'; hdr[1]='M';
    memcpy(hdr+2,  &file_sz, 4);
    uint32_t off = sizeof(hdr);            memcpy(hdr+10, &off, 4);
    uint32_t dib = 40;                     memcpy(hdr+14, &dib, 4);
    int32_t  w = W, h = -H;                /* negative = top-down        */
    memcpy(hdr+18, &w, 4); memcpy(hdr+22, &h, 4);
    uint16_t planes = 1, bpp = 16;
    memcpy(hdr+26, &planes, 2); memcpy(hdr+28, &bpp, 2);
    uint32_t comp = 3;                     memcpy(hdr+30, &comp, 4);
    memcpy(hdr+34, &data_sz, 4);
    uint32_t rmask=0xF800, gmask=0x07E0, bmask=0x001F;
    memcpy(hdr+54,&rmask,4); memcpy(hdr+58,&gmask,4); memcpy(hdr+62,&bmask,4);

    httpd_resp_set_type(req, "image/bmp");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");
    httpd_resp_send_chunk(req, (char *)hdr, sizeof(hdr));

    /* LV_COLOR_16_SWAP stores swapped bytes; un-swap per pixel for BMP  */
    static uint8_t row[824];
    const uint8_t *src = dsc.data;
    for (int y = 0; y < H; y++) {
        const uint8_t *s = src + y * stride;
        for (int x = 0; x < stride; x += 2) {
            row[x]     = s[x + 1];
            row[x + 1] = s[x];
        }
        if (httpd_resp_send_chunk(req, (char *)row, stride) != ESP_OK) break;
    }
    httpd_resp_send_chunk(req, NULL, 0);
    return ESP_OK;
}

static esp_err_t log_get(httpd_req_t *req)
{
    static char buf[48 * 74];
    int n = petlog_dump(buf, sizeof(buf));
    httpd_resp_set_type(req, "text/plain");
    return httpd_resp_send(req, buf, n);
}

static void url_decode(char *s)
{
    char *o = s;
    while (*s) {
        if (*s == '+') { *o++ = ' '; s++; }
        else if (*s == '%' && s[1] && s[2]) {
            char h[3] = { s[1], s[2], 0 };
            *o++ = (char)strtol(h, NULL, 16);
            s += 3;
        } else *o++ = *s++;
    }
    *o = 0;
}

static esp_err_t ctl_get(httpd_req_t *req)
{
    char q[220] = { 0 }, cmd[16] = { 0 }, arg[160] = { 0 };
    httpd_req_get_url_query_str(req, q, sizeof(q));
    httpd_query_key_value(q, "cmd", cmd, sizeof(cmd));
    httpd_query_key_value(q, "arg", arg, sizeof(arg));
    url_decode(arg);
    char out[64] = "ok";

    if (!strcmp(cmd, "nav")) {
        if (lvgl_port_lock(500)) {
            if (!strcmp(arg, "home"))          router_home();
            else if (!strcmp(arg, "menu"))     router_go(SCR_MENU);
            else if (!strcmp(arg, "camera"))   router_go(SCR_CAMERA);
            else if (!strcmp(arg, "listen"))   router_go(SCR_LISTEN);
            else if (!strcmp(arg, "settings")) router_go(SCR_SETTINGS);
            lvgl_port_unlock();
        } else snprintf(out, sizeof(out), "ui busy");
    } else if (!strcmp(cmd, "react")) {
        pet_react(arg[0] ? arg : "happy", 0, 0);
    } else if (!strcmp(cmd, "say")) {
        pet_say(arg[0] ? arg : "ctl test", 4000);
    } else if (!strcmp(cmd, "reboot")) {
        httpd_resp_sendstr(req, "rebooting");
        vTaskDelay(pdMS_TO_TICKS(200));
        esp_restart();
        return ESP_OK;
    } else snprintf(out, sizeof(out),
                    "cmds: nav|react|say|reboot (+arg)");
    petlog("ctl: %s %.24s", cmd, arg);
    httpd_resp_set_type(req, "text/plain");
    return httpd_resp_sendstr(req, out);
}

static esp_err_t brain_get(httpd_req_t *req)
{
    char q[160] = { 0 }, url[130] = { 0 };
    httpd_req_get_url_query_str(req, q, sizeof(q));
    if (httpd_query_key_value(q, "url", url, sizeof(url)) == ESP_OK && url[0]) {
        assistant_set_url(url);
        petlog("brain url set via browser");
    }
    char cur[130];
    bool has = assistant_get_url(cur, sizeof(cur));
    char out[200];
    snprintf(out, sizeof(out), "brain: %s\nset with: /brain?url=http://host:8087/pet/think",
             has ? cur : "(demo mode — not set)");
    httpd_resp_set_type(req, "text/plain");
    return httpd_resp_send(req, out, HTTPD_RESP_USE_STRLEN);
}

static esp_err_t info_get(httpd_req_t *req)
{
    wifi_ap_record_t ap = { 0 };
    esp_wifi_sta_get_ap_info(&ap);

    char buf[96];
    snprintf(buf, sizeof(buf), "last reset: %s \xc2\xb7 free heap: %lu",
             g_state.last_reset, (unsigned long)esp_get_free_heap_size());
    httpd_resp_set_type(req, "text/plain");
    return httpd_resp_send(req, buf, HTTPD_RESP_USE_STRLEN);
}

void svc_mirror_start(void)
{
    if (srv) return;
    httpd_config_t cfg = HTTPD_DEFAULT_CONFIG();
    cfg.max_open_sockets = 7;
    cfg.lru_purge_enable = true;
    cfg.lru_purge_enable = true;
    if (httpd_start(&srv, &cfg) != ESP_OK) return;
    httpd_uri_t u_root = { .uri = "/",         .method = HTTP_GET, .handler = mirror_get };
    httpd_uri_t u_mir  = { .uri = "/mirror",   .method = HTTP_GET, .handler = mirror_get };
    httpd_uri_t u_shot = { .uri = "/shot.bmp", .method = HTTP_GET, .handler = shot_get };
    httpd_register_uri_handler(srv, &u_root);
    httpd_register_uri_handler(srv, &u_mir);
    httpd_register_uri_handler(srv, &u_shot);
    httpd_uri_t u_info = { .uri = "/info", .method = HTTP_GET, .handler = info_get };
    httpd_register_uri_handler(srv, &u_info);
    httpd_uri_t u_log = { .uri = "/log", .method = HTTP_GET, .handler = log_get };
    httpd_register_uri_handler(srv, &u_log);
    httpd_uri_t u_brain = { .uri = "/brain", .method = HTTP_GET, .handler = brain_get };
    httpd_register_uri_handler(srv, &u_brain);
    httpd_uri_t u_ctl = { .uri = "/ctl", .method = HTTP_GET, .handler = ctl_get };
    httpd_register_uri_handler(srv, &u_ctl);
    ESP_LOGI(TAG, "mirror at http://%s/", g_state.ip);
}
