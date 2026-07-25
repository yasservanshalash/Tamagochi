#pragma once
/* Pet behavior log: every brain decision lands here (and on serial).
 * Ring of recent events, served by the mirror at /log and shown live
 * under the mirror image — the "why is he doing that" window.          */
void petlog(const char *fmt, ...);
void petlog_boot_recover(void);          /* dump pre-crash trail at boot */
int  petlog_dump(char *out, int cap);    /* newest-last, returns bytes  */
int  petlog_tail(char *out, int cap, int n);  /* last n lines, JSON-safe */
