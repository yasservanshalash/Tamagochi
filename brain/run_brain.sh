#!/bin/bash
# Lumen brain launcher. Secrets live in .env (gitignored), NOT here.
# Copy .env.example -> .env and put your real OPENROUTER_API_KEY in it.
set -e
cd "$(dirname "$0")"

# Load secrets / overrides from .env if present
[ -f .env ] && set -a && . ./.env && set +a

: "${OPENROUTER_API_KEY:?Set OPENROUTER_API_KEY in brain/.env (copy from .env.example)}"

export PET_MODEL="${PET_MODEL:-x-ai/grok-4.3}"   # Grok: less filtered, edgier
export PET_SPICE="${PET_SPICE:-1}"               # casual swearing / roasting / dark comedy
export PIPER_BIN="${PIPER_BIN:-$HOME/piper/piper/piper}"
export PIPER_VOICE="${PIPER_VOICE:-$HOME/piper/en_US-ryan-high.onnx}"
export PET_VISION="${PET_VISION:-1}"
export PYTHONUNBUFFERED=1                         # wake-check prints must reach the log live
export PET_LANG="${PET_LANG:-en}"                 # stop whisper hallucinating other languages

# --- Speech-to-text (whisper) ---------------------------------------------
# On a machine with an NVIDIA GPU, a big model on CUDA float16 is BOTH more
# accurate AND faster than a tiny CPU model. Auto-detected by think_server.py.
# On your faster PC, bump this to large-v3 (or distil-large-v3 for speed).
export PET_STT_MODEL="${PET_STT_MODEL:-distil-large-v3}"
export PET_STT_PRELOAD="${PET_STT_PRELOAD:-1}"    # load whisper at boot, not on first reply
# export PET_STT_DEVICE=cuda      # auto by default; force cuda/cpu if needed
# export PET_STT_COMPUTE=float16  # auto by default (float16 on gpu, int8 on cpu)

PORT="${PORT:-8087}"
exec "${UVICORN:-$HOME/yasser-brain/bin/uvicorn}" think_server:app --host 0.0.0.0 --port "$PORT"
