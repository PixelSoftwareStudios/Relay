#!/bin/sh
# Builds relay.icns (every size macOS asks for) and relay-app-512.png from relay-app-macos.svg.
# Needs macOS `iconutil` and `resvg` (cargo install resvg --locked).
set -eu
cd "$(dirname "$0")"
set_dir=$(mktemp -d)/relay.iconset
mkdir -p "$set_dir"
render() { resvg -w "$1" -h "$1" relay-app-macos.svg "$set_dir/$2"; }
render 16   icon_16x16.png
render 32   icon_16x16@2x.png
render 32   icon_32x32.png
render 64   icon_32x32@2x.png
render 128  icon_128x128.png
render 256  icon_128x128@2x.png
render 256  icon_256x256.png
render 512  icon_256x256@2x.png
render 512  icon_512x512.png
render 1024 icon_512x512@2x.png
iconutil -c icns "$set_dir" -o relay.icns
resvg -w 512 -h 512 relay-app-macos.svg relay-app-512.png
rm -rf "$(dirname "$set_dir")"
