# Lumen — Yasser AI Tamagotchi

Custom firmware + brain for a SenseCAP Watcher (ESP32-S3) turned into a living
pixel-art character whose mind is an LLM gateway on the LAN.

Two parts:

| Dir         | What it is                                                        |
|-------------|-------------------------------------------------------------------|
| `brain/`    | `think_server.py` — the LLM gateway + **local whisper (STT) & piper (TTS)**. This is the part that gets faster on a GPU box. |
| `firmware/` | The `lumen-watcher` ESP-IDF app + SenseCAP components. Runs on the device. |

## brain/ — run the local models

```bash
cd brain
python3 -m venv .venv && . .venv/bin/activate
pip install -r requirements.txt
cp .env.example .env          # then put your real OPENROUTER_API_KEY in .env
./run_brain.sh                # serves on 0.0.0.0:8087
```

Point the device at it once, from any browser on the LAN:
`http://<watcher-ip>/brain?url=http://<this-box-ip>:8087/pet/think`

### Making speech pickup faster & more accurate (the whole point of the PC move)
`think_server.py` **auto-detects an NVIDIA GPU**. On a CUDA box it loads
`distil-large-v3` in `float16` — far more accurate than the laptop's `base`
model, and still faster because it's on the GPU. Override via env in `.env`:

- `PET_STT_MODEL=large-v3` — max accuracy (needs ~3 GB VRAM in float16, or set
  `PET_STT_COMPUTE=int8_float16` to fit ~1.6 GB).
- `PET_STT_MODEL=distil-large-v3` — near-large accuracy, English, fast (default on GPU).
- `PET_STT_DEVICE=cuda|cpu` and `PET_STT_COMPUTE=float16|int8` — force if auto-detect is wrong.

GPU whisper needs the CUDA/cuDNN runtime:
`pip install nvidia-cublas-cu12 nvidia-cudnn-cu12` (or a system CUDA install).

**Not committed** (get them on the target machine): the piper binary + voice
`.onnx`, whisper model weights (auto-download on first run), your `.env`.

## firmware/ — build & flash the device

Needs **ESP-IDF v5.2.1** installed separately (not in this repo):

```bash
# one-time: install ESP-IDF v5.2.1 per Espressif's guide, then each session:
. $HOME/esp/esp-idf/export.sh
cd firmware/examples/lumen-watcher
idf.py build
idf.py -p /dev/ttyACM1 flash monitor
```

`build/` and `managed_components/` are gitignored — `idf.py` regenerates them.
See `firmware/examples/lumen-watcher/CLAUDE.md` for architecture, hard
constraints, and open bugs.
