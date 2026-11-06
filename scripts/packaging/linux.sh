#!/usr/bin/env bash
# Build the Linux packages for one target from an existing release build:
# dist/relay-v<version>-<arch>.{deb,rpm,AppImage}.
# Usage: scripts/packaging/linux.sh <x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu>
# Needs nfpm and appimagetool on PATH (CI installs both; see .github/workflows/release.yml).
set -euo pipefail

target="${1:?usage: linux.sh <rust-target-triple>}"
case "$target" in
x86_64-unknown-linux-gnu) export ARCH=amd64 rpm_arch=x86_64 appimage_arch=x86_64 ;;
aarch64-unknown-linux-gnu) export ARCH=arm64 rpm_arch=aarch64 appimage_arch=aarch64 ;;
*) echo "not a Linux target: $target" >&2; exit 1 ;;
esac

export VERSION
VERSION=$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p;}' Cargo.toml)
bin="target/${target}/release"
export STAGE="$PWD/dist/stage-${target}"

rm -rf "$STAGE"
mkdir -p "$STAGE/usr/bin" "$STAGE/usr/share/applications" "$STAGE/usr/share/doc/relay" dist
for size in 256 512; do
	mkdir -p "$STAGE/usr/share/icons/hicolor/${size}x${size}/apps"
	cp "assets/icon/relay-mark-${size}.png" "$STAGE/usr/share/icons/hicolor/${size}x${size}/apps/relay.png"
done
for program in relayd relay relay-desktop; do
	install -m 755 "${bin}/${program}" "$STAGE/usr/bin/${program}"
done
cp packaging/linux/relay.desktop "$STAGE/usr/share/applications/"
cp LICENSE README.md assets/font/OFL-*.txt assets/font/LICENSE-phosphor.txt "$STAGE/usr/share/doc/relay/"
if [ -f THIRD_PARTY_LICENSES ]; then cp THIRD_PARTY_LICENSES "$STAGE/usr/share/doc/relay/"; fi

nfpm package --config packaging/linux/nfpm.yaml --packager deb --target "dist/relay-v${VERSION}-${ARCH}.deb"
nfpm package --config packaging/linux/nfpm.yaml --packager rpm --target "dist/relay-v${VERSION}-${rpm_arch}.rpm"

# AppImage: the same tree, started through the desktop app. The host supplies GTK, ALSA and
# the display libraries, as for the .deb and .rpm.
appdir="dist/Relay-${target}.AppDir"
rm -rf "$appdir"
mkdir -p "$appdir"
cp -R "$STAGE/usr" "$appdir/usr"
cp packaging/linux/relay.desktop "$appdir/relay.desktop"
cp assets/icon/relay-mark-256.png "$appdir/relay.png"
ln -s usr/bin/relay-desktop "$appdir/AppRun"
ARCH="$appimage_arch" appimagetool --appimage-extract-and-run "$appdir" "dist/relay-v${VERSION}-${appimage_arch}.AppImage"
rm -rf "$appdir" "$STAGE"
ls -l dist/relay-v"${VERSION}"-*
