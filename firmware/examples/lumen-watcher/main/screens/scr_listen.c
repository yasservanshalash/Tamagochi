/* LISTENING — the character is present: attentive bust while you speak
 * (rings pulse behind him with live mic level), thought-bubble frames
 * while the query is in flight. Press = ask -> RESPONSE.               */
#include "router.h"
#include "theme.h"
#include "svc_assistant.h"
#include "svc_audio.h"
#include "svc_petlog.h"
#include "pet.h"
#include "esp_heap_caps.h"
#include <stdio.h>
#include "assets.h"
#include "ui_arcs.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "esp_heap_caps.h"
#include <stdatomic.h>
#include <string.h>

static lv_obj_t   *lb_transcript, *img_char, *lb_title;
static atomic_int  answer_ready;          /* 0 idle, 1 thinking, 2 done  */
static char        answer_buf[1024];

static void pulse_exec(void *var, int32_t v)
{
    lv_obj_t *o = (lv_obj_t *)var;
    int span = 150 + (g_state.mic_level * 9) / 10;      /* 150..240      */
    int size = (150 + ((v - 150) * span) / 220) & ~1;
    lv_obj_set_size(o, size, size);
    lv_obj_align(o, LV_ALIGN_CENTER, 0, -30);           /* behind bust   */
    lv_opa_t opa = (lv_opa_t)LV_CLAMP(0, 150 - (v - 150) * 150 / 220, 255);
    lv_obj_set_style_border_opa(o, opa, 0);
}

