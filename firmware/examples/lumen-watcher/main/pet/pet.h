#pragma once
#include <stdint.h>
/* The AI motion contract. When /pet/think (or BLE, or any sense) wants
 * the character to move, it calls this — from ANY task (thread-safe):
 *   emotion: "idle" | "happy" | "busy" | "talk" | "glitch"
 *   glitch:  0..100 — >=60 prefixes a corruption burst
 *   hold_ms: how long talk/busy holds (happy/glitch have natural length)
 * The home screen consumes it on the next brain tick.                  */
void pet_react(const char *emotion, uint8_t glitch, uint16_t hold_ms);
void pet_say(const char *text, uint16_t hold_ms);   /* home subtitle       */
void pet_user_touch(void);       /* mark a USER interaction (not AI)  */
