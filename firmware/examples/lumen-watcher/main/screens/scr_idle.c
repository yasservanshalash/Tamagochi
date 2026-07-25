/* HOME v4 — full Yasser library (amber pixel art) + FX overlay system.
 * Clips carry optional floating FX (speech/think bubbles, hearts, notes,
 * zzz, "!?") anchored above the character. pet_react() speaks the whole
 * emotional vocabulary; every decision goes to the pet log.             */
#include "router.h"
#include "theme.h"
#include "ui_arcs.h"
#include "assets.h"
#include "pet.h"
#include "svc_petlog.h"
#include "pet_persona.h"
#include "svc_assistant.h"
#include "svc_audio.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "esp_heap_caps.h"
#include <stdio.h>
#include <string.h>
#include <time.h>
#include <stdlib.h>
#include <stdatomic.h>

typedef struct { const lv_img_dsc_t *img; uint16_t ms; int8_t dx, dy; } frame_t;
typedef struct {
    const frame_t *f; uint8_t n; uint8_t loop;
    const lv_img_dsc_t *const *fx; uint8_t fxn; uint16_t fx_ms;
} clip_t;
#define F(i,m,x,y) { &(i),(m),(x),(y) }

static const frame_t f_idle[]  = { F(img_y_sitmug,900,0,0), F(img_y_sitb,700,0,0),
                                   F(img_y_sitc,520,0,0),   F(img_y_sitb,700,0,0) };
static const frame_t f_blink[] = { F(img_y_blinkh,60,0,0),  F(img_y_blink,110,0,0),
                                   F(img_y_blinkh,60,0,0) };
static const frame_t f_talk[]  = { F(img_y_talk0,100,0,0), F(img_y_talk1,90,0,0),
                                   F(img_y_talk2,100,0,0), F(img_y_talk3,120,0,0),
                                   F(img_y_talk2,90,0,0),  F(img_y_talk1,90,0,0) };
static const frame_t f_think[] = { F(img_y_expr3,800,0,0), F(img_y_expr3,800,0,1) };
static const frame_t f_poss[]  = { F(img_y_pos0,140,2,0),  F(img_y_pos1,140,-2,0),
                                   F(img_y_pos2,140,1,0),  F(img_y_pos3,140,-1,0),
                                   F(img_y_pos4,140,2,0) };
static const frame_t f_wave[]  = { F(img_y_act0,150,0,0),  F(img_y_act1,140,0,0),
                                   F(img_y_act0,150,0,0),  F(img_y_act1,140,0,0),
                                   F(img_y_act0,150,0,0),  F(img_y_act1,150,0,0) };
static const frame_t f_laugh[] = { F(img_y_expr2,1700,0,0) };
static const frame_t f_sad[]   = { F(img_y_expr0,1300,0,0), F(img_y_expr0,1300,0,1) };
static const frame_t f_scare[] = { F(img_y_expr1,1500,0,0) };
static const frame_t f_sus[]   = { F(img_y_expr4,1800,0,0) };
static const frame_t f_whis[]  = { F(img_y_speak2,900,0,0), F(img_y_speak2,900,0,1) };
static const frame_t f_point[] = { F(img_y_speak0,1500,0,0) };
static const frame_t f_shrug[] = { F(img_y_act2,1400,0,0) };
static const frame_t f_palm[]  = { F(img_y_act3,1700,0,0) };
static const frame_t f_cross[] = { F(img_y_act4,1200,0,0), F(img_y_act4,1200,1,0) };
static const frame_t f_jump[]  = { F(img_y_big0,200,0,-6), F(img_y_big1,190,0,30),
                                   F(img_y_big2,220,0,2),  F(img_y_big0,140,0,-2) };
static const frame_t f_dance[] = { F(img_y_big3,240,-3,0), F(img_y_big4,240,3,0) };
static const frame_t f_strch[] = { F(img_y_stretch,2200,0,0) };
static const frame_t f_pad[]   = { F(img_y_sitpad,6000,0,0) };
static const frame_t f_zzz[]   = { F(img_y_zsleep,1400,0,0), F(img_y_zsleep,1400,0,1) };

