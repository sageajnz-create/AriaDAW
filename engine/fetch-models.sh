#!/bin/bash
# Fetch the "Standard" tier GGUF set for an 8GB-VRAM GPU (RX 6650 XT).
# Uses plain curl with resume — no `hf` CLI, no Python dependency.
set -eu

REPO="Serveurperso/ACE-Step-1.5-GGUF"
DIR="$(dirname "$0")/acestep.cpp/models"
BASE="https://huggingface.co/${REPO}/resolve/main"

# Standard tier: ~5.6 GB total, fits ~6.9 GB usable VRAM.
FILES=(
    "vae-BF16.gguf"                      # 322 MB  — quality-critical, always BF16
    "Qwen3-Embedding-0.6B-Q8_0.gguf"     # 784 MB  — text encoder
    "acestep-5Hz-lm-1.7B-Q8_0.gguf"      # 1.98 GB — lyrics/codes LM
    "acestep-v15-turbo-Q8_0.gguf"        # 2.55 GB — DiT, 8-step turbo
)

mkdir -p "$DIR"
for f in "${FILES[@]}"; do
    if [ -f "$DIR/$f" ]; then
        echo "[ok] $f"
        continue
    fi
    echo "[get] $f"
    curl -fL -C - --retry 5 --retry-delay 2 \
        -o "$DIR/$f" "${BASE}/${f}"
done

echo "[done] models in $DIR"
du -sh "$DIR"