static void ring(lv_obj_t *parent, uint32_t delay)
{
    lv_obj_t *o = lv_obj_create(parent);
    lv_obj_set_style_radius(o, LV_RADIUS_CIRCLE, 0);
    lv_obj_set_style_bg_opa(o, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(o, 4, 0);
    lv_obj_set_style_border_color(o, C_ACCENT, 0);
    lv_obj_clear_flag(o, LV_OBJ_FLAG_CLICKABLE | LV_OBJ_FLAG_SCROLLABLE);
    lv_anim_t a;
    lv_anim_init(&a);
    lv_anim_set_var(&a, o);
    lv_anim_set_exec_cb(&a, pulse_exec);
    lv_anim_set_values(&a, 150, 370);
    lv_anim_set_time(&a, 1700);
    lv_anim_set_delay(&a, delay);
    lv_anim_set_repeat_count(&a, LV_ANIM_REPEAT_INFINITE);
    lv_anim_start(&a);
}

static char emo_buf[20], audio_buf[220], heard_buf[160];
static uint8_t glitch_buf;
static uint8_t *rec_pcm = NULL;
#define REC_MAX (16000 * 2 * 8)          /* 8 s @ 16 kHz mono s16 */
static int rec_bytes = 0;

/* Persistent worker: spawning a task at send-time fails whenever voice
 * playback has internal RAM pinned low (spawn needs 8KB contiguous).
 * Create once at screen-open (idle heap), park on a notify, reuse forever. */
static TaskHandle_t assist_worker = NULL;

static void query_run(void)
{
    bool ok;
    if (rec_pcm && rec_bytes > 8000) {          /* > ~0.25 s of speech  */
        ok = assistant_converse(rec_pcm, rec_bytes,
                                heard_buf, sizeof(heard_buf),
                                answer_buf, sizeof(answer_buf),
                                emo_buf, sizeof(emo_buf), &glitch_buf,
                                audio_buf, sizeof(audio_buf));
    } else {
        heard_buf[0] = 0;
        ok = assistant_think("talk_button", "", answer_buf,
                             sizeof(answer_buf), emo_buf, sizeof(emo_buf),
                             &glitch_buf, audio_buf, sizeof(audio_buf));
    }
    if (!ok) {
        audio_buf[0] = 0;
        snprintf(answer_buf, sizeof(answer_buf),
                 "brain unreachable. probably the pigeons.");
        strcpy(emo_buf, "suspicious");
        glitch_buf = 0;
    }
    atomic_store(&answer_ready, 2);
}

static void assist_worker_task(void *arg)
{
    (void)arg;
    for (;;) {
        ulTaskNotifyTake(pdTRUE, portMAX_DELAY);
        query_run();
    }
}

void scr_listen_prewarm(void)          /* boot-time stack reservation */
{
    if (!assist_worker &&
        xTaskCreate(assist_worker_task, "assist", 8192, NULL, 4,
                    &assist_worker) != pdPASS) {
        assist_worker = NULL;
        petlog("assist worker: boot create failed");
    }
}

static lv_obj_t *img_wave, *img_bubble;

static void vad_tick(void);              /* defined below poll_cb */
static void vad_reset(void);

static void think_cb(lv_timer_t *t)
{
    (void)t;
    if (atomic_load(&answer_ready) == 1) {
        lv_img_set_src(img_char, &img_y_expr3);          /* eyes-up ponder */
        lv_obj_align(img_char, LV_ALIGN_CENTER, 0, -26);
        lv_obj_clear_flag(img_bubble, LV_OBJ_FLAG_HIDDEN);
        lv_obj_align_to(img_bubble, img_char, LV_ALIGN_OUT_TOP_MID, 44, 34);
        lv_obj_add_flag(img_wave, LV_OBJ_FLAG_HIDDEN);
        return;
    }
    /* live mic -> waveform size */
    const lv_img_dsc_t *w = g_state.mic_level < 25 ? &img_fx_wave_s
                          : g_state.mic_level < 60 ? &img_fx_wave_m
                          : &img_fx_wave_l;
    lv_img_set_src(img_wave, w);
    lv_obj_align(img_wave, LV_ALIGN_BOTTOM_MID, 0, -108);
}

static void poll_cb(lv_timer_t *t)
{
    (void)t;
    if (atomic_load(&answer_ready) == 2) {
        atomic_store(&answer_ready, 0);
        if (heard_buf[0]) petlog("heard: \"%.40s\"", heard_buf);
        petlog("ai: %s g=%u (%d chars)", emo_buf, glitch_buf,
               (int)strlen(answer_buf));
        lumen_set_response_text(answer_buf);
        pet_react(emo_buf, glitch_buf,
                  (uint16_t)(1200 + 55 * strlen(answer_buf) > 8000 ? 8000
                             : 1200 + 55 * strlen(answer_buf)));
        pet_say(answer_buf, (uint16_t)(1500 + 50 * strlen(answer_buf)));
        if (audio_buf[0]) {
            svc_audio_play_url(audio_buf);
            router_home();               /* watch him say it            */
        } else {
            router_go(SCR_RESPONSE);     /* text-only: read it          */
        }
        return;
    }
    vad_tick();                          /* hands-free: send on silence */
}

static bool vad_armed;                  /* defined with the VAD below */

static void fire_query(void)
{
    if (atomic_load(&answer_ready) != 0) return;
    vad_armed = false;                  /* belt-and-suspenders: no re-fire */
    pet_user_touch();
    atomic_store(&answer_ready, 1);
    rec_bytes = svc_audio_record_stop();
    svc_audio_mic_stop();               /* quiesce i2s during upload+wait */
    {
        const int16_t *sp = (const int16_t *)rec_pcm;
        int n = rec_bytes / 2;
        uint64_t acc = 0;
        for (int i = 0; i < n; i += 8) acc += (int32_t)sp[i] * sp[i];
        int rms = n ? (int)__builtin_sqrtf((float)(acc / (n / 8 + 1))) : 0;
        petlog("listen: %d ms recorded, level %d -> thinking",
               rec_bytes / 32, rms);
    }
    lv_label_set_text(lb_title, "THINKING");
    lv_label_set_text(lb_transcript, "\xC2\xB7\xC2\xB7\xC2\xB7");
    if (assist_worker) {
        xTaskNotifyGive(assist_worker);
    } else {                                            /* deliver fallback */
        petlog("assist worker missing at send, int free %u",
               (unsigned)heap_caps_get_free_size(MALLOC_CAP_INTERNAL));
        audio_buf[0] = 0;
        snprintf(answer_buf, sizeof(answer_buf),
                 "my thinking task wouldn't start. low memory?");
        strcpy(emo_buf, "suspicious");
        glitch_buf = 0;
        atomic_store(&answer_ready, 2);
    }
}

static void send_cb(lv_event_t *e) { (void)e; fire_query(); }

/* hands-free end-of-speech: fire once you go quiet, no tap needed */
static bool     vad_spoke;              /* vad_armed declared above fire_query */
static uint32_t vad_silence, vad_t0;

static void vad_reset(void)
{
    vad_spoke = false;
    vad_armed = true;               /* one turn per screen; re-armed on reopen */
    vad_silence = 0;
    vad_t0 = lv_tick_get();
}

static void vad_tick(void)                  /* called from poll_cb @ 50ms */
{
    if (!vad_armed) return;                  /* already fired this turn     */
    if (atomic_load(&answer_ready) != 0) return;   /* send in flight        */
    uint32_t now = lv_tick_get();
    if (g_state.mic_level > 30) {           /* speaking                    */
        vad_spoke = true;
        vad_silence = 0;
    } else if (vad_spoke) {                  /* trailing silence           */
        vad_silence += 50;
        if (vad_silence >= 900 || now - vad_t0 > 8000) {
            vad_armed = false;               /* disarm BEFORE firing        */
            fire_query();                    /* end of turn -> respond      */
        }
    } else if (now - vad_t0 > 12000) {
        vad_armed = false;
        router_home();                       /* opened but never spoke      */
    }
}

static void del_timer_cb(lv_event_t *e)
{
    svc_audio_mic_stop();
    lv_timer_del((lv_timer_t *)lv_event_get_user_data(e));
}

lv_obj_t *scr_listen_create(void)
{
    lv_obj_t *scr = theme_screen_create();
    atomic_store(&answer_ready, 0);

    lb_title = theme_title(scr, "LISTENING");


    img_char = lv_img_create(scr);
    lv_img_set_src(img_char, &img_y_speak1);   /* hand to ear */
    lv_obj_align(img_char, LV_ALIGN_CENTER, 0, -30);
    lv_obj_clear_flag(img_char, LV_OBJ_FLAG_CLICKABLE);

    img_wave = lv_img_create(scr);
    lv_img_set_src(img_wave, &img_fx_wave_s);
    lv_obj_align(img_wave, LV_ALIGN_BOTTOM_MID, 0, -108);
    lv_obj_clear_flag(img_wave, LV_OBJ_FLAG_CLICKABLE);

    img_bubble = lv_img_create(scr);
    lv_img_set_src(img_bubble, &img_fx_think);
    lv_obj_add_flag(img_bubble, LV_OBJ_FLAG_HIDDEN);
    lv_obj_clear_flag(img_bubble, LV_OBJ_FLAG_CLICKABLE);

    lb_transcript = lv_label_create(scr);
    lv_obj_add_style(lb_transcript, &st_body, 0);
    lv_obj_set_width(lb_transcript, 300);
    lv_obj_set_style_text_align(lb_transcript, LV_TEXT_ALIGN_CENTER, 0);
    lv_label_set_text(lb_transcript, "listening\xC2\xB7\xC2\xB7\xC2\xB7");
    lv_obj_align(lb_transcript, LV_ALIGN_BOTTOM_MID, 0, -52);

    lv_obj_add_flag(scr, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(scr, send_cb, LV_EVENT_CLICKED, NULL);
    lv_group_add_obj(router_group(), scr);

    lv_obj_t *back = router_attach_back(scr);
    lv_group_add_obj(router_group(), back);

    pet_user_touch();
    petlog("listen: opened, recording");
    if (!assist_worker &&
        xTaskCreate(assist_worker_task, "assist", 8192, NULL, 4,
                    &assist_worker) != pdPASS) {
        assist_worker = NULL;    /* retried on every open until it lands */
        petlog("assist worker: create failed, int free %u",
               (unsigned)heap_caps_get_free_size(MALLOC_CAP_INTERNAL));
    }
    if (!rec_pcm) rec_pcm = heap_caps_malloc(REC_MAX, MALLOC_CAP_SPIRAM);
    if (svc_audio_mic_start()) {
        if (rec_pcm) svc_audio_record_start(rec_pcm, REC_MAX);
        vad_reset();                     /* start hands-free turn timing */
        lv_label_set_text(lb_transcript,
                          "just talk \xC2\xB7 i send when you pause");
    } else
        lv_label_set_text(lb_transcript, "mic unavailable \xC2\xB7 demo mode");

    lv_timer_t *t = lv_timer_create(poll_cb, 50, NULL);
    lv_obj_add_event_cb(scr, del_timer_cb, LV_EVENT_DELETE, t);
    lv_timer_t *t2 = lv_timer_create(think_cb, 140, NULL);
    lv_obj_add_event_cb(scr, del_timer_cb, LV_EVENT_DELETE, t2);
    return scr;
}
