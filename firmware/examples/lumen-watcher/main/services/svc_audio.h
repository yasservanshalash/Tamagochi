#pragma once
#include <stdbool.h>
#include <stdint.h>

/* Mic capture -> live RMS in g_state.mic_level (0..100).
 * Codec initialised lazily on first use (keeps boot instant).
 * Speaker volume applied via bsp_codec_volume_set from settings.        */
void svc_audio_init(void);           /* create lock; call from app_main */
void svc_audio_prewarm_voice(void);  /* reserve voice worker stacks at boot */
bool svc_audio_mic_start(void);
void svc_audio_mic_stop(void);
void svc_audio_apply_volume(void);   /* pushes g_state.volume to codec  */
void svc_audio_beep(void);           /* short confirmation tone         */
bool svc_audio_play_url(const char *url);  /* stream WAV; mouth follows  */
void svc_audio_record_start(uint8_t *buf, int max);  /* mic must be on   */
int  svc_audio_record_stop(void);                    /* bytes captured   */