static const lv_img_dsc_t *const FX_THINK[]  = { &img_fx_think };
static const lv_img_dsc_t *const FX_HEART[]  = { &img_fx_heart };
static const lv_img_dsc_t *const FX_DROP[]   = { &img_fx_drop };
static const lv_img_dsc_t *const FX_BANGQ[]  = { &img_fx_bangq };
static const lv_img_dsc_t *const FX_ANGER[]  = { &img_fx_anger };
static const lv_img_dsc_t *const FX_NOTES[]  = { &img_fx_note1, &img_fx_note2 };

static const clip_t C_IDLE  = { f_idle,4,1, NULL,0,0 };
static const clip_t C_BLINK = { f_blink,3,0, NULL,0,0 };
static const clip_t C_TALK  = { f_talk,6,1, NULL,0,0 };   /* mouth carries it */
static const clip_t C_THINK = { f_think,2,1, FX_THINK,1,0 };
static const clip_t C_POSS  = { f_poss,5,1, NULL,0,0 };
static const clip_t C_WAVE  = { f_wave,6,0, NULL,0,0 };
static const clip_t C_LAUGH = { f_laugh,1,0, FX_HEART,1,0 };
static const clip_t C_SAD   = { f_sad,2,1, FX_DROP,1,0 };
static const clip_t C_SCARE = { f_scare,1,0, FX_BANGQ,1,0 };
static const clip_t C_SUS   = { f_sus,1,0, NULL,0,0 };
static const clip_t C_WHIS  = { f_whis,2,1, NULL,0,0 };
static const clip_t C_POINT = { f_point,1,0, NULL,0,0 };
static const clip_t C_SHRUG = { f_shrug,1,0, NULL,0,0 };
static const clip_t C_PALM  = { f_palm,1,0, NULL,0,0 };
static const clip_t C_CROSS = { f_cross,2,1, FX_ANGER,1,0 };
static const clip_t C_JUMP  = { f_jump,4,0, NULL,0,0 };
static const clip_t C_DANCE = { f_dance,2,1, FX_NOTES,2,380 };
static const clip_t C_STRCH = { f_strch,1,0, NULL,0,0 };
static const clip_t C_PAD   = { f_pad,1,0, NULL,0,0 };
static const clip_t C_SLEEP = { f_zzz,2,1, NULL,0,0 };

static lv_obj_t *lb_clock, *arc_batt, *ic_wifi, *img_char, *img_fx, *lb_sub;
static int sub_ms = 0;
static const clip_t *base = &C_IDLE, *shot = NULL;
static uint8_t fidx = 0, fx_idx = 0;
static int fms = 0, hold = 0, fx_ms_left = 0;
static uint32_t person_since = 0, person_gone_at = 0;
static bool person_here = false;
static uint32_t person_cd = 0, blink_in = 3000, vign_in = 45000,
                amb_glitch_in = 200000, grump_in = 0;
static bool was_charging = false;

static atomic_int pend_e = -1, pend_g = 0, pend_h = 0, pend_ai = 0;
static int  last_emo = 0;
static int  intro_ms = 0;               /* emotion shows before mouth   */
static bool prev_voice = false;
static const lv_img_dsc_t *const *talk_fx = NULL;
static uint8_t talk_fxn = 0;
static uint32_t sleep_since = 0, self_talk_in = 90000;
static atomic_uint user_touch = 0;
static uint8_t  out_stage = 0;           /* outreach ritual: 0 off, 1..3 */
static int      out_ms = 0;
static uint32_t out_touch_snap = 0;
static bool     boot_greet_done = false;

void pet_user_touch(void) { atomic_store(&user_touch, lv_tick_get()); }
static void spawn_thought(const char *reason);

static void start_outreach(const char *event)
{
    out_touch_snap = atomic_load(&user_touch);
    out_stage = 1;
    out_ms = 25000;
    spawn_thought(event);
}
#define ENGAGED_MS   (10u * 60 * 1000)
#define NAP_AFTER_MS (20u * 60 * 1000)
#define LONG_SLEEP_MS (45u * 60 * 1000)

