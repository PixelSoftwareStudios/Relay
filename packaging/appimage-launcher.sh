#!/bin/sh
# relay-desktop starts relayd from its own directory, so both stay in usr/bin of the image.
# AppRun runs this launcher from PATH (usr/bin), so command -v finds its directory.
dir=$(dirname "$(command -v "$0")")
exec "$dir/relay-desktop" "$@"
