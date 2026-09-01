#!/bin/bash
# Build Aria into Linux packages: .deb, AppImage, and (if tools exist) Flatpak.
#
# The engine is a separate C++ binary and the models are ~12 GB, so:
#   - the engine is built here and bundled as a Tauri sidecar
#   - the models are NOT packaged; the app downloads them on first run
#
# Sidecars must be named with the Rust target triple, which is what most of
# the fiddling below is about.
#
# Usage (from the repo root, on Linux):
#
#   ./scripts/package.sh                 real engine, all formats we can build
#   ./scripts/package.sh --layout-check  dummy engine, .deb only (CI)
#   ./scripts/package.sh --flatpak-only  wrap an already-built .deb as a Flatpak
#   ./scripts/package.sh --help
#
# What you need for a real package:
#   - the same tools as ./scripts/setup.sh --engine  (cmake, Vulkan headers, glslc)
#   - Node 20+, Rust, and the WebKitGTK 4.1 development files
#   - for Flatpak: flatpak, flatpak-builder, and org.gnome.Platform//48
#
# Debian/Ubuntu, one-shot:
#   sudo apt install build-essential cmake git pkg-config \
#       libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
#       librsvg2-dev patchelf libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
#       glslc libvulkan-dev spirv-headers
#
# Then:
#   ./scripts/package.sh
# Packages land in src-tauri/target/release/bundle/  and  packaging/flatpak/

set -euo pipefail
cd "$(dirname "$0")/.."

LAYOUT_CHECK=0
FLATPAK_ONLY=0
SKIP_FLATPAK=0
SKIP_APPIMAGE=0
for arg in "$@"; do
    case "$arg" in
        --layout-check) LAYOUT_CHECK=1; SKIP_APPIMAGE=1; SKIP_FLATPAK=1 ;;
        --flatpak-only) FLATPAK_ONLY=1 ;;
        --skip-flatpak) SKIP_FLATPAK=1 ;;
        --skip-appimage) SKIP_APPIMAGE=1 ;;
        -h|--help) sed -n '2,32p' "$0" | sed 's/^# \?//'; exit 0 ;;
        *) echo "unknown option: $arg (try --help)"; exit 1 ;;
    esac
done

if [ "$(uname -s)" != "Linux" ]; then
    echo "This script builds Linux packages. It has to run on Linux."
    exit 1
fi

TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')"
[ -n "$TRIPLE" ] || { echo "could not detect the Rust target triple"; exit 1; }
SIDECAR="src-tauri/binaries/ace-server-${TRIPLE}"
MARKER="aria-dev-placeholder"

echo "==> target: $TRIPLE"

is_placeholder() {
    [ -f "$1" ] && grep -a -F -q "$MARKER" "$1"
}

stage_real_engine() {
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
    cp -f engine/acestep.cpp/build-static/ace-server "$SIDECAR"
    chmod +x "$SIDECAR"
    echo "==> engine staged as $(basename "$SIDECAR")"
}

stage_dummy_engine() {
    # A tiny ELF so CI can check the bundle *layout* without compiling
    # acestep.cpp (Vulkan + several minutes). It is not a working engine.
    # build.rs will still refuse the setup.sh placeholder in release.
    echo "==> staging a dummy engine for --layout-check (not a release)"
    mkdir -p src-tauri/binaries
    local src="$PWD/src-tauri/binaries/.ace-server-dummy.c"
    cat > "$src" <<'C'
/* Dummy ace-server for bundle-layout CI. Not the Aria engine. */
int main(void) { return 1; }
C
    if ! command -v cc >/dev/null 2>&1; then
        echo "cc is required for --layout-check (sudo apt install build-essential)"
        exit 1
    fi
    cc -O0 -o "$SIDECAR" "$src"
    rm -f "$src"
    chmod +x "$SIDECAR"
    echo "==> dummy engine staged as $(basename "$SIDECAR")"
}

refuse_placeholder() {
    if is_placeholder "$SIDECAR"; then
        echo "!! $SIDECAR is still the setup.sh placeholder."
        echo "   A package built now would not be able to generate music."
        echo "   Build the real engine, or pass --layout-check for a CI dummy."
        exit 1
    fi
    if [ ! -x "$SIDECAR" ]; then
        echo "!! no engine sidecar at $SIDECAR"
        exit 1
    fi
    # Shared libggml*.so would only exist on the build machine.
    if ldd "$SIDECAR" 2>/dev/null | grep -qiE 'ggml|not found'; then
        echo "!! the engine still depends on ggml shared libraries, so it would not"
        echo "   start once installed. Remove engine/acestep.cpp/build-static and"
        echo "   let this script rebuild it."
        ldd "$SIDECAR" || true
        exit 1
    fi
}

find_deb() {
    find src-tauri/target/release/bundle/deb -maxdepth 1 -type f -name '*.deb' 2>/dev/null | head -1
}

