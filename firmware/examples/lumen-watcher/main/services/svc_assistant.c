#include "svc_assistant.h"
#include "lumen.h"
#include "pet_persona.h"
#include "esp_http_client.h"
#include "esp_wifi.h"
#include "esp_log.h"
#include "cJSON.h"
#include "svc_petlog.h"
#include "router.h"
#include "esp_netif.h"
#include "nvs.h"
#include <string.h>
#include <stdio.h>
#include <time.h>

static const char *TAG = "assistant";

static char pending_action[12] = "";
const char *assistant_take_action(void)
{
    static char out[12];
    strncpy(out, pending_action, sizeof(out));
    pending_action[0] = 0;
    return out;
}


bool assistant_wake_check(const uint8_t *pcm, int len, char *heard, int hlen)
{
    if (heard && hlen) heard[0] = 0;
    char base[128];
    if (!assistant_get_url(base, sizeof(base))) return false;
    char *cut = strstr(base, "/pet/think");
    if (cut) *cut = 0;
    char url[160];
    snprintf(url, sizeof(url), "%s/pet/wake", base);
    esp_http_client_config_t cfg = {
        .url = url, .method = HTTP_METHOD_POST, .timeout_ms = 15000 };
    esp_http_client_handle_t cli = esp_http_client_init(&cfg);
    esp_http_client_set_header(cli, "Content-Type", "application/octet-stream");
    esp_wifi_set_ps(WIFI_PS_NONE);
    bool woke = false;
    static char resp[300];
    if (esp_http_client_open(cli, len) == ESP_OK) {
        int w = 0;
        while (w < len) {
            int r = esp_http_client_write(cli, (const char *)pcm + w, len - w);
            if (r <= 0) break;
            w += r;
        }
        if (w == len) {
            esp_http_client_fetch_headers(cli);
            int rl = 0, r;
            while (rl < (int)sizeof(resp) - 1 &&
                   (r = esp_http_client_read(cli, resp + rl,
                                             sizeof(resp) - 1 - rl)) > 0)
                rl += r;
            resp[rl] = 0;
            cJSON *j = cJSON_Parse(resp);
            if (j) {
                woke = cJSON_IsTrue(cJSON_GetObjectItem(j, "wake"));
                cJSON *h = cJSON_GetObjectItem(j, "heard");
                if (heard && hlen && cJSON_IsString(h))
                    snprintf(heard, hlen, "%s", h->valuestring);
                cJSON_Delete(j);
            }
        }
    }
    esp_http_client_cleanup(cli);
    if (!g_state.voice_playing) esp_wifi_set_ps(WIFI_PS_MIN_MODEM);
    return woke;
}

