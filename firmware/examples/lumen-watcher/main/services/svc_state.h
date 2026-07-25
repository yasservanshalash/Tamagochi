#pragma once
#include "lumen.h"

/* starts the 5 s battery/charging poll + loads persisted settings */
void svc_state_init(void);

/* persist brightness/volume to NVS (call after user change) */
void svc_state_save_settings(void);
