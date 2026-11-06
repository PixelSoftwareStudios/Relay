#!/bin/sh
# relay-desktop starts relayd from its own directory, so both stay in $APPDIR/usr/bin.
exec "$APPDIR/usr/bin/relay-desktop" "$@"