static const struct { const char *name; const clip_t *clip; bool is_base; int def_hold; }
EMO[] = {
    { "idle",      &C_IDLE,  true,  0    },
    { "happy",     &C_WAVE,  false, 0    },
    { "talk",      &C_TALK,  true,  2600 },
    { "think",     &C_THINK, true,  4000 },
    { "glitch",    &C_POSS,  true,  2200 },
    { "dance",     &C_DANCE, true,  5200 },
    { "celebrate", &C_DANCE, true,  5200 },
    { "jump",      &C_JUMP,  false, 0    },
    { "confused",  &C_SHRUG, false, 0    },
    { "sad",       &C_SAD,   true,  3500 },
    { "scared",    &C_SCARE, false, 0    },
    { "laugh",     &C_LAUGH, false, 0    },
    { "suspicious",&C_SUS,   false, 0    },
    { "whisper",   &C_WHIS,  true,  3000 },
    { "point",     &C_POINT, false, 0    },
    { "facepalm",  &C_PALM,  false, 0    },
    { "grumpy",    &C_CROSS, true,  3000 },
    { "stretch",   &C_STRCH, false, 0    },
    { "busy",      &C_PAD,   true,  6000 },
};
#define EMO_N (sizeof(EMO)/sizeof(EMO[0]))

static char     pend_say[200];
static int      pend_say_ms = 0;
static atomic_int say_flag = 0;

void pet_say(const char *text, uint16_t hold_ms)
{
    /* callable from ANY task, ANY screen: queued, consumed by the home
     * screen's brain tick when the subtitle widget actually exists.    */
    strncpy(pend_say, text, sizeof(pend_say) - 1);
    pend_say[sizeof(pend_say) - 1] = 0;
    pend_say_ms = hold_ms ? hold_ms : 4000;
    atomic_store(&say_flag, 1);
}

static atomic_int brain_busy = 0;

static void thought_task(void *arg)
{
    static char say[200], emo[20], audio[220];
    uint8_t gl = 0;
    if (assistant_think(arg ? (const char *)arg : "idle_thought",
                        "", say, sizeof(say), emo, sizeof(emo), &gl,
                        audio, sizeof(audio))) {
        petlog("ai: %s g=%u \"%.28s\"", emo, gl, say);
        atomic_store(&pend_ai, 1);
        pet_react(emo, gl, 300 + 60 * strlen(say) > 8000 ? 8000
                          : 300 + 60 * strlen(say));
        pet_say(say, 1200 + 55 * strlen(say));
        if (audio[0]) svc_audio_play_url(audio);   /* engine choreographs */
    } else {
        petlog("ai: unreachable");
        pet_react("facepalm", 0, 0);
        pet_say("brain unreachable \xC2\xB7 check server + /brain", 4500);
    }
    atomic_store(&brain_busy, 0);
    vTaskDelete(NULL);
}

static uint32_t brain_busy_since = 0;

static void spawn_thought(const char *reason)
{
    uint32_t now = lv_tick_get();
    if (atomic_exchange(&brain_busy, 1)) {
        /* self-heal: a thought that died abnormally must not gag him    */
        if (now - brain_busy_since < 60000) return;
        petlog("brain guard was wedged \xC2\xB7 releasing");
    }
    brain_busy_since = now;
    xTaskCreate(thought_task, "thought", 8192, (void *)reason, 4, NULL);
}

void pet_react(const char *em, uint8_t glitch, uint16_t hold_ms)
{
    int e = 0;
    for (int i = 0; i < (int)EMO_N; i++)
        if (!strcmp(em, EMO[i].name)) { e = i; break; }
    atomic_store(&pend_g, glitch);
    atomic_store(&pend_h, hold_ms);
    atomic_store(&pend_e, e);
}

static void apply_fx(void)
{
    const clip_t *c = shot ? shot : base;
    const lv_img_dsc_t *const *fx = c->fx;
    uint8_t fxn = c->fxn;
    if (c == &C_TALK && talk_fx) { fx = talk_fx; fxn = talk_fxn; }
    if (!fx) { lv_obj_add_flag(img_fx, LV_OBJ_FLAG_HIDDEN); return; }
    lv_img_set_src(img_fx, fx[fx_idx % fxn]);
    lv_obj_clear_flag(img_fx, LV_OBJ_FLAG_HIDDEN);
    lv_obj_align_to(img_fx, img_char, LV_ALIGN_OUT_TOP_MID, 44, 34);
}

static void set_frame(void)
{
    const clip_t *c = shot ? shot : base;
    const frame_t *fr = &c->f[fidx];
    lv_img_set_src(img_char, fr->img);
    lv_obj_align(img_char, LV_ALIGN_BOTTOM_MID, fr->dx, -26 - fr->dy);
    fms = fr->ms;
    apply_fx();
}

