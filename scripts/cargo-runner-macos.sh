#!/bin/bash
# Cargo runner (see .cargo/config.toml): signs Relay dev binaries with the stable local
# identity before running them, then executes them. Never blocks a run: when the
# certificate does not exist yet (run scripts/dev-sign-macos.sh once) it just runs.
# Only relayd, relay and relay-desktop are touched; test binaries run as they are.
bin="$1"
case "$(basename "$bin")" in
relayd | relay | relay-desktop)
	RELAY_DEV_SIGN_NO_CREATE=1 "$(dirname "$0")/dev-sign-macos.sh" "$bin" >/dev/null 2>&1 || true
	;;
esac
exec "$@"
