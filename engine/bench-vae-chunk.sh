#!/bin/bash
# Find a VAE chunk size that decodes reliably on this GPU.
#
# Background: amdgpu enforces a compute-ring watchdog. A VAE tile large enough
# to exceed it kills the GPU context (vk::DeviceLostError) and takes the whole
# process with it. Upstream's default of 1024 does this on RADV/Navi23.
#
# We care about RELIABILITY first and speed second: a user whose first song
# wedges their graphics driver does not come back.

set -u
cd "$(dirname "$0")/acestep.cpp"

REQ="${1:-/home/sage/Projects/AriaDAW/output/req0.json}"
TRIALS="${2:-2}"

printf '%-8s %-8s %-10s %s\n' CHUNK TRIAL RESULT SECONDS
for chunk in 256 192 128 64; do
    overlap=$(( chunk / 8 ))
    for t in $(seq 1 "$TRIALS"); do
        S=$SECONDS
        out=$(./build/ace-synth --models models \
                --vae-chunk "$chunk" --vae-overlap "$overlap" \
                --request "$REQ" 2>&1)
        el=$(( SECONDS - S ))
        if echo "$out" | grep -q 'DeviceLost\|context is lost'; then
            printf '%-8s %-8s %-10s %s\n' "$chunk" "$t" "DEVICE_LOST" "$el"
            # Let the driver settle after a ring reset before the next attempt.
            sleep 10
        elif echo "$out" | grep -q 'All done'; then
            printf '%-8s %-8s %-10s %s\n' "$chunk" "$t" "ok" "$el"
        else
            printf '%-8s %-8s %-10s %s\n' "$chunk" "$t" "ERROR" "$el"
        fi
    done
done
