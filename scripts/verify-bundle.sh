#!/bin/bash
# Check that a Linux bundle of Aria is safe to ship:
#   - it contains the engine sidecar (ace-server)
#   - that sidecar is NOT the setup.sh placeholder
#   - it does NOT contain model weights (those download on first run)
#
# Usage:
#   ./scripts/verify-bundle.sh path/to/Aria_0.1.0_amd64.deb
#   ./scripts/verify-bundle.sh path/to/Aria.AppImage
#   ./scripts/verify-bundle.sh path/to/org.aria.music.flatpak
#   ./scripts/verify-bundle.sh --self-test
#
# Exit 0 if the bundle looks right, 1 if it does not.

set -euo pipefail

MARKER="aria-dev-placeholder"

usage() {
    sed -n '2,13p' "$0" | sed 's/^# \?//'
    exit 1
}

[ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ] && usage

self_test() {
    local work dest
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT

    command -v cc >/dev/null 2>&1 || {
        echo "self-test needs a C compiler (sudo apt install build-essential)"
        exit 1
    }
    command -v ar >/dev/null 2>&1 || {
        echo "self-test needs ar (sudo apt install binutils)"
        exit 1
    }

    make_deb() {
        dest="$1"
        local tree="$2"
        tar -C "$tree" -czf "$work/data.tar.gz" .
        rm -f "$dest"
        ( cd "$work" && ar r "$dest" debian-binary control.tar.gz data.tar.gz >/dev/null )
    }

    echo "==> self-test: building a tiny fake .deb"
    mkdir -p "$work/data/usr/bin" "$work/control"
    printf 'int main(void){return 1;}\n' > "$work/ace.c"
    cc -O0 -o "$work/data/usr/bin/ace-server" "$work/ace.c"
    cc -O0 -o "$work/data/usr/bin/aria" "$work/ace.c"
    printf '2.0\n' > "$work/debian-binary"
    printf 'Package: aria\nVersion: 0.0.0\nArchitecture: amd64\nDescription: test\n' \
        > "$work/control/control"
    tar -C "$work/control" -czf "$work/control.tar.gz" .
    make_deb "$work/good.deb" "$work/data"
    "$0" "$work/good.deb"

    echo "==> self-test: a .deb that still has the placeholder must fail"
    mkdir -p "$work/bad/usr/bin"
    printf '#!/bin/sh\n# aria-dev-placeholder\nexit 1\n' > "$work/bad/usr/bin/ace-server"
    chmod +x "$work/bad/usr/bin/ace-server"
    make_deb "$work/bad.deb" "$work/bad"
    if "$0" "$work/bad.deb"; then
        echo "!! placeholder .deb was accepted" >&2
        exit 1
    fi

    echo "==> self-test: a .deb that ships a .gguf must fail"
    mkdir -p "$work/gguf/usr/bin" "$work/gguf/usr/share/aria"
    cp "$work/data/usr/bin/ace-server" "$work/gguf/usr/bin/ace-server"
    printf 'not-a-model' > "$work/gguf/usr/share/aria/vae-BF16.gguf"
    make_deb "$work/gguf.deb" "$work/gguf"
    if "$0" "$work/gguf.deb"; then
        echo "!! .gguf .deb was accepted" >&2
        exit 1
    fi

    echo "==> self-test passed"
    exit 0
}

[ "${1:-}" = "--self-test" ] && self_test
[ -n "${1:-}" ] || usage
BUNDLE="$1"
[ -f "$BUNDLE" ] || { echo "not a file: $BUNDLE" >&2; exit 1; }

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

fail() { echo "!! $*" >&2; exit 1; }
ok() { echo "    ok  $*"; }

