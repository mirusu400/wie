#!/usr/bin/env bash
# Build wie_libretro for an Android target.
#
# Usage:   scripts/build-android.sh <rust-target>
# Required env: ANDROID_NDK_HOME (or ANDROID_NDK_ROOT)
# Optional env: ANDROID_API (default 24)
#
# Why each fix-up:
# - Android NDK has no separate libpthread/libunwind (folded into libc).
#   unicorn-engine-sys still emits `cargo:rustc-link-lib=pthread`, so we
#   provide an empty libpthread.a on the link path to satisfy the linker.
# - cmake-rs sees CMAKE_TOOLCHAIN_FILE for the target and passes it to cmake,
#   but the NDK's android.toolchain.cmake needs ANDROID_ABI as a cmake
#   variable (not env). We wrap it with a thin file that pre-sets ANDROID_ABI.

set -euo pipefail

if [[ $# -lt 1 ]]; then
    echo "usage: $0 <rust-target>" >&2
    exit 2
fi

TARGET="$1"
NDK="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}"
if [[ -z "$NDK" ]]; then
    echo "ANDROID_NDK_HOME or ANDROID_NDK_ROOT must be set" >&2
    exit 2
fi
API="${ANDROID_API:-24}"

case "$TARGET" in
    aarch64-linux-android)
        ABI=arm64-v8a
        CC_NAME="aarch64-linux-android${API}-clang"
        ;;
    armv7-linux-androideabi)
        ABI=armeabi-v7a
        CC_NAME="armv7a-linux-androideabi${API}-clang"
        ;;
    x86_64-linux-android)
        ABI=x86_64
        CC_NAME="x86_64-linux-android${API}-clang"
        ;;
    i686-linux-android)
        ABI=x86
        CC_NAME="i686-linux-android${API}-clang"
        ;;
    *)
        echo "unsupported target: $TARGET" >&2
        exit 2
        ;;
esac

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HOST_TAG="linux-x86_64"
case "$(uname -s)" in
    Darwin) HOST_TAG="darwin-x86_64" ;;
    Linux) HOST_TAG="linux-x86_64" ;;
esac
TOOLCHAIN_BIN="$NDK/toolchains/llvm/prebuilt/$HOST_TAG/bin"
STUBS="$REPO_ROOT/target/android-stubs"
TOOLCHAIN_FILE="$REPO_ROOT/target/android-toolchain-${ABI}.cmake"

mkdir -p "$STUBS" "$(dirname "$TOOLCHAIN_FILE")"

# Empty stub for libpthread / libunwind (Android merges both into libc).
# Compile with the per-target wrapper so the stub matches the link target.
STUB_OUT="$STUBS/$ABI"
mkdir -p "$STUB_OUT"
TMP_O="$(mktemp -t empty.XXXXXX).o"
trap 'rm -f "$TMP_O"' EXIT
printf '' | "$TOOLCHAIN_BIN/$CC_NAME" -x c -c - -o "$TMP_O"
"$TOOLCHAIN_BIN/llvm-ar" rcs "$STUB_OUT/libpthread.a" "$TMP_O"

cat > "$TOOLCHAIN_FILE" <<EOF
set(ANDROID_ABI ${ABI})
set(ANDROID_PLATFORM android-${API})
set(ANDROID_STL c++_shared)
include(\$ENV{ANDROID_NDK_ROOT}/build/cmake/android.toolchain.cmake)
EOF

export ANDROID_NDK_HOME="$NDK"
export ANDROID_NDK_ROOT="$NDK"
export ANDROID_NDK="$NDK"
export PATH="$TOOLCHAIN_BIN:$PATH"
export LIBCLANG_PATH="$NDK/toolchains/llvm/prebuilt/$HOST_TAG/lib"

# Translate target triple to the env-var spellings cargo / cmake-rs / cc-rs use.
TARGET_LOWER="${TARGET//-/_}"
TARGET_UPPER="${TARGET_LOWER^^}"

export "CMAKE_TOOLCHAIN_FILE_${TARGET_LOWER}=$TOOLCHAIN_FILE"
export "CC_${TARGET_LOWER}=$CC_NAME"
export "CXX_${TARGET_LOWER}=${CC_NAME}++"
export "AR_${TARGET_LOWER}=llvm-ar"
export "CARGO_TARGET_${TARGET_UPPER}_LINKER=$CC_NAME"
export "CARGO_TARGET_${TARGET_UPPER}_AR=llvm-ar"
export "CARGO_TARGET_${TARGET_UPPER}_RUSTFLAGS=-L $STUB_OUT"

rustup target add "$TARGET" >/dev/null 2>&1 || true

cargo build --release -p wie_libretro --target "$TARGET" "${@:2}"
