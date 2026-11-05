//! A real `relayd` for the CLI tests, wrapped so it is isolated and countable.
#![allow(dead_code)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Builds `relayd` once and returns where it is.
pub fn relayd() -> &'static PathBuf {
	static PATH: OnceLock<PathBuf> = OnceLock::new();
	PATH.get_or_init(|| {
		let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
		let built = std::process::Command::new(cargo)
			.args(["build", "--quiet", "-p", "relay-daemon"])
			.status()
			.expect("cargo runs");
		assert!(built.success(), "relayd builds");
		// target/<profile>/deps/<this test> -> target/<profile>/relayd
		let exe = std::env::current_exe().expect("exe");
		exe.parent()
			.and_then(|deps| deps.parent())
			.expect("target dir")
			.join("relayd")
	})
}

/// A script that records each launch in `spawns` and runs the real `relayd`
/// with a throwaway home, no LAN and a fresh identity.
pub fn wrapper(dir: &Path, spawns: &Path) -> PathBuf {
	let script = dir.join("relayd-wrapper");
	let body = format!(
		"#!/bin/sh\necho started >> '{}'\nexec '{}' \"$@\" --home '{}' --identity-store memory --listen-port 0 --no-discovery\n",
		spawns.display(),
		relayd().display(),
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
