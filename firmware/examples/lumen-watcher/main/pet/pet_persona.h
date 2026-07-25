#pragma once
#include <stdint.h>
/* Local personality engine: six stats (0..100) drift with events and
 * decay toward baselines. They bias idle behavior on-device and ride
 * along in every AI request, so the model's replies match the mood the
 * body is already showing.                                              */
typedef struct {
    uint8_t mood, paranoia, curiosity, energy, boredom, trust;
} pet_persona_t;

void          pet_persona_init(void);
pet_persona_t pet_persona_get(void);
void          pet_persona_event(const char *ev);   /* "poke","pet","person",
                                                      "noise","talk","glitch" */
void          pet_persona_minute(void);            /* decay + boredom tick   */
