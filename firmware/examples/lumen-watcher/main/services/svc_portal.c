#include "svc_portal.h"
#include "svc_mirror.h"
#include "lumen.h"
#include "esp_wifi.h"
#include "esp_http_server.h"
#include "esp_log.h"
#include "lwip/sockets.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include <string.h>
#include <stdio.h>
#include <ctype.h>

static const char *TAG = "portal";
static httpd_handle_t httpd = NULL;
static TaskHandle_t dns_task_h = NULL;
static volatile bool dns_run = false;

/* ---------------- DNS catch-all (every name -> 192.168.4.1) ---------------- */
static void dns_task(void *arg)
{
    (void)arg;
    int sock = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
    struct sockaddr_in addr = { .sin_family = AF_INET,
                                .sin_port = htons(53),
                                .sin_addr.s_addr = htonl(INADDR_ANY) };
    bind(sock, (struct sockaddr *)&addr, sizeof(addr));
    struct timeval tv = { .tv_sec = 1 };
    setsockopt(sock, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));

    uint8_t buf[256];
    while (dns_run) {
        struct sockaddr_in src;
        socklen_t slen = sizeof(src);
        int len = recvfrom(sock, buf, sizeof(buf), 0, (struct sockaddr *)&src, &slen);
        if (len < 12) continue;
        /* answer: copy query, set response flags, append A record 192.168.4.1 */
        buf[2] = 0x81; buf[3] = 0x80;            /* std response, recursion  */
        buf[6] = 0x00; buf[7] = 0x01;            /* 1 answer                 */
        buf[8] = buf[9] = buf[10] = buf[11] = 0;
        int q = 12;
        while (q < len && buf[q]) q += buf[q] + 1;   /* skip QNAME           */
        q += 5;                                       /* null + type + class */
        if (q + 16 > (int)sizeof(buf)) continue;
        uint8_t ans[] = { 0xC0, 0x0C,             /* name ptr to query      */
                          0x00, 0x01, 0x00, 0x01, /* A, IN                  */
                          0x00, 0x00, 0x00, 0x1E, /* TTL 30s                */
                          0x00, 0x04, 192, 168, 4, 1 };
        memcpy(buf + q, ans, sizeof(ans));
        sendto(sock, buf, q + sizeof(ans), 0, (struct sockaddr *)&src, slen);
    }
    close(sock);
    dns_task_h = NULL;
    vTaskDelete(NULL);
}

/* ---------------- helpers ---------------- */
static int urldecode(char *s)
{
    char *o = s, *p = s;
    while (*p) {
        if (*p == '+') { *o++ = ' '; p++; }
        else if (*p == '%' && isxdigit((int)p[1]) && isxdigit((int)p[2])) {
            char h[3] = { p[1], p[2], 0 };
            *o++ = (char)strtol(h, NULL, 16);
            p += 3;
        } else *o++ = *p++;
    }
    *o = 0;
    return o - s;
}

static bool form_field(const char *body, const char *key, char *out, int out_len)
{
    char pat[24];
    snprintf(pat, sizeof(pat), "%s=", key);
    const char *p = strstr(body, pat);
    if (!p) return false;
    p += strlen(pat);
    int i = 0;
    while (*p && *p != '&' && i < out_len - 1) out[i++] = *p++;
    out[i] = 0;
    urldecode(out);
    return true;
}

/* ---------------- scan cache (filled before the AP goes up) ------------- */
#define MAX_APS 12
static wifi_ap_record_t ap_cache[MAX_APS];
static uint16_t ap_count = 0;

static void scan_now(void)
{
    wifi_scan_config_t sc = { .show_hidden = false };
    if (esp_wifi_scan_start(&sc, true) == ESP_OK) {
        ap_count = MAX_APS;
        esp_wifi_scan_get_ap_records(&ap_count, ap_cache);
    }
}

/* ---------------- pages ---------------- */
static const char PAGE_HDR[] =
"<!doctype html><html><head><meta charset=utf-8>"
"<meta name=viewport content='width=device-width,initial-scale=1'>"
"<title>Lumen setup</title><style>"
"body{background:#000;color:#F5F2EA;font-family:ui-monospace,monospace;"
"margin:0;padding:28px;max-width:420px}h1{color:#FFB000;font-size:15px;"
"letter-spacing:5px;font-weight:400}a,button,input,select{width:100%;"
"box-sizing:border-box;background:#14120E;color:#F5F2EA;border:1px solid "
"#2B2925;border-radius:40px;padding:16px 20px;font:inherit;margin:8px 0}"
"button{background:#FFB000;color:#000;border:0;font-weight:700}"
".lo{color:#8A8478;font-size:13px}</style></head><body>"
"<h1>LUMEN \xc2\xb7 WI-FI SETUP</h1>";