static void play_shot(const clip_t *c) { shot = c; fidx = 0; fx_idx = 0; set_frame(); }
static void set_base(const clip_t *c)  { base = c; shot = NULL; fidx = 0; fx_idx = 0; set_frame(); }

static void anim_cb(lv_timer_t *t)
{
    (void)t;
    if (!shot && base == &C_TALK && g_state.voice_playing) {
        static const lv_img_dsc_t *VIS[4] =
            { &img_y_talk0, &img_y_talk1, &img_y_talk2, &img_y_talk3 };
        static uint8_t last = 255;
        uint8_t v = g_state.voice_level < 8  ? 0 :
                    g_state.voice_level < 30 ? 1 :
                    g_state.voice_level < 60 ? 2 : 3;
        if (v != last) { last = v; lv_img_set_src(img_char, VIS[v]); }
        fms = 60;
        return;
    }
    const clip_t *c = shot ? shot : base;
    if (c->fx && c->fxn > 1) {
        fx_ms_left -= 30;
        if (fx_ms_left <= 0) { fx_ms_left = c->fx_ms; fx_idx++; apply_fx(); }
    }
    fms -= 30;
    if (fms > 0) return;
    fidx++;
    if (fidx >= c->n) {
        if (shot) { shot = NULL; fidx = 0; }
        else if (c->loop) fidx = 0;
        else fidx = c->n - 1;
    }
    set_frame();
}

static bool is_night(void)
{
    if (!g_state.time_valid) return false;
    time_t now = time(NULL); struct tm ti; localtime_r(&now, &ti);
    return ti.tm_hour >= 22 || ti.tm_hour < 8;
}

static void do_emotion(int e, int gl, int hold_ms)
{
    last_emo = e;
    bool from_ai = atomic_exchange(&pend_ai, 0);
    petlog("react: %s (glitch=%d)", EMO[e].name, gl);
    if (base == &C_SLEEP) {
        if (from_ai) return;      /* his own reply is not a wake-up call */
        uint32_t slept = lv_tick_get() - sleep_since;
        set_base(&C_IDLE); play_shot(&C_SCARE); hold = 8000;
        if (slept > LONG_SLEEP_MS) {
            petlog("woken after %lu min -> PANIC", (unsigned long)(slept/60000));
            start_outreach("long_sleep_wake");
        } else {
            petlog("woken up -> greeting");
            start_outreach("wake_greet");
        }
        return;
    }
    if (gl >= 60 && EMO[e].clip != &C_POSS) play_shot(&C_POSS);
    if (EMO[e].is_base) {
        set_base(EMO[e].clip);
        hold = hold_ms ? hold_ms : EMO[e].def_hold;
    } else {
        play_shot(EMO[e].clip);
    }
}

/* ---- "hey yasser" wake: idle mic + server name-check ----------------
 * No on-device wake model exists for a custom name, so: watch the mic
 * level for a speech burst, ship the clip to the brain (LOCAL whisper,
 * no LLM cost), and only wake into a conversation when the name is in
 * it. Gated hard on !voice_playing so he can't wake himself.           */
#define WAKE_MAX (16000 * 2 * 3)            /* 3 s @ 16 kHz mono s16     */
static uint8_t     *wake_pcm = NULL;
static int          wake_state = 0;         /* 0 off, 1 armed, 2 capture */
static uint32_t     wake_t0 = 0, wake_quiet = 0;
static int          wake_len = 0;
static atomic_int   wake_hit = 0;           /* worker -> brain_cb        */
static TaskHandle_t wake_worker = NULL;

static void wake_worker_task(void *arg)
{
    (void)arg;
    for (;;) {
        ulTaskNotifyTake(pdTRUE, portMAX_DELAY);
        char heard[96] = "";
        bool woke = assistant_wake_check(wake_pcm, wake_len,
                                         heard, sizeof(heard));
        if (heard[0] || woke)
            petlog("wake: \"%.60s\" -> %s", heard, woke ? "MATCH" : "no");
        if (woke) atomic_store(&wake_hit, 1);
    }
}