# Extract a named file from the bundle into $TMP/extracted.
# Sets EXTRACTED to that path.
extract_list_and_sidecar() {
    local path="$1"
    local lower
    lower=$(echo "$path" | tr '[:upper:]' '[:lower:]')
    EXTRACTED=""
    LIST="$TMP/list.txt"

    case "$lower" in
        *.deb)
            ( cd "$TMP" && ar x "$path" )
            local data=""
            for cand in "$TMP"/data.tar.gz "$TMP"/data.tar.xz "$TMP"/data.tar.zst "$TMP"/data.tar.bz2; do
                [ -f "$cand" ] && data="$cand" && break
            done
            [ -n "$data" ] || fail ".deb has no data.tar.* (is it a debian package?)"
            tar -tf "$data" > "$LIST"
            mkdir -p "$TMP/data"
            tar -C "$TMP/data" -xf "$data"
            if [ -x "$TMP/data/usr/bin/ace-server" ]; then
                EXTRACTED="$TMP/data/usr/bin/ace-server"
            fi
            ;;
        *.appimage)
            # Prefer a leftover AppDir next to the image (Tauri often keeps it).
            local appdir
            appdir=$(find "$(dirname "$path")" -maxdepth 2 -type d -name '*.AppDir' 2>/dev/null | head -1)
            if [ -n "$appdir" ] && [ -e "$appdir/usr/bin/ace-server" ]; then
                find "$appdir" -type f | sed "s|^$appdir/||" > "$LIST"
                EXTRACTED="$appdir/usr/bin/ace-server"
            else
                # AppImage is a squashfs with an ELF header. Asking it to
                # extract one file works without libfuse when this is set.
                export APPIMAGE_EXTRACT_AND_RUN=1
                chmod +x "$path" 2>/dev/null || true
                ( cd "$TMP" && "$path" --appimage-extract usr/bin/ace-server >/dev/null )
                ( cd "$TMP" && "$path" --appimage-extract usr/bin >/dev/null 2>&1 || true )
                if [ -e "$TMP/squashfs-root" ]; then
                    find "$TMP/squashfs-root" -type f | sed "s|^$TMP/squashfs-root/||" > "$LIST"
                    [ -e "$TMP/squashfs-root/usr/bin/ace-server" ] \
                        && EXTRACTED="$TMP/squashfs-root/usr/bin/ace-server"
                fi
            fi
            ;;
        *.flatpak)
            # A .flatpak is an ostree repo dump. listing needs `ostree` or
            # `flatpak`. Fall back to `tar` if it's a just-built repo file.
            if command -v flatpak >/dev/null 2>&1; then
                flatpak info --show-location org.aria.music >/dev/null 2>&1 || true
            fi
            # We still try `ostree`/`tar`; if we can't list, we fail honestly.
            if command -v tar >/dev/null 2>&1 && tar -tf "$path" >/dev/null 2>&1; then
                tar -tf "$path" > "$LIST"
            else
                echo "    (cannot list .flatpak contents without it installed; skipping file list)"
                : > "$LIST"
            fi
            ;;
        *)
            fail "unrecognised bundle (want .deb, .AppImage, or .flatpak): $path"
            ;;
    esac
}

echo "==> checking $(basename "$BUNDLE")"
extract_list_and_sidecar "$(readlink -f "$BUNDLE" 2>/dev/null || realpath "$BUNDLE" 2>/dev/null || echo "$BUNDLE")"

# --- model weights must not be inside the package ---
if grep -qiE '\.gguf$|\.safetensors$' "$LIST" 2>/dev/null; then
    echo "    files that look like model weights:"
    grep -iE '\.gguf$|\.safetensors$' "$LIST" | sed 's/^/      /'
    fail "bundle contains model weights — they must download on first run, not ship"
fi
ok "no model weights packed"

# --- engine sidecar ---
if [ -z "$EXTRACTED" ] || [ ! -e "$EXTRACTED" ]; then
    echo "    files in the bundle (first 40):"
    head -40 "$LIST" | sed 's/^/      /' || true
    fail "ace-server is missing from the bundle (Tauri sidecar was not packed)"
fi
ok "engine sidecar present ($(du -h "$EXTRACTED" | awk '{print $1}'))"

if grep -a -F -q "$MARKER" "$EXTRACTED"; then
    fail "engine sidecar is the setup.sh placeholder, not a real ace-server"
fi
ok "engine is not the placeholder"

# ELF on Linux. A shell-script placeholder would fail this too.
if command -v file >/dev/null 2>&1; then
    desc=$(file -b "$EXTRACTED")
    echo "    type: $desc"
    echo "$desc" | grep -qiE 'ELF|executable' || fail "ace-server is not an executable binary"
else
    # Minimal ELF magic: 0x7f E L F
    magic=$(head -c 4 "$EXTRACTED" | od -An -t x1 | tr -d ' \n')
    [ "$magic" = "7f454c46" ] || fail "ace-server does not start with ELF magic"
fi
ok "engine looks like a real binary"

# Shared libggml*.so would only exist on the build machine.
if command -v ldd >/dev/null 2>&1; then
    if ldd "$EXTRACTED" 2>/dev/null | grep -qiE 'ggml|not found'; then
        echo "    ldd:"
        ldd "$EXTRACTED" | sed 's/^/      /'
        fail "engine still depends on ggml shared libraries (rebuild with BUILD_SHARED_LIBS=OFF)"
    fi
    ok "engine does not need libggml*.so from the build directory"
fi

echo "==> $(basename "$BUNDLE") looks packable"
