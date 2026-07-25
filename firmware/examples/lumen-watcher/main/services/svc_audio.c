#include "svc_audio.h"
#include "svc_petlog.h"
#include "lumen.h"
#include "sensecap-watcher.h"
#include "esp_log.h"
#include "freertos/semphr.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include <math.h>
#include <string.h>
#include <stdlib.h>
#include "esp_http_client.h"
#include "esp_wifi.h"
#include "esp_heap_caps.h"
#include "freertos/stream_buffer.h"

static const char *TAG = "svc_audio";
static SemaphoreHandle_t codec_lock = NULL;   /* created in svc_audio_init */
static bool codec_ready = false;
static volatile bool mic_run = false;
static TaskHandle_t mic_task_h = NULL;
static uint8_t *rec_buf = NULL;          /* push-to-talk capture        */
static volatile int rec_len = 0, rec_max = 0;
static volatile bool rec_on = false;
static int mic_slot = -1;                /* which interleaved slot is live */

void svc_audio_init(void)                    /* call once from app_main */
{
    if (!codec_lock) codec_lock = xSemaphoreCreateMutex();
}

static bool ensure_codec(void)
{
    if (codec_ready) return true;
    if (!codec_lock) return false;           /* init not called yet     */
    xSemaphoreTake(codec_lock, portMAX_DELAY);
    if (!codec_ready) {
        if (bsp_codec_init() == ESP_OK) {
            bsp_codec_set_fs(16000, 16, I2S_SLOT_MODE_MONO);
            int set = 0;
            bsp_codec_volume_set(g_state.volume, &set);
            codec_ready = true;
        } else {
            ESP_LOGE(TAG, "codec init failed");
        }
    }
    xSemaphoreGive(codec_lock);
    return codec_ready;
}

void svc_audio_apply_volume(void)
{
    if (!ensure_codec()) return;          /* Sound row works standalone */
    int set = 0;
    bsp_codec_volume_set(g_state.volume, &set);
}

void svc_audio_beep(void)
{
    if (!ensure_codec()) return;
    /* 60 ms 1 kHz sine @16k mono — audible confirmation at new volume */
    static int16_t tone[960];
    static bool made = false;
    if (!made) {
        for (int i = 0; i < 960; i++) {
            float t = (float)i / 16000.f;
            float env = i < 80 ? i / 80.f : (i > 880 ? (960 - i) / 80.f : 1.f);
            tone[i] = (int16_t)(9000.f * env *
                                __builtin_sinf(2.f * 3.14159265f * 1000.f * t));
        }
        made = true;
    }
    size_t bw = 0;
    bsp_i2s_write(tone, sizeof(tone), &bw, 200);
}

