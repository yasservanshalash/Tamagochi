#pragma once

/* Non-blocking: call once after the first frame is on screen.
 * - stored credentials -> STA connect -> SNTP -> RTC sync
 * - no credentials     -> SoftAP provisioning (ESP SoftAP Prov app),
 *                         QR payload published in g_state.prov_qr_payload
 * Zero cloud accounts involved. */
void svc_wifi_start(void);

/* wipe stored Wi-Fi credentials and reboot into provisioning */
void svc_wifi_reset(void);
