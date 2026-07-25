#pragma once
/* Live screen mirror + screenshots over HTTP. Starts automatically once
 * Wi-Fi is up (and provisioning is done):
 *   http://<watcher-ip>/          auto-refreshing mirror page
 *   http://<watcher-ip>/shot.bmp  single screenshot (16-bit BMP)
 * Debug/inspection tool — zero cost until a request arrives.           */
void svc_mirror_start(void);
