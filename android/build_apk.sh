#!/usr/bin/env bash
# Build the OnlyGAZ Android APK (arm64-v8a, portrait, touch controls).
#
#   android/build_apk.sh            # release APK -> android/build/OnlyGAZ-<version>.apk
#   SKIP_CARGO=1 android/build_apk.sh   # repackage without recompiling Rust
#
# Works with either
#   * a normal Android Studio setup: ANDROID_HOME (SDK with build-tools and a
#     platform) + an NDK (ANDROID_NDK_HOME, or the newest one in $ANDROID_HOME/ndk)
#     and `rustup target add aarch64-linux-android`; or
#   * a minimal setup without Google downloads: ANDROID_NDK_SYSROOT pointing at
#     an NDK sysroot (usr/include + usr/lib), the system clang/lld, Debian/Ubuntu
#     packages aapt, zipalign, apksigner, dalvik-exchange, android-framework-res,
#     android-sdk-platform-23, and the Rust standard library built from source
#     (rust-src + RUSTC_BOOTSTRAP; used automatically when the target is not
#     installed through rustup).
#
# Signing: uses $ONLYGAZ_KEYSTORE / $ONLYGAZ_KEYSTORE_PASS / $ONLYGAZ_KEY_ALIAS
# when set, otherwise android/keystore/onlygaz-release.jks, which is created on
# the first run together with android/keystore/keystore.properties. Keep that
# folder safe: every update of the app must be signed with the same key.

set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
BUILD="$HERE/build"
TARGET=aarch64-linux-android
ABI=arm64-v8a
API=26
LIB_NAME=onlygaz

log() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