bool assistant_converse(const uint8_t *pcm, int pcm_len,
                        char *heard, int heard_len,
                        char *say, int say_len,
                        char *emotion, int emo_len, uint8_t *glitch,
                        char *audio_url, int audio_len)
{
    if (heard_len) heard[0] = 0;
    if (audio_len) audio_url[0] = 0;
    char base[128];
    if (!assistant_get_url(base, sizeof(base))) return false;

    /* .../pet/think -> .../pet/converse?ctx  */
    char *cut = strstr(base, "/pet/think");
    if (cut) *cut = 0;
    time_t now = time(NULL); struct tm ti; localtime_r(&now, &ti);
    pet_persona_t p = pet_persona_get();
    char url[300];
    snprintf(url, sizeof(url),
        "%s/pet/converse?battery=%u&hour=%d&mood=%u&paranoia=%u"
        "&curiosity=%u&energy=%u&boredom=%u&trust=%u",
        base, g_state.battery_pct, g_state.time_valid ? ti.tm_hour : 12,
        p.mood, p.paranoia, p.curiosity, p.energy, p.boredom, p.trust);

    esp_http_client_config_t cfg = {
        .url = url, .method = HTTP_METHOD_POST, .timeout_ms = 45000 };
    esp_http_client_handle_t cli = esp_http_client_init(&cfg);
    esp_http_client_set_header(cli, "Content-Type", "application/octet-stream");

    /* modem power-save naps push LAN RTT to 400-800ms and stall the
     * multi-10KB PCM upload until the 45s timeout — radio fully awake
     * for the transfer, same pattern as the voice stream.             */
    esp_wifi_set_ps(WIFI_PS_NONE);

    bool ok = false;
    static char resp[600];
    if (esp_http_client_open(cli, pcm_len) == ESP_OK) {
        int w = 0;
        while (w < pcm_len) {
            int r = esp_http_client_write(cli, (const char *)pcm + w,
                                          pcm_len - w);
            if (r <= 0) break;
            w += r;
        }
        if (w == pcm_len) {
            esp_http_client_fetch_headers(cli);
            int status = esp_http_client_get_status_code(cli);
            int len = 0, r;
            while (len < (int)sizeof(resp) - 1 &&
                   (r = esp_http_client_read(cli, resp + len,
                                             sizeof(resp) - 1 - len)) > 0)
                len += r;
            if (status == 200 && len > 0) {
                resp[len] = 0;
                cJSON *j = cJSON_Parse(resp);
                cJSON *s = j ? cJSON_GetObjectItem(j, "say") : NULL;
                if (cJSON_IsString(s)) {
                    snprintf(say, say_len, "%s", s->valuestring);
                    cJSON *e = cJSON_GetObjectItem(j, "emotion");
                    cJSON *g = cJSON_GetObjectItem(j, "glitch");
                    cJSON *hd = cJSON_GetObjectItem(j, "heard");
                    cJSON *au = cJSON_GetObjectItem(j, "audio");
                    snprintf(emotion, emo_len, "%s",
                             cJSON_IsString(e) ? e->valuestring : "talk");
                    *glitch = cJSON_IsNumber(g) ? (uint8_t)g->valuedouble : 0;
                    cJSON *ac = cJSON_GetObjectItem(j, "action");
                    if (cJSON_IsString(ac))
                        strncpy(pending_action, ac->valuestring,
                                sizeof(pending_action) - 1);
                    if (heard_len && cJSON_IsString(hd))
                        snprintf(heard, heard_len, "%s", hd->valuestring);
                    if (audio_len && cJSON_IsString(au) && au->valuestring[0]) {
                        char *sl = strstr(base, "://");
                        sl = sl ? strchr(sl + 3, '/') : NULL;
                        int bl = sl ? (int)(sl - base) : (int)strlen(base);
                        snprintf(audio_url, audio_len, "%.*s%s",
                                 bl, base, au->valuestring);
                    }
                    ok = true;
                }
                cJSON_Delete(j);
            } else ESP_LOGW(TAG, "converse: status=%d len=%d", status, len);
        }
    }
    esp_http_client_cleanup(cli);
    /* don't re-nap the radio out from under an active voice stream */
    if (!g_state.voice_playing) esp_wifi_set_ps(WIFI_PS_MIN_MODEM);
    petlog("converse: %s", ok ? "ok" : "FAILED");
    return ok;
}

void assistant_set_url(const char *url)
{
    nvs_handle_t h;
    if (nvs_open("lumen", NVS_READWRITE, &h) == ESP_OK) {
        nvs_set_str(h, "brain_url", url);
        nvs_commit(h);
        nvs_close(h);
        ESP_LOGI(TAG, "brain url set: %s", url);
    }
}

bool assistant_get_url(char *out, int len)
{
    nvs_handle_t h;
    size_t l = len;
    if (nvs_open("lumen", NVS_READONLY, &h) != ESP_OK) return false;
    bool ok = nvs_get_str(h, "brain_url", out, &l) == ESP_OK && out[0];
    nvs_close(h);
    return ok;
}

static const char *DEMO[] = {
    "the pigeons went quiet. that's exactly what they'd do.|suspicious|10",
    "classified transmission received. it's just static. or IS it.|glitch|70",
    "i counted the router blinks. it's morse for 'buy snacks'.|point|0",
    "you again! statistically my favorite person in this room.|laugh|0",
    "the mug has been staring at me all day. i respect it.|whisper|20",
};

