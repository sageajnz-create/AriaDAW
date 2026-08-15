#!/bin/bash
# Build Aria into an AppImage and a .deb.
#
# The engine is a separate C++ binary and the models are ~12 GB, so:
#   - the engine is built here and bundled as a Tauri sidecar
#   - the models are NOT packaged; the app downloads them on first run
#
# Sidecars must be named with the Rust target triple, which is what most of
# the fiddling below is about.

set -euo pipefail
cd "$(dirname "$0")/.."

TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')"
echo "==> target: $TRIPLE"

# --- engine ---------------------------------------------------------------
if [ ! -x engine/acestep.cpp/build/ace-server ]; then
    echo "==> building the engine (this takes a few minutes)"
    if [ ! -d engine/acestep.cpp ]; then
        git clone --recurse-submodules --depth 1 \
            https://github.com/ServeurpersoCom/acestep.cpp.git engine/acestep.cpp
    fi
    (cd engine/acestep.cpp && ./buildvulkan.sh)
fi

mkdir -p src-tauri/binaries
cp engine/acestep.cpp/build/ace-server "src-tauri/binaries/ace-server-${TRIPLE}"
echo "==> engine staged as ace-server-${TRIPLE}"

# ggml builds shared libraries next to the binary; they have to travel with it.
mkdir -p src-tauri/resources/engine
cp engine/acestep.cpp/build/*.so* src-tauri/resources/engine/ 2>/dev/null || true
echo "==> $(ls -1 src-tauri/resources/engine 2>/dev/null | wc -l) engine libraries staged"

# --- app ------------------------------------------------------------------
echo "==> installing frontend dependencies"
npm ci --silent 2>/dev/null || npm install --silent

echo "==> building"
npm run tauri build

echo
echo "==> done"
find src-tauri/target/release/bundle -maxdepth 2 -type f \
    \( -name '*.AppImage' -o -name '*.deb' \) -printf '    %p  (%s bytes)\n' 2>/dev/null || true
echo
echo "Models are not included — about 12 GB, downloaded on first run."