VERSION_NAME="$(sed -n 's/.*android:versionName="\([^"]*\)".*/\1/p' "$HERE/AndroidManifest.xml" | head -1)"
[ -n "$VERSION_NAME" ] || die "could not read versionName from AndroidManifest.xml"

SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"

# ---------------------------------------------------------------- tools
if printf '1\n' | sort -V >/dev/null 2>&1; then VERSION_SORT="sort -V"; else VERSION_SORT="sort"; fi
newest_dir() { ls -d "$1"/*/ 2>/dev/null | sed 's#/$##' | $VERSION_SORT | tail -1; }

BUILD_TOOLS=""
if [ -n "$SDK" ] && [ -d "$SDK/build-tools" ]; then
    BUILD_TOOLS="$(newest_dir "$SDK/build-tools")"
fi
tool() {
    if [ -n "$BUILD_TOOLS" ] && [ -x "$BUILD_TOOLS/$1" ]; then
        echo "$BUILD_TOOLS/$1"
    elif command -v "$1" >/dev/null 2>&1; then
        command -v "$1"
    else
        echo ""
    fi
}
AAPT="$(tool aapt)"
ZIPALIGN="$(tool zipalign)"
APKSIGNER="$(tool apksigner)"
D8="$(tool d8)"
DX="$(tool dalvik-exchange)"
[ -n "$DX" ] || DX="$(tool dx)"
[ -n "$AAPT" ] || die "aapt not found (install Android SDK build-tools or the 'aapt' package)"
[ -n "$ZIPALIGN" ] || die "zipalign not found"
[ -n "$APKSIGNER" ] || die "apksigner not found"
[ -n "$D8$DX" ] || die "neither d8 nor dx (dalvik-exchange) found"
command -v javac >/dev/null || die "javac (JDK 11+) not found"

# Platform used to compile resources and Java.
PLATFORM_JAR=""
if [ -n "$SDK" ] && [ -d "$SDK/platforms" ]; then
    PLATFORM_JAR="$(newest_dir "$SDK/platforms")/android.jar"
    [ -f "$PLATFORM_JAR" ] || PLATFORM_JAR=""
fi
if [ -z "$PLATFORM_JAR" ]; then
    PLATFORM_JAR=/usr/lib/android-sdk/platforms/android-23/android.jar
    RESOURCE_BASE=/usr/share/android-framework-res/framework-res.apk
    [ -f "$PLATFORM_JAR" ] || die "no Android platform (android.jar) found"
    [ -f "$RESOURCE_BASE" ] || die "framework-res.apk not found (package android-framework-res)"
else
    RESOURCE_BASE="$PLATFORM_JAR"
fi

# ---------------------------------------------------------------- native library
build_native() {
    local ndk="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-${NDK_HOME:-}}}"
    if [ -z "$ndk" ] && [ -n "$SDK" ] && [ -d "$SDK/ndk" ]; then
        ndk="$(newest_dir "$SDK/ndk")"
    fi
    local cc cxx ar
    if [ -n "$ndk" ] && [ -d "$ndk/toolchains/llvm/prebuilt" ]; then
        local bin
        bin="$(newest_dir "$ndk/toolchains/llvm/prebuilt")/bin"
        cc="$bin/${TARGET}${API}-clang"
        cxx="$bin/${TARGET}${API}-clang++"
        ar="$bin/llvm-ar"
        log "NDK: $ndk"
    elif [ -n "${ANDROID_NDK_SYSROOT:-}" ]; then
        command -v clang >/dev/null || die "clang not found"
        command -v ld.lld >/dev/null || die "ld.lld not found"
        local wrappers="$BUILD/toolchain"
        mkdir -p "$wrappers"
        for driver in clang clang++; do
            cat >"$wrappers/${TARGET}${API}-$driver" <<EOF
#!/bin/sh
exec $driver --target=${TARGET}${API} --sysroot="$ANDROID_NDK_SYSROOT" -fuse-ld=lld -Wno-unused-command-line-argument "\$@"
EOF
            chmod +x "$wrappers/${TARGET}${API}-$driver"
        done
        cc="$wrappers/${TARGET}${API}-clang"
        cxx="$wrappers/${TARGET}${API}-clang++"
        ar="$(command -v llvm-ar || echo ar)"
        log "NDK sysroot: $ANDROID_NDK_SYSROOT (system clang)"
    else
        die "set ANDROID_NDK_HOME (or ANDROID_HOME with an installed NDK) or ANDROID_NDK_SYSROOT"
    fi

    export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$cc"
    export CC_aarch64_linux_android="$cc"
    export CXX_aarch64_linux_android="$cxx"
    export AR_aarch64_linux_android="$ar"

    local build_std=()
    if rustup target list --installed 2>/dev/null | grep -qx "$TARGET"; then
        log "Rust target $TARGET installed through rustup"
    elif [ -d "$(rustc --print sysroot)/lib/rustlib/src/rust/library" ]; then
        log "Rust target not installed: building the standard library from rust-src"
        export RUSTC_BOOTSTRAP=1
        build_std=(-Zbuild-std=std,panic_abort)
    else
        die "run: rustup target add $TARGET"
    fi

    log "compiling lib${LIB_NAME}.so (release, this takes a while the first time)"
    (cd "$HERE" && cargo build --release --target "$TARGET" ${build_std[@]+"${build_std[@]}"})
}

if [ "${SKIP_CARGO:-0}" != "1" ]; then
    build_native
fi
SO="$HERE/target/$TARGET/release/lib${LIB_NAME}.so"
[ -f "$SO" ] || die "missing $SO"

# ---------------------------------------------------------------- staging
STAGE="$BUILD/stage"
rm -rf "$STAGE"
mkdir -p "$STAGE/lib/$ABI" "$BUILD"
cp "$SO" "$STAGE/lib/$ABI/lib${LIB_NAME}.so"

ASSETS="$BUILD/assets"
if [ "${REUSE_ASSETS:-0}" = "1" ] && [ -d "$ASSETS" ]; then
    log "reusing prepared assets in $ASSETS"
elif python3 -c "import PIL" >/dev/null 2>&1; then
    log "preparing phone-sized assets"
    python3 "$HERE/tools/prepare_assets.py" "$ROOT/assets" "$ASSETS" | tail -1
else
    log "Pillow not installed: packaging the desktop textures unchanged (pip install pillow)"
    rm -rf "$ASSETS"
    cp -R "$ROOT/assets" "$ASSETS"
fi

log "compiling MainActivity"
CLASSES="$BUILD/classes"
rm -rf "$CLASSES"
mkdir -p "$CLASSES"
javac --release 8 -Xlint:-options -nowarn -classpath "$PLATFORM_JAR" -d "$CLASSES" \
    "$HERE/java/kz/webnet/onlygaz/MainActivity.java" 2>&1 | grep -v "Picked up JAVA_TOOL_OPTIONS" || true
[ -f "$CLASSES/kz/webnet/onlygaz/MainActivity.class" ] || die "javac failed"
if [ -n "$D8" ]; then
    (cd "$CLASSES" && "$D8" --release --min-api "$API" --lib "$PLATFORM_JAR" --output "$STAGE" \
        $(find . -name '*.class'))
else
    "$DX" --dex --min-sdk-version="$API" --output="$STAGE/classes.dex" "$CLASSES"
fi
[ -f "$STAGE/classes.dex" ] || die "dex conversion failed"

# ---------------------------------------------------------------- package
UNSIGNED="$BUILD/unsigned.apk"
ALIGNED="$BUILD/aligned.apk"
OUT="$BUILD/OnlyGAZ-${VERSION_NAME}.apk"
rm -f "$UNSIGNED" "$ALIGNED" "$OUT"

log "packaging resources and assets"
"$AAPT" package -f \
    -M "$HERE/AndroidManifest.xml" \
    -S "$HERE/res" \
    -A "$ASSETS" \
    -I "$RESOURCE_BASE" \
    -F "$UNSIGNED"
(cd "$STAGE" && "$AAPT" add "$UNSIGNED" classes.dex "lib/$ABI/lib${LIB_NAME}.so" >/dev/null)

log "aligning"
"$ZIPALIGN" -f -p 4 "$UNSIGNED" "$ALIGNED"

# ---------------------------------------------------------------- sign
KEYSTORE="${ONLYGAZ_KEYSTORE:-$HERE/keystore/onlygaz-release.jks}"
KEY_ALIAS="${ONLYGAZ_KEY_ALIAS:-onlygaz}"
PROPS="$HERE/keystore/keystore.properties"
if [ -z "${ONLYGAZ_KEYSTORE_PASS:-}" ] && [ -f "$PROPS" ]; then
    ONLYGAZ_KEYSTORE_PASS="$(sed -n 's/^storePassword=//p' "$PROPS")"
fi
if [ ! -f "$KEYSTORE" ]; then
    [ -z "${ONLYGAZ_KEYSTORE:-}" ] || die "keystore $KEYSTORE not found"
    command -v keytool >/dev/null || die "keytool (JDK) not found"
    log "creating a new release key: $KEYSTORE"
    mkdir -p "$(dirname "$KEYSTORE")"
    # (head reads a fixed amount, so no SIGPIPE trips `set -o pipefail`.)
    ONLYGAZ_KEYSTORE_PASS="$(head -c 512 /dev/urandom | LC_ALL=C tr -dc 'A-Za-z0-9' | cut -c1-24)"
    keytool -genkeypair -noprompt -keystore "$KEYSTORE" -storetype PKCS12 \
        -alias "$KEY_ALIAS" -keyalg RSA -keysize 4096 -validity 10000 \
        -storepass "$ONLYGAZ_KEYSTORE_PASS" -keypass "$ONLYGAZ_KEYSTORE_PASS" \
        -dname "CN=OnlyGAZ, O=WebNet.kz, C=KZ" 2>&1 | grep -v "Picked up JAVA_TOOL_OPTIONS" || true
    (
        umask 077
        printf 'storeFile=onlygaz-release.jks\nkeyAlias=%s\nstorePassword=%s\nkeyPassword=%s\n' \
            "$KEY_ALIAS" "$ONLYGAZ_KEYSTORE_PASS" "$ONLYGAZ_KEYSTORE_PASS" >"$PROPS"
    )
    chmod 600 "$KEYSTORE"
fi
[ -n "${ONLYGAZ_KEYSTORE_PASS:-}" ] || die "keystore password unknown: set ONLYGAZ_KEYSTORE_PASS"

log "signing"
"$APKSIGNER" sign --ks "$KEYSTORE" --ks-key-alias "$KEY_ALIAS" \
    --ks-pass "pass:$ONLYGAZ_KEYSTORE_PASS" --key-pass "pass:$ONLYGAZ_KEYSTORE_PASS" \
    --min-sdk-version "$API" --v1-signing-enabled true --v2-signing-enabled true \
    --out "$OUT" "$ALIGNED" 2>&1 | grep -v "Picked up JAVA_TOOL_OPTIONS" || true
[ -f "$OUT" ] || die "signing failed"
"$APKSIGNER" verify --min-sdk-version "$API" "$OUT" 2>&1 | grep -v "Picked up JAVA_TOOL_OPTIONS" || true
rm -f "$UNSIGNED" "$ALIGNED"

SIZE="$(du -h "$OUT" | cut -f1)"
log "done: $OUT ($SIZE)"
echo "Install on a phone with USB debugging:  adb install -r \"$OUT\""
