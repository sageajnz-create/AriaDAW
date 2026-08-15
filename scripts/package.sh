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
# Built with BUILD_SHARED_LIBS=OFF on purpose. The default build links against
# four libggml*.so files by absolute path inside the build directory, so a
# packaged copy installs cleanly and then fails to start on any other machine.
# A static binary is bigger (~55 MB) and simply works.
if [ ! -x engine/acestep.cpp/build-static/ace-server ]; then
    echo "==> building the engine (this takes a few minutes)"
    if [ ! -d engine/acestep.cpp ]; then
        git clone --recurse-submodules --depth 1 \
            https://github.com/ServeurpersoCom/acestep.cpp.git engine/acestep.cpp
    fi
    (
        cd engine/acestep.cpp
        mkdir -p build-static && cd build-static
        cmake .. -DGGML_VULKAN=ON -DBUILD_SHARED_LIBS=OFF
        cmake --build . --config Release -j "$(nproc)"
    )
fi

mkdir -p src-tauri/binaries
cp engine/acestep.cpp/build-static/ace-server "src-tauri/binaries/ace-server-${TRIPLE}"
echo "==> engine staged as ace-server-${TRIPLE}"

# Refuse to ship a binary that needs libraries we aren't shipping.
if ldd "src-tauri/binaries/ace-server-${TRIPLE}" 2>/dev/null | grep -qiE 'ggml|not found'; then
    echo "!! the engine still depends on ggml shared libraries, so it would not"
    echo "   start once installed. Remove engine/acestep.cpp/build-static and"
    echo "   let this script rebuild it."
    exit 1
fi
echo "==> engine is self-contained"

# --- app ------------------------------------------------------------------
echo "==> installing frontend dependencies"
npm ci --silent 2>/dev/null || npm install --silent

# The AppImage tooling is itself an AppImage and needs libfuse.so.2, which
# current distros no longer ship — they have fuse3. Rather than make people
# install a legacy fuse2 package, tell those tools to extract and run.
export APPIMAGE_EXTRACT_AND_RUN=1
# linuxdeploy's strip step fails on the 55 MB static engine binary and takes
# the whole AppImage down with it. Skipping strip costs a few MB.
export NO_STRIP=1

# Build the .deb first and separately. A failure in the AppImage stage aborts
# the whole run, and there is no reason to lose a working .deb to it.
echo "==> building .deb"
npm run tauri build -- --bundles deb

echo "==> building AppImage"
npm run tauri build -- --bundles appimage || {
    echo "!! AppImage failed; the .deb above is still good"
}

echo
echo "==> done"
find src-tauri/target/release/bundle -maxdepth 2 -type f \
    \( -name '*.AppImage' -o -name '*.deb' \) -printf '    %p  (%s bytes)\n' 2>/dev/null || true
echo
echo "Models are not included — about 12 GB, downloaded on first run."