build_flatpak() {
    local deb="$1"
    if [ "$SKIP_FLATPAK" = 1 ]; then
        return 0
    fi
    if ! command -v flatpak-builder >/dev/null 2>&1; then
        echo
        echo "!! Skipping Flatpak — flatpak-builder is not installed."
        echo "   The .deb and AppImage (if built) are still good."
        echo "   To add Flatpak later:"
        echo "     sudo apt install flatpak flatpak-builder"
        echo "     sudo flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo"
        echo "     flatpak install flathub org.gnome.Platform//48 org.gnome.Sdk//48"
        echo "     ./scripts/package.sh --flatpak-only"
        return 0
    fi
    if ! flatpak info org.gnome.Platform//48 >/dev/null 2>&1; then
        echo
        echo "!! Skipping Flatpak — org.gnome.Platform//48 is not installed."
        echo "     flatpak install flathub org.gnome.Platform//48 org.gnome.Sdk//48"
        echo "     ./scripts/package.sh --flatpak-only"
        return 0
    fi

    echo "==> building Flatpak from $(basename "$deb")"
    local work="packaging/flatpak"
    cp -f "$deb" "$work/aria.deb"
    mkdir -p "$work/build" "$work/repo"
    # --disable-rofiles-fuse: cloud VMs and some CI runners have no FUSE.
    local fuse_flag=()
    if [ ! -e /dev/fuse ]; then
        fuse_flag=(--disable-rofiles-fuse)
    fi
    flatpak-builder --force-clean --user \
        "${fuse_flag[@]}" \
        --repo="$work/repo" \
        "$work/build" \
        "$work/org.aria.music.yml"
    local out="packaging/aria-0.1.0.flatpak"
    flatpak build-bundle "$work/repo" "$out" org.aria.music
    echo "==> Flatpak: $out"
    ./scripts/verify-bundle.sh "$out" || {
        echo "!! Flatpak was produced but could not be inspected; opening it still needs:"
        echo "     flatpak install --user $out"
        echo "     flatpak run org.aria.music"
    }
}

if [ "$FLATPAK_ONLY" = 1 ]; then
    deb=$(find_deb)
    if [ -z "$deb" ]; then
        echo "!! no .deb under src-tauri/target/release/bundle/deb/"
        echo "   Run ./scripts/package.sh once first, then --flatpak-only."
        exit 1
    fi
    SKIP_FLATPAK=0
    build_flatpak "$deb"
    exit 0
fi

# --- engine ---------------------------------------------------------------
if [ "$LAYOUT_CHECK" = 1 ]; then
    stage_dummy_engine
else
    stage_real_engine
fi
refuse_placeholder
echo "==> engine is ready to bundle"

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
npm run tauri -- build --bundles deb

deb=$(find_deb)
[ -n "$deb" ] || { echo "!! Tauri did not produce a .deb"; exit 1; }
echo "==> .deb: $deb"
./scripts/verify-bundle.sh "$deb"

if [ "$SKIP_APPIMAGE" != 1 ]; then
    echo "==> building AppImage"
    npm run tauri -- build --bundles appimage || {
        echo "!! AppImage failed; the .deb above is still good"
    }
    img=$(find src-tauri/target/release/bundle/appimage -maxdepth 1 -type f -name '*.AppImage' 2>/dev/null | head -1)
    if [ -n "$img" ]; then
        echo "==> AppImage: $img"
        ./scripts/verify-bundle.sh "$img" || echo "!! AppImage built but failed the bundle check"
    fi
fi

build_flatpak "$deb"

echo
echo "==> done"
find src-tauri/target/release/bundle -maxdepth 2 -type f \
    \( -name '*.AppImage' -o -name '*.deb' \) -printf '    %p  (%s bytes)\n' 2>/dev/null || true
if [ -f packaging/aria-0.1.0.flatpak ]; then
    printf '    packaging/aria-0.1.0.flatpak  (%s bytes)\n' "$(wc -c < packaging/aria-0.1.0.flatpak)"
fi
echo
if [ "$LAYOUT_CHECK" = 1 ]; then
    echo "This was --layout-check: the .deb contains a dummy engine, not acestep.cpp."
    echo "For something you can install:  ./scripts/package.sh"
else
    echo "Models are not included — about 12 GB, downloaded on first run."
    echo
    echo "Install the .deb:      sudo apt install ./src-tauri/target/release/bundle/deb/*.deb"
    echo "Run the AppImage:      chmod +x src-tauri/target/release/bundle/appimage/*.AppImage"
    echo "                       ./src-tauri/target/release/bundle/appimage/*.AppImage"
    if [ -f packaging/aria-0.1.0.flatpak ]; then
        echo "Install the Flatpak:   flatpak install --user packaging/aria-0.1.0.flatpak"
        echo "                       flatpak run org.aria.music"
    fi
fi
