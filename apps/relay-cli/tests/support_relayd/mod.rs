//! A real `relayd` for the CLI tests, wrapped so it is isolated and countable.
#![allow(dead_code)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A script that runs this package's own `relay` executable as the daemon (`--run-daemon`),
/// the same way the CLI starts it. No second build runs inside a test: a nested `cargo build`
/// recompiled dependencies (feature unification differs) and serialised every test process.
pub fn relayd(dir: &Path) -> PathBuf {
	let script = dir.join("relayd-shim");
	let body = format!(
		"#!/bin/sh\nexec '{}' --run-daemon \"$@\"\n",
		env!("CARGO_BIN_EXE_relay"),
	);
	std::fs::write(&script, body).expect("script");
	std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
	script
}

/// A script that records each launch in `spawns` and runs the real `relayd`
/// with a throwaway home, no LAN and a fresh identity.
pub fn wrapper(dir: &Path, spawns: &Path) -> PathBuf {
	let script = dir.join("relayd-wrapper");
	let body = format!(
		"#!/bin/sh\necho started >> '{}'\nexec '{}' --run-daemon \"$@\" --home '{}' --identity-store memory --listen-port 0 --no-discovery\n",
		spawns.display(),
		env!("CARGO_BIN_EXE_relay"),
		dir.join("home").display(),
	);
	std::fs::write(&script, body).expect("script");
	std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
	script
}

/// Launches recorded so far.
pub fn launches(spawns: &Path) -> usize {
	std::fs::read_to_string(spawns).map_or(0, |text| text.lines().count())
}
