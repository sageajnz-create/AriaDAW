#!/bin/bash
# Get a working copy of Aria ready to run from source.
#
# The engine and the model weights are deliberately not committed (a C++ build
# and ~5.6 GB of GGUF), so a fresh clone can't build until they're accounted
# for. This script accounts for them, in the order they actually block you:
#
#   ./scripts/setup.sh              app only — window runs, engine shows as missing
#   ./scripts/setup.sh --engine     also build acestep.cpp with Vulkan (needs cmake)
#   ./scripts/setup.sh --models     also fetch the Standard-tier weights (~5.6 GB)
#
# The app half is useful on its own: everything except generation — library,
# player, lyrics, export — is testable with no engine and no weights.

set -u
cd "$(dirname "$0")/.."

WANT_ENGINE=0
WANT_MODELS=0
for arg in "$@"; do
    case "$arg" in
        --engine) WANT_ENGINE=1 ;;
        --models) WANT_MODELS=1 ;;
        --all)    WANT_ENGINE=1; WANT_MODELS=1 ;;
        -h|--help) sed -n '2,13p' "$0" | sed 's/^# \?//'; exit 0 ;;
        *) echo "unknown option: $arg (try --help)"; exit 1 ;;
    esac
done

fail=0
need() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "  missing: $1 — $2"
        fail=1
    fi
}

echo "==> checking the toolchain"
need node "install Node 20+"
need npm "ships with Node"
need cargo "https://rustup.rs, or: mise use -g rust@stable"
[ "$WANT_ENGINE" = 1 ] && { need cmake "the engine is a CMake project"; need git "to clone acestep.cpp"; }
[ "$WANT_MODELS" = 1 ] && need curl "to download the weights"
[ "$fail" = 1 ] && exit 1

# Tauri builds against the system WebKitGTK. Checking here turns a wall of
# linker errors into one line naming the package to install. Linux only —
# Windows and macOS use the webview that ships with the OS.
if [ "$(uname -s)" = "Linux" ] && ! pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
    echo "  missing: webkit2gtk-4.1 development files"
    echo "    Arch   sudo pacman -S webkit2gtk-4.1 libsoup3"
    echo "    Debian sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev"
    exit 1
fi
# The engine's Vulkan backend compiles GGML's compute shaders at build time, so
# it needs the shader toolchain and the SPIR-V headers as well as Vulkan itself.
# CMake only reports these one at a time, several minutes apart, so check them
# together and name them all at once.
if [ "$WANT_ENGINE" = 1 ] && [ "$(uname -s)" = "Linux" ]; then
    missing_vk=""
    command -v glslc >/dev/null 2>&1 || missing_vk="$missing_vk shaderc"
    [ -d /usr/include/vulkan ] || missing_vk="$missing_vk vulkan-headers"
    # find_package(SPIRV-Headers CONFIG) wants this file specifically; the
    # headers alone are not enough.
    [ -f /usr/share/cmake/SPIRV-Headers/SPIRV-HeadersConfig.cmake ] \
        || [ -f /usr/lib/cmake/SPIRV-Headers/SPIRV-HeadersConfig.cmake ] \
        || missing_vk="$missing_vk spirv-headers"
    if [ -n "$missing_vk" ]; then
        echo "  missing, for the engine's Vulkan backend:$missing_vk"
        echo "    Arch   sudo pacman -S --needed$missing_vk"
        echo "    Debian sudo apt install glslc libvulkan-dev spirv-headers"
        exit 1
    fi
fi

echo "    ok"

# Not a build dependency, so this never blocks — but finding out from a finished
# song that the lyrics are about the wrong subject is a bad way to learn it.
if ! curl -sf --max-time 2 http://127.0.0.1:11434/api/tags >/dev/null 2>&1; then
    echo "==> note: no lyric writer found"
    echo "    Aria writes lyrics with a small local model through Ollama. Without"
    echo "    one it still makes songs, but the words usually aren't about what"
    echo "    you asked for. See the README."
    echo "      ollama pull llama3.2:3b     # ~2 GB, stays on this computer"
