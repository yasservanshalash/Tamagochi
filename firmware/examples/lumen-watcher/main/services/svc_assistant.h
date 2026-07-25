#pragma once
#include <stdbool.h>
#include <stdint.h>

/* The mind contract. POSTs to the brain gateway (think_server.py on
 * TinkerBox, which fronts OpenRouter / Ollama / anything):
 *   { event, text, vitals{battery,hour}, persona{...} }
 * and receives { say, emotion, glitch }.
 *
 * The gateway URL is stored in NVS and set once from a browser:
 *   http://<watcher-ip>/brain?url=http://<server>:8087/pet/think
 * With no URL configured, demo mode returns canned in-character lines
 * so every flow stays testable offline.
 *
 * Blocking; call from a worker task only.                              */
bool assistant_think(const char *event, const char *text,
                     char *say, int say_len,
                     char *emotion, int emo_len, uint8_t *glitch,
                     char *audio_url, int audio_len);  /* full URL or "" */

/* ship a mic burst to the brain; true if the pet's name was heard */
bool assistant_wake_check(const uint8_t *pcm, int len, char *heard, int hlen);

bool assistant_converse(const uint8_t *pcm, int pcm_len,
                        char *heard, int heard_len,
                        char *say, int say_len,
                        char *emotion, int emo_len, uint8_t *glitch,
                        char *audio_url, int audio_len);
const char *assistant_take_action(void);   /* model-chosen verb, once  */
void assistant_set_url(const char *url);   /* persists to NVS           */
bool assistant_get_url(char *out, int len);