static void wake_poll(uint32_t now)
{
    if (atomic_exchange(&wake_hit, 0)) {
        pet_user_touch();
        pet_react("confused", 0, 1200);     /* ?! moment, then ears on   */
        router_go(SCR_LISTEN);
        wake_state = 0;
        return;
    }
    if (!g_state.wifi_connected || g_state.voice_playing ||
        base == &C_SLEEP || atomic_load(&brain_busy)) {
        if (wake_state == 2) svc_audio_record_stop();
        wake_state = 0;
        return;
    }
    switch (wake_state) {
    case 0:
        if (!wake_pcm)
            wake_pcm = heap_caps_malloc(WAKE_MAX, MALLOC_CAP_SPIRAM);
        if (!wake_worker &&
            xTaskCreate(wake_worker_task, "wake", 6144, NULL, 3,
                        &wake_worker) != pdPASS)
            wake_worker = NULL;
        if (wake_pcm && wake_worker && svc_audio_mic_start())
            wake_state = 1;
        break;
    case 1:
        if (g_state.mic_level > 22) {      /* trip early: keep 1st syllable */
            svc_audio_record_start(wake_pcm, WAKE_MAX);
            wake_t0 = now; wake_quiet = 0; wake_state = 2;
        }
        break;
    case 2:
        wake_quiet = g_state.mic_level < 12 ? wake_quiet + 100 : 0;
        if (wake_quiet >= 800 || now - wake_t0 > 3000) {
            wake_len = svc_audio_record_stop();
            wake_state = 1;
            if (wake_len > 6000 && wake_worker)   /* >0.19 s of speech */
                xTaskNotifyGive(wake_worker);
        }
        break;
    }
}