fi

echo "==> installing frontend dependencies"
npm ci --silent 2>/dev/null || npm install --silent || exit 1

# `tauri::generate_context!` embeds `frontendDist` at compile time, so the Rust
# crate does not build — not even `cargo test` — until this exists. It is fast,
# and `tauri dev` serves from Vite anyway, so building it once here costs
# nothing and removes a confusing first failure.
echo "==> building the frontend"
npm run build --silent || exit 1

if [ "$WANT_ENGINE" = 1 ]; then
    if [ ! -x engine/acestep.cpp/build-static/ace-server ]; then
        echo "==> building the engine (several minutes)"
        [ -d engine/acestep.cpp ] || git clone --recurse-submodules --depth 1 \
            https://github.com/ServeurpersoCom/acestep.cpp.git engine/acestep.cpp || exit 1
        (
            cd engine/acestep.cpp || exit 1
            mkdir -p build-static && cd build-static || exit 1
            # Static for the same reason scripts/package.sh does it: the shared
            # build hardcodes libggml*.so paths from the build directory.
            cmake .. -DGGML_VULKAN=ON -DBUILD_SHARED_LIBS=OFF || exit 1
            cmake --build . --config Release -j "$(nproc)" || exit 1
        ) || exit 1
    fi
    echo "==> engine: engine/acestep.cpp/build-static/ace-server"
fi

if [ "$WANT_MODELS" = 1 ]; then
    echo "==> fetching model weights"
    ./engine/fetch-models.sh || exit 1
fi

# tauri-build refuses to build at all when an externalBin named in
# tauri.conf.json is absent, so a checkout with no engine can't even run the
# Rust tests. Stage whatever we have under the target-triple name it wants.
# On Windows Tauri also requires the `.exe` suffix — without it, `cargo test`
# looks for `ace-server-$triple.exe` and fails even though the placeholder
# is sitting there without the extension. (Linux CI never saw this; Windows
# CI on master has been failing on it.)
TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')"
SIDECAR="src-tauri/binaries/ace-server-${TRIPLE}"
case "$TRIPLE" in
    *-windows-*) SIDECAR="${SIDECAR}.exe" ;;
esac
mkdir -p src-tauri/binaries
if [ -x engine/acestep.cpp/build-static/ace-server ]; then
    cp engine/acestep.cpp/build-static/ace-server "$SIDECAR"
    echo "==> staged the engine as $(basename "$SIDECAR")"
elif [ ! -s "$SIDECAR" ] || head -2 "$SIDECAR" | grep -q 'aria-dev-placeholder'; then
    # A placeholder, not an engine. It exists to unblock the build, and says so
    # if anything ever runs it. `scripts/package.sh` overwrites it with the real
    # binary before bundling, so it cannot reach a release.
    #
    # The `aria-dev-placeholder` line is load-bearing: Tauri copies this file
    # next to the app, which is one of the places the engine supervisor looks,
    # so `src-tauri/src/engine.rs` reads that marker to tell it apart from a
    # real engine. Keep the two in step.
    cat > "$SIDECAR" <<'STUB'
#!/bin/sh
# aria-dev-placeholder
echo "This is a placeholder, not the Aria engine." >&2
echo "Build the real one:  ./scripts/setup.sh --engine" >&2
exit 1
STUB
    chmod +x "$SIDECAR"
    echo "==> staged a placeholder engine (build the real one with --engine)"
fi

echo
echo "==> ready"
echo "    npm run tauri dev          run the app"
echo "    npx vitest run             frontend tests"
echo "    cd src-tauri && cargo test  backend tests"
if [ ! -x engine/acestep.cpp/build-static/ace-server ]; then
    echo
    echo "    No engine yet, so the app will open on its setup screen and"
    echo "    generation is unavailable. Everything else works."
fi
