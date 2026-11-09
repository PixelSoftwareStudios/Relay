# Relay developer tasks. `just` lists them; `just android-apk` builds the phone app.

set shell := ["bash", "-euo", "pipefail", "-c"]

# Workspace version, the single source for artifact names and the APK's versionName.
version := `sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p;}' Cargo.toml`
android_sdk := env("ANDROID_HOME", env("HOME") / "Library/Android/sdk")
ndk_version := "28.2.13676358"
# SHA-256 of the Relay release signing certificate (public; also in docs/releasing.md).
release_cert := "0B:63:75:AD:94:A1:DC:96:5D:2A:0F:99:3C:06:51:61:C8:9E:8E:B2:DE:72:4C:81:C8:95:D2:5E:4A:EC:80:BB"

default:
	@just --list

# One-time: install the Android NDK and the Rust targets and tools the APK build needs.
android-setup:
	"{{android_sdk}}/cmdline-tools/latest/bin/sdkmanager" "ndk;{{ndk_version}}"
	rustup target add aarch64-linux-android x86_64-linux-android
	cargo install cargo-ndk --locked

# Build a debug-signed APK (arm64 phones) into dist/android/relay-<version>-android-<abis>.apk (arm64-v8a by default).
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
	# Gradle patches an existing APK in place and leaves the replaced entries behind as dead
	# space (a 49 MB APK had grown to 66 MB); packaging from scratch keeps it exact.
	rm -f apps/relay-android/app/build/outputs/apk/debug/app-debug.apk
	(cd apps/relay-android && ./gradlew assembleDebug -Prelay.version="{{version}}" -Prelay.abis="{{abis}}")
	python3 scripts/packaging/check_android_apk.py apps/relay-android/app/build/outputs/apk/debug/app-debug.apk
	mkdir -p dist/android
	apk="dist/android/relay-{{version}}-android-{{replace(abis, ",", "-")}}.apk"
	cp apps/relay-android/app/build/outputs/apk/debug/app-debug.apk "$apk"
	echo "$apk"

# The release APK: signed with the key in RELAY_KEYSTORE_FILE, _PASSWORD, RELAY_KEY_ALIAS, RELAY_KEY_PASSWORD
# (CI's android-signing environment). Refuses to run without them or if the result is debug-signed.
android-release:
	#!/usr/bin/env bash
	set -euo pipefail
	for v in RELAY_KEYSTORE_FILE RELAY_KEYSTORE_PASSWORD RELAY_KEY_ALIAS RELAY_KEY_PASSWORD; do
		[ -n "${!v:-}" ] || { echo "$v is not set: the release APK needs the signing key" >&2; exit 1; }
	done
	ndk="${ANDROID_NDK_HOME:-{{android_sdk}}/ndk/{{ndk_version}}}"
	[ -d "$ndk" ] || { echo "Android NDK not found at $ndk. Run: just android-setup" >&2; exit 1; }
	export ANDROID_HOME="{{android_sdk}}" ANDROID_NDK_HOME="$ndk"
	export CARGO_TARGET_DIR="$PWD/target/android" RELAY_HOST_TARGET_DIR="$PWD/target/android-host"
	rm -f apps/relay-android/app/build/outputs/apk/release/app-release.apk
	(cd apps/relay-android && ./gradlew assembleRelease -Prelay.version="{{version}}")
	apk=apps/relay-android/app/build/outputs/apk/release/app-release.apk
	python3 scripts/packaging/check_android_apk.py "$apk"
	python3 scripts/packaging/check_android_signature.py "$apk" --fingerprint "{{release_cert}}"
	mkdir -p dist/android
	cp "$apk" "dist/android/relay-{{version}}-android-arm64-v8a.apk"
	echo "dist/android/relay-{{version}}-android-arm64-v8a.apk"

# Build the APK and install it on the connected phone (USB or wireless debugging), then launch it.
android-install abis="arm64-v8a": (android-apk abis)
	adb install -r "dist/android/relay-{{version}}-android-{{replace(abis, ",", "-")}}.apk"
	adb shell am start -n dev.relay.app/.MainActivity

# One-time: cargo install cargo-nextest --locked. The real-clock tests are throttled in .config/nextest.toml.
# Run every test with nextest (one process per test), then the doctests it does not run.
test:
	cargo nextest run --workspace --no-fail-fast
	cargo test --workspace --doc

# Touches only target/: the dev incremental cache, superseded test binaries and old object files, android* dirs older than two weeks.
# Free disk without a full rebuild; anything removed is rebuilt on demand.
clean-cache:
	#!/usr/bin/env bash
	set -euo pipefail
	root="$PWD/target"
	[ -d "$root" ] || { echo "no target directory"; exit 0; }
	if pgrep -x cargo >/dev/null; then echo "a cargo build is running; try again when it has finished" >&2; exit 1; fi
	before=$(du -sk "$root" | cut -f1)
	rm -rf "${root:?}/debug/incremental"
	deps="${root:?}/debug/deps"
	if [ -d "$deps" ]; then
		# Every dependency change relinks each test binary under a new hash and leaves the old
		# executable, its dep-info and (on macOS) its object files behind. They are most of a
		# long-lived target. Per test source file, keep the newest binary and drop the rest.
		python3 -I - "$deps" <<'PY'
	import glob, os, sys
	deps = sys.argv[1]
	newest = {}
	stale = []
	for path in glob.glob(os.path.join(deps, "*-????????????????")):
		if not os.access(path, os.X_OK) or not os.path.isfile(path):
			continue
		try:
			with open(path + ".d") as info:
				source = info.readline().split(": ", 1)[1].split()[0]
		except (OSError, IndexError):
			continue
		mtime = os.path.getmtime(path)
		if source in newest and newest[source][0] > mtime:
			stale.append(path)
		else:
			if source in newest:
				stale.append(newest[source][1])
			newest[source] = (mtime, path)
	for path in stale:
		for leftover in [path, path + ".d"] + glob.glob(glob.escape(path) + ".*.rcgu.o"):
			try:
				os.remove(leftover)
			except OSError:
				pass
	PY
	fi
	find "${root:?}" -mindepth 1 -maxdepth 1 -type d -name 'android*' -mtime +14 -exec rm -rf {} +
	after=$(du -sk "$root" | cut -f1)
	echo "freed $(( (before - after) / 1024 )) MB"
