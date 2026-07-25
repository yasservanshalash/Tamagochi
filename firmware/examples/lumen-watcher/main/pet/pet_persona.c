#include "pet_persona.h"
#include "lumen.h"
#include "svc_petlog.h"
#include <string.h>
#include <time.h>

static pet_persona_t P = { 55, 45, 50, 70, 20, 60 };

static uint8_t clamp(int v) { return v < 0 ? 0 : (v > 100 ? 100 : v); }
static void add(uint8_t *s, int d) { *s = clamp((int)*s + d); }

void pet_persona_init(void) {}
pet_persona_t pet_persona_get(void) { return P; }

void pet_persona_event(const char *ev)
{
    if (!strcmp(ev, "poke"))   { add(&P.mood,+4);  add(&P.boredom,-12); add(&P.trust,+2); }
    else if (!strcmp(ev,"pet")){ add(&P.mood,+6);  add(&P.trust,+4);    add(&P.boredom,-10); }
    else if (!strcmp(ev,"person")) { add(&P.curiosity,+8); add(&P.boredom,-8); }
    else if (!strcmp(ev,"noise"))  { add(&P.paranoia,+6);  add(&P.curiosity,+4); }
    else if (!strcmp(ev,"talk"))   { add(&P.mood,+3); add(&P.trust,+3); add(&P.boredom,-15); }
    else if (!strcmp(ev,"glitch")) { add(&P.paranoia,+5); add(&P.mood,-2); }
}

void pet_persona_minute(void)
{
    /* boredom rises when ignored; battery drags energy; night lowers it */
    add(&P.boredom, +3);
    P.energy = clamp((g_state.battery_pct * 7 + P.energy * 3) / 10);
    time_t now = time(NULL); struct tm ti; localtime_r(&now, &ti);
    if (g_state.time_valid && (ti.tm_hour >= 21 || ti.tm_hour < 8)) add(&P.energy, -4);
    /* everything else decays gently toward baseline */
    P.mood      = clamp(P.mood      + (55 - P.mood)      / 8);
    P.paranoia  = clamp(P.paranoia  + (45 - P.paranoia)  / 10);
    P.curiosity = clamp(P.curiosity + (50 - P.curiosity) / 8);
    P.trust     = clamp(P.trust     + (60 - P.trust)     / 12);
    petlog("persona: mood%u par%u cur%u nrg%u bor%u tru%u",
           P.mood, P.paranoia, P.curiosity, P.energy, P.boredom, P.trust);
}