static void mic_task(void *arg)
{
    (void)arg;
    static int16_t buf[512];              /* 32 ms @ 16 kHz mono */
    size_t br = 0;
    int reads = 0, empties = 0;
    while (mic_run) {
        esp_err_t rerr = bsp_i2s_read(buf, sizeof(buf), &br, 100);
        if (++reads == 1)
            petlog("mic: 1st read err=%d br=%u vp=%d",
                   (int)rerr, (unsigned)br, (int)g_state.voice_playing);
        if (rerr == ESP_OK && br > 0) {
            empties = 0;
            /* the record stream is 2-slot interleaved (channel:2): find
             * the slot carrying the mic once, then compact to true mono */
            const int16_t *sp = (const int16_t *)buf;
            int frames = (int)br / 4;
            if (mic_slot < 0 && frames > 32) {
                uint64_t e0 = 0, e1 = 0;
                for (int i = 0; i < frames; i++) {
                    e0 += (uint32_t)abs(sp[2 * i]);
                    e1 += (uint32_t)abs(sp[2 * i + 1]);
                }
                mic_slot = e1 > e0 ? 1 : 0;
                ESP_LOGI(TAG, "mic on slot %d", mic_slot);
            }
            int slot = mic_slot < 0 ? 0 : mic_slot;
            if (rec_on && rec_buf && rec_len + frames * 2 <= rec_max) {
                int16_t *dst = (int16_t *)(rec_buf + rec_len);
                for (int i = 0; i < frames; i++) dst[i] = sp[2 * i + slot];
                rec_len += frames * 2;
            }
            /* level from the live slot only */
            {
                uint64_t acc = 0;
                for (int i = 0; i < frames; i++) {
                    int32_t v = sp[2 * i + slot];
                    acc += v * v;
                }
                float rms = __builtin_sqrtf((float)(acc / (frames ? frames : 1)));
                float lvl = (rms - 120.f) / 60.f;
                if (lvl < 0) lvl = 0;
                lvl = 100.f * (1.f - __builtin_expf(-lvl * 0.10f));
                g_state.mic_level = (uint8_t)(lvl > 100 ? 100 : lvl);
            }
            continue;
        }
        if (0) {
            int n = 0; (void)n;
            uint64_t acc = 0;
            for (int i = 0; i < n; i++) acc += (int32_t)buf[i] * buf[i];
            float rms = sqrtf((float)(acc / (n ? n : 1)));
            /* map ~[60 .. 6000] rms to 0..100 with soft knee */
            float lvl = (rms - 60.f) / 60.f;             /* rough dB-ish */
            if (lvl < 0) lvl = 0;
            lvl = 100.f * (1.f - expf(-lvl * 0.09f));
            uint8_t out = (uint8_t)(lvl > 100 ? 100 : lvl);
            /* light smoothing */
            g_state.mic_level = (uint8_t)((g_state.mic_level * 2 + out) / 3);
        } else {
            if (++empties == 10)   /* ~1 s with zero mic data */
                petlog("mic: starved, err=%d vp=%d", (int)rerr,
                       (int)g_state.voice_playing);
            vTaskDelay(pdMS_TO_TICKS(20));
        }
    }
    g_state.mic_level = 0;
    mic_task_h = NULL;
    vTaskDelete(NULL);
}

/* ---- TTS playback ----------------------------------------------------
 * Download and playout are DECOUPLED through a 96 KB PSRAM ring
 * (~2 s of audio): Wi-Fi latency spikes drain the ring, not the DMA —
 * no more mid-sentence dropouts. Radio power-save is suspended for the
 * duration (modem naps add 100-300 ms bursts). Mouth RMS is computed at
 * PLAYOUT, so lips match what the ear hears.                            */
static char voice_url[200];
static void mic_wait_stopped(void);
/* ring sized to swallow a WHOLE typical reply (~11 s @ 22.05 kHz s16):
 * the downloader races ahead into PSRAM and playout drains from RAM, so
 * mid-clip Wi-Fi dips can't stutter speech. Prefill 64 KB (~1.5 s) rides
 * out startup jitter; 20 KB (0.45 s) chunked audibly on this link.      */
#define VRING_SIZE   (512 * 1024)
#define VRING_PREFILL (32 * 1024)   /* 1 s @ 16 kHz: faster speech onset */
static StreamBufferHandle_t vring = NULL;
static StaticStreamBuffer_t vring_struct;
static uint8_t *vring_store = NULL;
static volatile bool dl_done = false;
static int voice_rate = 16000;
static volatile int vring_owners = 0;    /* dl + play; last one frees   */
/* persistent workers: spawning tasks per-clip fails whenever internal
 * heap is pinched (self-talk went silent that way). Created once on the
 * first play, parked on a notify, reused for every clip afterwards.    */
static TaskHandle_t vdl_h = NULL, vplay_h = NULL;

static void voice_release(void)
{
    if (__atomic_sub_fetch(&vring_owners, 1, __ATOMIC_SEQ_CST) == 0) {
        vStreamBufferDelete(vring);
        vring = NULL;
        g_state.voice_playing = false;  /* voice session truly over */
        esp_wifi_set_ps(WIFI_PS_MIN_MODEM);
    }
}

static uint32_t rd_le32(const uint8_t *p)
{ return p[0] | (p[1]<<8) | (p[2]<<16) | ((uint32_t)p[3]<<24); }

