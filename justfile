# Relay developer tasks. `just` lists them; `just android-apk` builds the phone app.

set shell := ["bash", "-euo", "pipefail", "-c"]

# Workspace version, the single source for artifact names and the APK's versionName.
version := `sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p;}' Cargo.toml`
android_sdk := env("ANDROID_HOME", env("HOME") / "Library/Android/sdk")
ndk_version := "28.2.13676358"

default:
	@just --list

# One-time: install the Android NDK and the Rust targets and tools the APK build needs.
android-setup:
	"{{android_sdk}}/cmdline-tools/latest/bin/sdkmanager" "ndk;{{ndk_version}}"
	rustup target add aarch64-linux-android x86_64-linux-android
	cargo install cargo-ndk --locked

# Build a debug-signed APK (arm64 phones) into dist/android/relay-v<version>.apk.
# Pass ABIs to include the x86_64 emulator too: just android-apk arm64-v8a,x86_64
android-apk abis="arm64-v8a":
	#!/usr/bin/env bash
	set -euo pipefail
	ndk="${ANDROID_NDK_HOME:-{{android_sdk}}/ndk/{{ndk_version}}}"
	if [ ! -d "$ndk" ]; then
		echo "Android NDK not found at $ndk. Run: just android-setup" >&2
		exit 1
	fi
	export ANDROID_HOME="{{android_sdk}}" ANDROID_NDK_HOME="$ndk"
	export CARGO_TARGET_DIR="$PWD/target/android" RELAY_HOST_TARGET_DIR="$PWD/target/android-host"
	(cd apps/relay-android && ./gradlew assembleDebug -Prelay.version="{{version}}" -Prelay.abis="{{abis}}")
	mkdir -p dist/android
	cp apps/relay-android/app/build/outputs/apk/debug/app-debug.apk "dist/android/relay-v{{version}}.apk"
	echo "dist/android/relay-v{{version}}.apk"

# Build the APK and install it on the connected phone (USB or wireless debugging), then launch it.
android-install abis="arm64-v8a": (android-apk abis)
	adb install -r "dist/android/relay-v{{version}}.apk"
	adb shell am start -n dev.relay.app/.MainActivity

# One-time: cargo install cargo-nextest --locked. The real-clock tests are throttled in .config/nextest.toml.
# Run every test with nextest (one process per test), then the doctests it does not run.
test:
	cargo nextest run --workspace --no-fail-fast
	cargo test --workspace --doc

# Never removes anything but the dev incremental cache and target/android* dirs older than two weeks.
# Free disk without a full rebuild.
clean-cache:
	#!/usr/bin/env bash
	set -euo pipefail
	root="$PWD/target"
	[ -d "$root" ] || { echo "no target directory"; exit 0; }
	before=$(du -sk "$root" | cut -f1)
	rm -rf "${root:?}/debug/incremental"
	find "${root:?}" -mindepth 1 -maxdepth 1 -type d -name 'android*' -mtime +14 -exec rm -rf {} +
	after=$(du -sk "$root" | cut -f1)
	echo "freed $(( (before - after) / 1024 )) MB"
