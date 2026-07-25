#pragma once
/* Captive-portal Wi-Fi setup: SoftAP "watcher-XXXX" + DNS catch-all +
 * a themed HTTP page (scan, pick network, password). Replaces the
 * Espressif provisioning app entirely. Started by svc_wifi when no
 * credentials exist; tears itself down after a successful connect.    */
void svc_portal_start(void);
void svc_portal_stop(void);
