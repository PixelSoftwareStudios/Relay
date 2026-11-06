#!/usr/bin/env bash
# Package the built binaries for one target into dist/relay-v<version>-<target>.<tar.gz|zip>.
# Usage: scripts/packaging/archive.sh <rust-target-triple>   (after cargo build --release --target <triple>)
set -euo pipefail

target="${1:?usage: archive.sh <rust-target-triple>}"
version=$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p;}' Cargo.toml)
name="relay-v${version}-${target}"
bin="target/${target}/release"
exe=""
case "$target" in *windows*) exe=".exe" ;; esac

stage="dist/${name}"
rm -rf "$stage"
mkdir -p "$stage/bin" "$stage/share/doc/relay"
for program in relayd relay relay-desktop; do
	cp "${bin}/${program}${exe}" "$stage/bin/"
done
cp LICENSE README.md "$stage/share/doc/relay/"
cp assets/font/OFL-*.txt assets/font/LICENSE-phosphor.txt "$stage/share/doc/relay/"
[ -f THIRD_PARTY_LICENSES ] && cp THIRD_PARTY_LICENSES "$stage/share/doc/relay/"

case "$target" in
*windows*) (cd dist && 7z a -tzip -bso0 "${name}.zip" "${name}") ;;
*) tar -C dist -czf "dist/${name}.tar.gz" "$name" ;;
esac
rm -rf "$stage"
ls -l dist/"${name}".*