bool assistant_think(const char *event, const char *text,
                     char *say, int say_len,
                     char *emotion, int emo_len, uint8_t *glitch,
                     char *audio_url, int audio_len)
{
    if (audio_len) audio_url[0] = 0;
    char url[128];
    if (!assistant_get_url(url, sizeof(url))) {
        static int di = 0;                       /* offline: stay in character */
        char tmp[160];
        strncpy(tmp, DEMO[di++ % 5], sizeof(tmp) - 1);
        char *e = strchr(tmp, '|'); *e++ = 0;
        char *g = strchr(e, '|');   *g++ = 0;
        snprintf(say, say_len, "%s", tmp);
        snprintf(emotion, emo_len, "%s", e);
        *glitch = atoi(g);
        return true;
    }

    time_t now = time(NULL); struct tm ti; localtime_r(&now, &ti);
    pet_persona_t p = pet_persona_get();

    char logs[420];
    petlog_tail(logs, sizeof(logs), 6);
    esp_netif_ip_info_t ipi = { 0 };
    esp_netif_t *nif = esp_netif_get_handle_from_ifkey("WIFI_STA_DEF");
    if (nif) esp_netif_get_ip_info(nif, &ipi);

    static char body[1200];
    snprintf(body, sizeof(body),
        "{\"event\":\"%s\",\"text\":\"%.120s\","
        "\"vitals\":{\"battery\":%u,\"hour\":%d,\"charging\":%s},"
        "\"persona\":{\"mood\":%u,\"paranoia\":%u,\"curiosity\":%u,"
        "\"energy\":%u,\"boredom\":%u,\"trust\":%u},"
        "\"senses\":{\"screen\":\"%s\",\"person\":%s,"
        "\"person_score\":%d,\"ip\":\"" IPSTR "\","
        "\"log\":\"%s\"}}",
        event, text ? text : "",
        g_state.battery_pct, g_state.time_valid ? ti.tm_hour : 12,
        g_state.charging ? "true" : "false",
        p.mood, p.paranoia, p.curiosity, p.energy, p.boredom, p.trust,
        router_screen_name(),
        (g_state.cam_ready && g_state.cam_score >= 80) ? "true" : "false",
        g_state.cam_ready ? g_state.cam_score : -1,
        IP2STR(&ipi.ip), logs);

    esp_http_client_config_t cfg = {
        .url = url, .method = HTTP_METHOD_POST, .timeout_ms = 28000 };
    esp_http_client_handle_t cli = esp_http_client_init(&cfg);
    esp_http_client_set_header(cli, "Content-Type", "application/json");
    esp_wifi_set_ps(WIFI_PS_NONE);   /* think bodies carry camera frames */

    /* canonical open/write/fetch/read — perform() would consume the body */
    bool ok = false;
    static char resp[512];
    int blen = strlen(body);
    if (esp_http_client_open(cli, blen) == ESP_OK &&
        esp_http_client_write(cli, body, blen) == blen) {
        esp_http_client_fetch_headers(cli);
        int status = esp_http_client_get_status_code(cli);
        int len = 0, r;
        while (len < (int)sizeof(resp) - 1 &&
               (r = esp_http_client_read(cli, resp + len,
                                         sizeof(resp) - 1 - len)) > 0)
            len += r;
        if (status == 200 && len > 0) {
            resp[len] = 0;
            cJSON *j = cJSON_Parse(resp);
            cJSON *s = j ? cJSON_GetObjectItem(j, "say") : NULL;
            cJSON *e = j ? cJSON_GetObjectItem(j, "emotion") : NULL;
            cJSON *g = j ? cJSON_GetObjectItem(j, "glitch") : NULL;
            if (cJSON_IsString(s)) {
                snprintf(say, say_len, "%s", s->valuestring);
                snprintf(emotion, emo_len, "%s",
                         cJSON_IsString(e) ? e->valuestring : "talk");
                *glitch = cJSON_IsNumber(g) ? (uint8_t)g->valuedouble : 0;
                cJSON *ac = cJSON_GetObjectItem(j, "action");
                if (cJSON_IsString(ac))
                    strncpy(pending_action, ac->valuestring,
                            sizeof(pending_action) - 1);
                cJSON *au = cJSON_GetObjectItem(j, "audio");
                if (audio_len && cJSON_IsString(au) && au->valuestring[0]) {
                    /* relative path -> absolute on the brain's host */
                    const char *rel = au->valuestring;
                    char *slash = strstr(url, "://");
                    slash = slash ? strchr(slash + 3, '/') : NULL;
                    int base = slash ? (int)(slash - url) : (int)strlen(url);
                    snprintf(audio_url, audio_len, "%.*s%s", base, url, rel);
                }
                ok = true;
            }
            cJSON_Delete(j);
        } else {
            ESP_LOGW(TAG, "brain: status=%d len=%d", status, len);
        }
    }
    esp_http_client_cleanup(cli);
    /* don't re-nap the radio out from under an active voice stream */
    if (!g_state.voice_playing) esp_wifi_set_ps(WIFI_PS_MIN_MODEM);
    if (!ok) ESP_LOGW(TAG, "brain unreachable at %s", url);
    return ok;
}