static void brain_cb(lv_timer_t *t)
{
    (void)t;
    wake_poll(lv_tick_get());
    if (atomic_exchange(&say_flag, 0) && lb_sub) {
        lv_label_set_text(lb_sub, pend_say);
        lv_obj_clear_flag(lb_sub, LV_OBJ_FLAG_HIDDEN);
        sub_ms = pend_say_ms;
    }
    int e = atomic_exchange(&pend_e, -1);
    if (e >= 0) { do_emotion(e, atomic_load(&pend_g), atomic_load(&pend_h)); return; }

    if (g_state.charging != was_charging) {
        was_charging = g_state.charging;
        if (g_state.charging && base != &C_SLEEP) {
            petlog("charger plugged -> jump!"); play_shot(&C_JUMP); return;
        }
    }
    /* ---- speech choreography: emotion intro, then mouth follows ---- */
    if (g_state.voice_playing != prev_voice) {
        prev_voice = g_state.voice_playing;
        /* fewer flushes while audio DMA is hot: collision windows halve */
        lv_timer_set_period(_lv_disp_get_refr_timer(lv_disp_get_default()),
                            prev_voice ? 60 : 15);
        if (prev_voice) {
            const clip_t *ec = EMO[last_emo].clip;
            bool emotive = ec != &C_TALK && ec != &C_IDLE;
            intro_ms = emotive ? 1500 : 0;
            talk_fx  = emotive ? ec->fx  : NULL;
            talk_fxn = emotive ? ec->fxn : 0;
        } else { talk_fx = NULL; talk_fxn = 0; }
    }
    if (g_state.voice_playing) {
        if (intro_ms > 0) intro_ms -= 100;
        else if (!shot && base != &C_TALK && base != &C_POSS)
            set_base(&C_TALK);           /* possessed speech stays possessed */
        if (base == &C_TALK || base == &C_POSS) hold = 500;
    }
    if (hold > 0) {
        hold -= 100;
        if (hold <= 0) {
            if (g_state.voice_playing) set_base(&C_TALK);
            else if (is_night()) { petlog("hold done -> sleep"); set_base(&C_SLEEP); }
            else                 { petlog("hold done -> idle");  set_base(&C_IDLE);  }
        }
        return;
    }
    if (base == &C_SLEEP) {
        if (!is_night()) {
            petlog("morning -> stretch, wake");
            set_base(&C_IDLE); play_shot(&C_STRCH);
        }
        return;
    }
    if (is_night()) {
        petlog("22:00 -> stretch, sleep");
        sleep_since = lv_tick_get();
        base = &C_SLEEP; shot = &C_STRCH; fidx = 0; set_frame(); return;
    }
    /* ---- boot greeting: he speaks first ----------------------------- */
    if (!boot_greet_done && g_state.wifi_connected &&
        lv_tick_get() > 12000 && lb_sub) {
        boot_greet_done = true;
        if (!is_night()) {              /* greet on EVERY boot once online */
            petlog("online -> greeting");
            start_outreach("wake_greet");
        } else petlog("night boot -> staying quiet");
    }
    /* ---- outreach ritual: spoke first, now waiting for you ---------- */
    if (out_stage) {
        if (atomic_load(&user_touch) != out_touch_snap) {
            petlog("they responded \xC2\xB7 ritual over");
            out_stage = 0;
        } else if (!g_state.voice_playing && !atomic_load(&brain_busy)) {
            out_ms -= 100;
            if (out_ms <= 0) {
                out_ms = 25000;
                if (out_stage == 1) {
                    out_stage = 2;
                    petlog("no response -> uneasy");
                    spawn_thought("no_response_1");
                } else {
                    out_stage = 0;
                    petlog("still nothing -> standing by");
                    spawn_thought("no_response_2");
                }
            }
        }
    }
    /* ---- act on the mind's chosen verb once he's done speaking ------ */
    if (!g_state.voice_playing && !atomic_load(&brain_busy)) {
        const char *act = assistant_take_action();
        if (act[0] && strcmp(act, "none")) {
            petlog("mind chose: %s", act);
            if (!strcmp(act, "camera"))      router_go(SCR_CAMERA);
            else if (!strcmp(act, "listen")) router_go(SCR_LISTEN);
            else if (!strcmp(act, "sleep")) { sleep_since = lv_tick_get();
                                              set_base(&C_SLEEP); }
            /* "home": already here */
        }
    }
    /* ---- presence: think aloud while you're around, nap when ignored  */
    {
        uint32_t now2 = lv_tick_get();
        uint32_t since = now2 - atomic_load(&user_touch);
        if (since > NAP_AFTER_MS && !g_state.voice_playing && !shot &&
            !out_stage) {
            petlog("ignored %lu min -> nap", (unsigned long)(since/60000));
            sleep_since = now2;
            set_base(&C_SLEEP);
            return;
        }
        if (since < ENGAGED_MS && g_state.wifi_connected && lb_sub &&
            !g_state.voice_playing && !out_stage) {
            if (self_talk_in > 100) self_talk_in -= 100;
            else {
                self_talk_in = 60000 + rand() % 90000;   /* 1-2.5 min    */
                petlog("self-talk");
                spawn_thought("self_talk");
            }
        }
    }
    if (shot) return;

    if (g_state.cam_ready && g_state.cam_score >= 85) {
        uint32_t now = lv_tick_get();
        if (!person_here) {
            person_here = true;
            person_since = now;
            petlog("camera: someone's here (%d%%)", g_state.cam_score);
            pet_persona_event("person");
            play_shot(&C_WAVE);
            static uint32_t greet_cd = 0;
            if (now - greet_cd > 600000) {       /* 10 min between       */
                greet_cd = now;
                spawn_thought("person_seen");
            }
            return;
        }
        person_gone_at = 0;
    } else if (person_here) {
        uint32_t now = lv_tick_get();
        if (!person_gone_at) person_gone_at = now;
        else if (now - person_gone_at > 15000) {
            person_here = false;
            if (now - person_since > 60000) {
                petlog("camera: they left");
                spawn_thought("person_left");
            }
        }
    }
    if (g_state.battery_pct < 15 && !g_state.charging) {
        grump_in -= 100;
        if ((int)grump_in <= 0) {
            grump_in = 90000;
            petlog("battery %d%% -> sad", g_state.battery_pct);
            set_base(&C_SAD); hold = 3500; return;
        }
    }
    blink_in -= 100;
    if ((int)blink_in <= 0) { blink_in = 2400 + rand()%3200; play_shot(&C_BLINK); return; }
    vign_in -= 100;
    if ((int)vign_in <= 0) {
        vign_in = 35000 + rand()%60000;
        static const char *VN[] = { "controller", "stretch", "suspicious glance",
                                    "facepalm" };
        pet_persona_t pp = pet_persona_get();
        int v = rand()%4;
        if (pp.paranoia > 65 && rand()%2) v = 2;      /* paranoid: side-eye  */
        if (pp.energy < 30) v = 1;                    /* tired: stretch      */
        petlog("vignette: %s", VN[v]);
        switch (v) {
        case 0: set_base(&C_PAD);   hold = 6000; break;
        case 1: play_shot(&C_STRCH); break;
        case 2: play_shot(&C_SUS);   break;
        case 3: play_shot(&C_PALM);  break;
        }
        return;
    }
    amb_glitch_in -= 100;
    if ((int)amb_glitch_in <= 0) {
        amb_glitch_in = 150000 + rand()%240000;
        petlog("ambient possession");
        pet_persona_event("glitch");
        set_base(&C_POSS); hold = 1800;
    }
}