static void voice_play_task(void *arg)
{
    (void)arg;
    static uint8_t buf[2048];
  for (;;) {
    ulTaskNotifyTake(pdTRUE, portMAX_DELAY);
    if (!vring) continue;                      /* stale notify */

    /* wait for prefill (or a short clip that finished downloading) */
    while (!dl_done && xStreamBufferBytesAvailable(vring) < VRING_PREFILL)
        vTaskDelay(pdMS_TO_TICKS(20));

    xSemaphoreTake(codec_lock, portMAX_DELAY);
    bsp_codec_set_fs(voice_rate, 16, I2S_SLOT_MODE_MONO);
    int set = 0;
    bsp_codec_volume_set(g_state.volume, &set);
    xSemaphoreGive(codec_lock);
    g_state.voice_playing = true;
    petlog("voice: playout start, %d Hz", voice_rate);

    int underruns = 0;
    for (;;) {
        size_t r = xStreamBufferReceive(vring, buf, sizeof(buf),
                                        pdMS_TO_TICKS(150));
        if (r == 0) {
            if (dl_done) break;
            underruns++;                   /* ring ran dry mid-clip      */
            /* rebuffer like a video player: ONE clean pause until a full
             * prefill is banked, instead of stuttering every syllable   */
            while (!dl_done &&
                   xStreamBufferBytesAvailable(vring) < VRING_PREFILL)
                vTaskDelay(pdMS_TO_TICKS(50));
            continue;
        }
        int n = r / 2;
        const int16_t *s = (const int16_t *)buf;
        uint64_t acc = 0;
        for (int i = 0; i < n; i++) acc += (int32_t)s[i] * s[i];
        float rms = __builtin_sqrtf((float)(acc / (n ? n : 1)));
        float lvl = (rms - 120.f) / 60.f;
        if (lvl < 0) lvl = 0;
        lvl = 100.f * (1.f - __builtin_expf(-lvl * 0.10f));
        g_state.voice_level = (uint8_t)(lvl > 100 ? 100 : lvl);
        size_t bw = 0;
        bsp_i2s_write((void *)buf, r, &bw, 1000);
    }

    g_state.voice_level = 0;
    g_state.voice_playing = false;
    xSemaphoreTake(codec_lock, portMAX_DELAY);
    bsp_codec_set_fs(16000, 16, I2S_SLOT_MODE_MONO);   /* mic default */
    xSemaphoreGive(codec_lock);
    petlog("voice: playout done, %d underruns", underruns);
    voice_release();
  }
}

static void voice_dl_task(void *arg)
{
    (void)arg;
    static uint8_t buf[2048];
  for (;;) {
    ulTaskNotifyTake(pdTRUE, portMAX_DELAY);
    if (!vring) continue;                      /* stale notify */
    esp_http_client_config_t cfg = { .url = voice_url, .timeout_ms = 12000 };
    esp_http_client_handle_t cli = esp_http_client_init(&cfg);

    if (esp_http_client_open(cli, 0) != ESP_OK) goto done;
    esp_http_client_fetch_headers(cli);
    if (esp_http_client_get_status_code(cli) != 200) goto done;

    /* minimal RIFF walk: find fmt (rate) then data */
    {
        int got = 0;
        while (got < 12) {
            int r = esp_http_client_read(cli, (char *)buf + got, 12 - got);
            if (r <= 0) goto done;
            got += r;
        }
        if (memcmp(buf, "RIFF", 4) || memcmp(buf + 8, "WAVE", 4)) goto done;
        for (;;) {
            got = 0;
            while (got < 8) {
                int r = esp_http_client_read(cli, (char *)buf + got, 8 - got);
                if (r <= 0) goto done;
                got += r;
            }
            uint32_t sz = rd_le32(buf + 4);
            if (!memcmp(buf, "data", 4)) break;
            bool is_fmt = !memcmp(buf, "fmt ", 4);
            uint32_t skip = sz;
            while (skip) {
                int chunk = skip > sizeof(buf) ? sizeof(buf) : skip;
                int r = esp_http_client_read(cli, (char *)buf, chunk);
                if (r <= 0) goto done;
                if (is_fmt && skip == sz && r >= 8)
                    voice_rate = rd_le32(buf + 4);
                skip -= r;
            }
        }
    }

    ESP_LOGI(TAG, "voice: %d Hz stream", voice_rate);
    for (int i = 0; i < 300 && rec_on; i++)     /* let the human finish  */
        vTaskDelay(pdMS_TO_TICKS(100));
    mic_wait_stopped();                    /* codec can't serve both      */
    __atomic_add_fetch(&vring_owners, 1, __ATOMIC_SEQ_CST);
    xTaskNotifyGive(vplay_h);              /* persistent worker, always up */

    for (;;) {
        int r = esp_http_client_read(cli, (char *)buf, sizeof(buf));
        if (r <= 0) break;
        if (xStreamBufferSend(vring, buf, r, pdMS_TO_TICKS(15000)) < (size_t)r) {
            petlog("voice: ring stalled -> abort stream");
            break;                         /* playout died; don't wedge   */
        }
    }

done:
    dl_done = true;
    voice_release();
    esp_http_client_cleanup(cli);
  }
}

