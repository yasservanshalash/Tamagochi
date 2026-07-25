#include "svc_petlog.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include <stdarg.h>
#include <stdio.h>
#include <string.h>
#include "esp_attr.h"

#define TRAIL_MAGIC 0x59A55E12
typedef struct { uint32_t magic; uint8_t idx; char l[8][72]; } trail_t;
static RTC_NOINIT_ATTR trail_t trail;

#define N    48
#define LEN  72
static char ring[N][LEN];
static volatile int head = 0, count = 0;
static portMUX_TYPE mux = portMUX_INITIALIZER_UNLOCKED;

void petlog(const char *fmt, ...)
{
    char msg[56];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(msg, sizeof(msg), fmt, ap);
    va_end(ap);

    uint32_t s = (uint32_t)(esp_timer_get_time() / 1000000);
    taskENTER_CRITICAL(&mux);
    snprintf(ring[head], LEN, "[%02lu:%02lu:%02lu] %s",
             (unsigned long)(s/3600), (unsigned long)((s/60)%60),
             (unsigned long)(s%60), msg);
    head = (head + 1) % N;
    if (count < N) count++;
    strncpy(trail.l[trail.idx % 8], ring[(head + N - 1) % N], 72);
    trail.idx++;
    trail.magic = TRAIL_MAGIC;
    taskEXIT_CRITICAL(&mux);
    ESP_LOGI("pet", "%s", msg);
}

void petlog_boot_recover(void)
{
    if (trail.magic == TRAIL_MAGIC) {
        petlog("---- pre-crash trail ----");
        for (int i = 0; i < 8; i++) {
            const char *s = trail.l[(trail.idx + i) % 8];
            if (s[0]) petlog("was: %.48s", s + (s[0]=='[' ? 11 : 0));
        }
    }
    trail.magic = TRAIL_MAGIC;
    trail.idx = 0;
    memset(trail.l, 0, sizeof(trail.l));
}

int petlog_tail(char *out, int cap, int n)
{
    /* last n lines, sanitized for JSON embedding, '|' separated */
    int written = 0;
    taskENTER_CRITICAL(&mux);
    int c2 = count < n ? count : n, h = head;
    for (int i = 0; i < c2 && written < cap - 60; i++) {
        const char *src = ring[(h - c2 + i + N) % N];
        if (i) out[written++] = '|';
        for (const char *p = src; *p && written < cap - 2; p++)
            out[written++] = (*p == '"' || *p == '\\') ? '\'' : *p;
    }
    taskEXIT_CRITICAL(&mux);
    out[written] = 0;
    return written;
}

int petlog_dump(char *out, int cap)
{
    int n = 0;
    taskENTER_CRITICAL(&mux);
    int c = count, h = head;
    taskEXIT_CRITICAL(&mux);
    for (int i = 0; i < c && n < cap - LEN - 2; i++) {
        int idx = (h - c + i + N) % N;
        n += snprintf(out + n, cap - n, "%s\n", ring[idx]);
    }
    return n;
}