static uint32_t minute_acc = 0, thought_cd = 0;

static void tick_cb(lv_timer_t *t)
{
    (void)t;
    if (g_state.voice_playing) return;   /* no chrome redraws mid-speech */
    if (sub_ms > 0) {
        sub_ms -= 1000;
        if (sub_ms <= 0) lv_obj_add_flag(lb_sub, LV_OBJ_FLAG_HIDDEN);
    }
    if (++minute_acc >= 60) {
        minute_acc = 0;
        pet_persona_minute();
        pet_persona_t pp = pet_persona_get();
        thought_cd = thought_cd ? thought_cd - 1 : 0;
        if (!thought_cd && g_state.wifi_connected && base != &C_SLEEP &&
            (pp.boredom > 70 || (pp.paranoia > 75 && rand()%2))) {
            thought_cd = 7 + rand()%9;               /* 7-15 min between    */
            petlog("bored/paranoid -> spontaneous thought");
            pet_persona_event("talk");
            spawn_thought("spontaneous");
        }
    }
    if (g_state.time_valid) {
        time_t now = time(NULL); struct tm ti; localtime_r(&now, &ti);
        char b[8]; strftime(b, sizeof(b), "%H:%M", &ti);
        lv_label_set_text(lb_clock, b);
    } else lv_label_set_text(lb_clock, g_state.provisioning ? "SETUP" : "--:--");
    rim_arc_set(arc_batt, g_state.battery_pct);
    static bool pulse; pulse = !pulse;
    lv_obj_set_style_arc_opa(arc_batt,
        g_state.charging ? (pulse ? LV_OPA_COVER : LV_OPA_50) : LV_OPA_COVER,
        LV_PART_INDICATOR);
    icon_wifi_set_state(ic_wifi, g_state.wifi_connected);
}

static void char_click_cb(lv_event_t *e)
{
    (void)e;
    static uint32_t taps[4];
    static uint32_t last;
    uint32_t now = lv_tick_get();

    /* spam detector: 4 taps inside 2 s -> overwhelmed hallucination */
    taps[0] = taps[1]; taps[1] = taps[2]; taps[2] = taps[3]; taps[3] = now;
    if (now - taps[0] < 2000) {
        petlog("SPAM-poked -> overwhelmed");
        pet_persona_event("noise");
        pet_react("glitch", 85, 2600);
        spawn_thought("spam_poked");
        last = now;
        return;
    }
    pet_user_touch();
    if (now - last < 1200) {
        if (now - last < 350) { last = now; return; }   /* touch bounce */
        petlog("double-poke -> panicked, asking what you need");
        pet_persona_event("talk");
        pet_react("scared", 0, 1500);       /* instant local panic       */
        spawn_thought("poked_twice");
    } else {
        /* zone-aware single poke: head vs body */
        lv_indev_t *ind = lv_indev_get_act();
        lv_point_t pt = { 0, 0 };
        if (ind) lv_indev_get_point(ind, &pt);
        lv_area_t a;
        lv_obj_get_coords(img_char, &a);
        bool head = pt.y < a.y1 + (a.y2 - a.y1) * 2 / 5;
        if (head) {
            petlog("head-poked -> indignant");
            pet_persona_event("poke");
            pet_react("grumpy", 0, 1800);
        } else {
            petlog("poked");
            pet_persona_event("poke");
            pet_react("happy", 0, 0);
        }
    }
    last = now;
}
static void bg_click_cb(lv_event_t *e) { (void)e; router_go(SCR_MENU); }
static void swipe_cb(lv_event_t *e)
{
    (void)e;
    if (lv_indev_get_gesture_dir(lv_indev_get_act()) == LV_DIR_LEFT) {
        pet_user_touch();
        router_go(SCR_MENU);            /* swipe left on standby -> menu */
    }
}
static void wheel_key_cb(lv_event_t *e)
{
    uint32_t k = lv_event_get_key(e);
    if (k == LV_KEY_ENTER) {                 /* wheel press -> menu */
        pet_user_touch();
        router_go(SCR_MENU);
        return;
    }
    if (k == LV_KEY_LEFT || k == LV_KEY_RIGHT) {
        pet_user_touch();
        petlog("petted (wheel)"); pet_persona_event("pet"); pet_react("laugh", 0, 0);
    }
}
static void del_timer_cb(lv_event_t *e)
{ lv_timer_del((lv_timer_t *)lv_event_get_user_data(e)); }