bool svc_audio_play_url(const char *url)
{
    if (!ensure_codec() || g_state.voice_playing) return false;
    if (vring) { petlog("voice: busy (prev teardown), dropping clip"); return false; }
    if (!vring_store)                          /* +8: FreeRTOS stream rings
                                                  need size+1 slack (kernel
                                                  adds it itself only when
                                                  IT allocates)            */
        vring_store = heap_caps_malloc(VRING_SIZE + 8, MALLOC_CAP_SPIRAM);
    if (!vring_store) return false;
    vring = xStreamBufferCreateStatic(VRING_SIZE, 1, vring_store,
                                      &vring_struct);
    dl_done = false;
    voice_rate = 16000;
    vring_owners = 1;                          /* the downloader          */
    esp_wifi_set_ps(WIFI_PS_NONE);         /* radio fully awake to talk  */
    strncpy(voice_url, url, sizeof(voice_url) - 1);
    voice_url[sizeof(voice_url) - 1] = 0;
    if (!vdl_h && xTaskCreatePinnedToCore(voice_dl_task, "lumen_vdl", 8192,
                                          NULL, 5, &vdl_h, 0) != pdPASS)
        vdl_h = NULL;                      /* retried on next clip        */
    if (!vplay_h && xTaskCreatePinnedToCore(voice_play_task, "lumen_vplay",
                                            6144, NULL, 5, &vplay_h, 0) != pdPASS)
        vplay_h = NULL;
    if (!vdl_h || !vplay_h) {
        petlog("voice: worker create failed, int free %u",
               (unsigned)heap_caps_get_free_size(MALLOC_CAP_INTERNAL));
        voice_release();       /* owners 1->0: ring freed, PS restored,
                                  vring NULLed — never wedge future voice */
        return false;
    }
    /* mark the session ACTIVE from acceptance, not first sample: during
     * the download+prefill gap brain_cb saw !voice_playing and executed
     * post-speech actions (listen) INTO the start of the speech.        */
    g_state.voice_playing = true;
    xTaskNotifyGive(vdl_h);
    return true;
}

void svc_audio_prewarm_voice(void)     /* boot-time stack reservation */
{
    if (!vdl_h && xTaskCreatePinnedToCore(voice_dl_task, "lumen_vdl", 8192,
                                          NULL, 5, &vdl_h, 0) != pdPASS)
        vdl_h = NULL;
    if (!vplay_h && xTaskCreatePinnedToCore(voice_play_task, "lumen_vplay",
                                            6144, NULL, 5, &vplay_h, 0) != pdPASS)
        vplay_h = NULL;
    if (!vdl_h || !vplay_h) petlog("voice workers: boot create failed");
}

bool svc_audio_mic_start(void)
{
    if (!ensure_codec()) { petlog("mic_start: codec unavailable"); return false; }
    if (mic_task_h) { petlog("mic_start: task already running"); return true; }
    mic_run = true;
    return xTaskCreatePinnedToCore(mic_task, "lumen_mic", 4096, NULL, 5,
                                   &mic_task_h, 0) == pdPASS;
}

void svc_audio_mic_stop(void) { mic_run = false; rec_on = false; mic_slot = -1; }

void svc_audio_record_start(uint8_t *buf, int max)
{ rec_buf = buf; rec_max = max; rec_len = 0; rec_on = true; }

int svc_audio_record_stop(void)
{ rec_on = false; return rec_len; }

static void mic_wait_stopped(void)
{
    svc_audio_mic_stop();
    for (int i = 0; i < 30 && mic_task_h; i++) vTaskDelay(pdMS_TO_TICKS(20));
}
