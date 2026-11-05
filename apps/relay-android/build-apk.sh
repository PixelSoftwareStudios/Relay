#!/usr/bin/env bash
# Spike APK without Gradle: cargo-ndk -> aapt2 -> zip -> zipalign -> apksigner.
# Env: ANDROID_NDK_HOME, ANDROID_HOME (platform + build-tools), OUT (scratch dir).
set -euo pipefail
ABIS=${ABIS:-arm64-v8a}
OUT=${OUT:?set OUT to a scratch directory}
BT=$(ls -d "$ANDROID_HOME"/build-tools/* | tail -1)
PLAT=$(ls -d "$ANDROID_HOME"/platforms/android-* | tail -1)
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$OUT"
args=(); for a in $ABIS; do args+=(-t "$a"); done
(cd "$here" && CARGO_TARGET_DIR="$OUT/target" cargo ndk "${args[@]}" -P 26 -o "$OUT/jniLibs" build --release)
"$BT/aapt2" link -o "$OUT/base.apk" -I "$PLAT/android.jar" --manifest "$here/android/AndroidManifest.xml" --min-sdk-version 26 --target-sdk-version 35
rm -rf "$OUT/stage" && mkdir -p "$OUT/stage" && cp -R "$OUT/jniLibs" "$OUT/stage/lib"
cp "$OUT/base.apk" "$OUT/unsigned.apk"
(cd "$OUT/stage" && zip -qr -D "$OUT/unsigned.apk" lib)
rm -f "$OUT/aligned.apk"; "$BT/zipalign" -p 4 "$OUT/unsigned.apk" "$OUT/aligned.apk"
[ -f "$OUT/debug.keystore" ] || keytool -genkeypair -keystore "$OUT/debug.keystore" -storepass android -keypass android -alias d -keyalg RSA -keysize 2048 -validity 3650 -dname "CN=spike" >/dev/null 2>&1
"$BT/apksigner" sign --ks "$OUT/debug.keystore" --ks-pass pass:android --out "$OUT/relay-spike.apk" "$OUT/aligned.apk"
"$BT/apksigner" verify "$OUT/relay-spike.apk" && ls -l "$OUT/relay-spike.apk"