static esp_err_t root_get(httpd_req_t *req)
{
    char q[16] = { 0 };
    httpd_req_get_url_query_str(req, q, sizeof(q));
    if (strstr(q, "rescan"))
        scan_now();     /* explicit only: hops channels, phone may blip */

    /* prefill the SSID with any stored (failing) credentials */
    wifi_config_t cur = { 0 };
    esp_wifi_get_config(WIFI_IF_STA, &cur);

    httpd_resp_set_type(req, "text/html");
    httpd_resp_send_chunk(req, PAGE_HDR, HTTPD_RESP_USE_STRLEN);
    httpd_resp_send_chunk(req,
        "<form method=post action=/connect>"
        "<input name=ssid id=ss list=nets placeholder='network name' required>"
        "<datalist id=nets>", HTTPD_RESP_USE_STRLEN);
    if (cur.sta.ssid[0]) {
        char pre[64];
        snprintf(pre, sizeof(pre),
                 "<script>ss.value='%.32s'</script>", (char *)cur.sta.ssid);
        /* placed after datalist below via chunk order is fine for browsers,
         * but keep it simple: emit now, browsers tolerate it */
        httpd_resp_send_chunk(req, pre, HTTPD_RESP_USE_STRLEN);
    }
    char row[96];
    for (int i = 0; i < ap_count; i++) {
        snprintf(row, sizeof(row), "<option>%.32s</option>",
                 (char *)ap_cache[i].ssid);
        httpd_resp_send_chunk(req, row, HTTPD_RESP_USE_STRLEN);
    }
    httpd_resp_send_chunk(req,
        "</datalist><input name=pass id=pw type=password placeholder=password>"
        "<label class=lo style='display:flex;gap:8px;align-items:center;"
        "border:0;background:none'><input type=checkbox style='width:auto'"
        " onclick=\"pw.type=this.checked?'text':'password'\">show password"
        "</label><button>Connect</button></form>"
        "<p class=lo>pick from the list or type a name \xc2\xb7 "
        "<a href='/?rescan=1'>rescan</a> (phone may blip off briefly)</p>"
        "</body></html>", HTTPD_RESP_USE_STRLEN);
    httpd_resp_send_chunk(req, NULL, 0);
    return ESP_OK;
}

static esp_err_t connect_post(httpd_req_t *req)
{
    char body[192] = { 0 };
    int len = httpd_req_recv(req, body, sizeof(body) - 1);
    if (len <= 0) return ESP_FAIL;

    wifi_config_t wc = { 0 };
    char pass[65] = { 0 };
    if (!form_field(body, "ssid", (char *)wc.sta.ssid, sizeof(wc.sta.ssid)))
        return ESP_FAIL;
    form_field(body, "pass", pass, sizeof(pass));
    size_t plen = strlen(pass);
    if (plen >= sizeof(wc.sta.password)) plen = sizeof(wc.sta.password) - 1;
    memcpy(wc.sta.password, pass, plen);

    ESP_LOGI(TAG, "connecting to %s", wc.sta.ssid);
    esp_wifi_set_config(WIFI_IF_STA, &wc);     /* persists to NVS */
    esp_wifi_connect();

    httpd_resp_set_type(req, "text/html");
    httpd_resp_send_chunk(req, PAGE_HDR, HTTPD_RESP_USE_STRLEN);
    httpd_resp_send_chunk(req,
        "<p>Connecting\xc2\xb7\xc2\xb7\xc2\xb7 watch the device screen.</p>"
        "<p class=lo>If the password was wrong, rejoin the watcher hotspot "
        "and this page returns.</p></body></html>", HTTPD_RESP_USE_STRLEN);
    httpd_resp_send_chunk(req, NULL, 0);
    return ESP_OK;
}

/* everything else (incl. Android/iOS captive checks) -> redirect to portal */
static esp_err_t any_get(httpd_req_t *req)
{
    httpd_resp_set_status(req, "302 Found");
    httpd_resp_set_hdr(req, "Location", "http://192.168.4.1/");
    httpd_resp_send(req, NULL, 0);
    return ESP_OK;
}

void svc_portal_start(void)
{
    if (httpd) return;

    /* scan while still pure-STA: no AP exists yet, so nothing to kick.
     * esp_wifi_start() is a no-op ESP_OK if wifi is already running
     * (auto-fallback path arrives here with wifi started).              */
    ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_STA));
    esp_wifi_start();
    scan_now();

    wifi_config_t ap = { 0 };
    strncpy((char *)ap.ap.ssid, g_state.device_name, sizeof(ap.ap.ssid));
    ap.ap.ssid_len = strlen(g_state.device_name);
    ap.ap.authmode = WIFI_AUTH_OPEN;
    ap.ap.max_connection = 2;
    ap.ap.channel = 1;
    ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_APSTA));
    ESP_ERROR_CHECK(esp_wifi_set_config(WIFI_IF_AP, &ap));

    httpd_config_t cfg = HTTPD_DEFAULT_CONFIG();
    cfg.max_open_sockets = 7;
    cfg.lru_purge_enable = true;
    cfg.uri_match_fn = httpd_uri_match_wildcard;
    cfg.max_uri_handlers = 4;
    if (httpd_start(&httpd, &cfg) == ESP_OK) {
        httpd_uri_t u_root = { .uri = "/",        .method = HTTP_GET,  .handler = root_get };
        httpd_uri_t u_conn = { .uri = "/connect", .method = HTTP_POST, .handler = connect_post };
        httpd_uri_t u_any  = { .uri = "/*",       .method = HTTP_GET,  .handler = any_get };
        httpd_register_uri_handler(httpd, &u_root);
        httpd_register_uri_handler(httpd, &u_conn);
        httpd_register_uri_handler(httpd, &u_any);
    }

    dns_run = true;
    xTaskCreatePinnedToCore(dns_task, "portal_dns", 3072, NULL, 4, &dns_task_h, 0);
    g_state.provisioning = true;
    snprintf(g_state.prov_qr_payload, sizeof(g_state.prov_qr_payload),
             "WIFI:T:nopass;S:%s;;", g_state.device_name);   /* any camera app */
    ESP_LOGI(TAG, "portal up as %s", g_state.device_name);
}

void svc_portal_stop(void)
{
    dns_run = false;
    if (httpd) { httpd_stop(httpd); httpd = NULL; }
    esp_wifi_set_mode(WIFI_MODE_STA);
    g_state.provisioning = false;
    ESP_LOGI(TAG, "portal down");
    if (g_state.wifi_connected)
        svc_mirror_start();          /* port 80 is free now: hand it over */
}