static void del_scr_cb(lv_event_t *e)
{ (void)e; lb_sub = NULL; img_char = NULL; img_fx = NULL; }

lv_obj_t *scr_idle_create(void)
{
    lv_obj_t *scr = theme_screen_create();
    lv_obj_t *bg = lv_img_create(scr);
    lv_img_set_src(bg, &img_pet_bg);
    lv_obj_center(bg);
    lv_obj_clear_flag(bg, LV_OBJ_FLAG_CLICKABLE);

    arc_batt = rim_arc_create(scr, ARC_BATTERY_SWEEP, 0, C_ACCENT);
    ic_wifi = icon_wifi_create(scr);
    lv_obj_align(ic_wifi, LV_ALIGN_TOP_MID, 0, 28);
    lb_clock = lv_label_create(scr);
    lv_obj_add_style(lb_clock, &st_data, 0);
    lv_obj_set_style_text_color(lb_clock, C_TEXT_LO, 0);
    lv_obj_align(lb_clock, LV_ALIGN_TOP_MID, 0, 66);

    img_char = lv_img_create(scr);
    lv_obj_add_flag(img_char, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(img_char, char_click_cb, LV_EVENT_CLICKED, NULL);

    lb_sub = lv_label_create(scr);
    lv_obj_add_style(lb_sub, &st_body, 0);
    lv_obj_set_width(lb_sub, 320);
    lv_obj_set_style_text_align(lb_sub, LV_TEXT_ALIGN_CENTER, 0);
    lv_obj_set_style_bg_color(lb_sub, C_BG, 0);
    lv_obj_set_style_bg_opa(lb_sub, LV_OPA_70, 0);
    lv_obj_set_style_pad_all(lb_sub, 6, 0);
    lv_obj_set_style_radius(lb_sub, 10, 0);
    lv_label_set_long_mode(lb_sub, LV_LABEL_LONG_WRAP);
    lv_obj_align(lb_sub, LV_ALIGN_BOTTOM_MID, 0, -34);
    lv_obj_add_flag(lb_sub, LV_OBJ_FLAG_HIDDEN);
    lv_obj_clear_flag(lb_sub, LV_OBJ_FLAG_CLICKABLE);

    img_fx = lv_img_create(scr);
    lv_obj_add_flag(img_fx, LV_OBJ_FLAG_HIDDEN);
    lv_obj_clear_flag(img_fx, LV_OBJ_FLAG_CLICKABLE);

    lv_obj_add_flag(scr, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(scr, del_scr_cb, LV_EVENT_DELETE, NULL);
    lv_obj_add_event_cb(scr, bg_click_cb, LV_EVENT_CLICKED, NULL);
    lv_obj_add_event_cb(scr, swipe_cb, LV_EVENT_GESTURE, NULL);
    lv_obj_add_event_cb(scr, wheel_key_cb, LV_EVENT_KEY, NULL);
    lv_group_add_obj(router_group(), scr);
    lv_group_set_editing(router_group(), true);

    pet_persona_init();
    was_charging = g_state.charging;
    pet_user_touch();
    set_base(is_night() ? &C_SLEEP : &C_IDLE);

    tick_cb(NULL);
    lv_timer_t *t1 = lv_timer_create(tick_cb, 1000, NULL);
    lv_obj_add_event_cb(scr, del_timer_cb, LV_EVENT_DELETE, t1);
    lv_timer_t *t2 = lv_timer_create(brain_cb, 100, NULL);
    lv_obj_add_event_cb(scr, del_timer_cb, LV_EVENT_DELETE, t2);
    lv_timer_t *t3 = lv_timer_create(anim_cb, 30, NULL);
    lv_obj_add_event_cb(scr, del_timer_cb, LV_EVENT_DELETE, t3);
    return scr;
}
