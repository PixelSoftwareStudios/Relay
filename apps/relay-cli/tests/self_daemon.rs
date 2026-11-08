//! `relay daemon start` with no daemon file anywhere: the `relay` executable runs the daemon
//! itself (`--run-daemon`), on a private socket and home, never the user's.
#![cfg(unix)]

use std::process::{Command, Output};

fn relay(dir: &std::path::Path, args: &[&str]) -> Output {
	Command::new(env!("CARGO_BIN_EXE_relay"))
		.env("HOME", dir)
		.env_remove("RELAY_SOCKET")
		.env_remove("RELAY_TCP_PROFILE")
		.arg("--socket")
		.arg(dir.join("e.sock"))
		.args(args)
		.output()
		.expect("relay runs")
}

#[test]
fn the_cli_starts_its_own_executable_as_the_daemon() {
	let dir = tempfile::Builder::new()
		.prefix("rlys")
		.tempdir_in("/tmp")
		.expect("dir");
	let home = dir.path().join("home");
	let log = dir.path().join("relayd.log");
	let started = relay(
		dir.path(),
		&[
			"daemon",
			"start",
			"--log-file",
			log.to_str().expect("utf8"),
			"--",
			"--home",
			home.to_str().expect("utf8"),
			"--identity-store",
			"memory",
			"--listen-port",
			"0",
			"--no-discovery",
		],
	);
	assert!(
		started.status.success(),
		"{}",
		String::from_utf8_lossy(&started.stderr)
	);
	let status = relay(dir.path(), &["--no-autostart", "daemon", "status"]);
	assert!(status.status.success(), "the daemon answers");
	let stopped = relay(dir.path(), &["daemon", "stop"]);
	assert!(stopped.status.success());
	assert!(
		!dir.path().join("e.sock").exists(),
		"the endpoint is removed"
	);
}
