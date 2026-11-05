//! `relay config` and `relay hotkey set` against a temporary settings directory.

mod common;

use clap::Parser;
use common::FakeControl;
use common::engine::EngineHarness;
use relay_cli::cli::{Cli, CliError, GlobalArgs, exit};
use relay_cli::commands::Out;
use relay_cli::commands::config::{LIVE_KEYS, run_in, set_live};
use relay_config::{Paths, Storage};

fn global(socket: &std::path::Path) -> GlobalArgs {
	Cli::try_parse_from(["relay", "--socket", socket.to_str().unwrap(), "status"])
		.unwrap()
		.global
}

struct Setup {
	dir: tempfile::TempDir,
	paths: Paths,
	global: GlobalArgs,
}

fn setup() -> Setup {
	let dir = tempfile::tempdir().unwrap();
	let paths = Paths::under(dir.path().join("cfg"));
	let global = global(&dir.path().join("no-engine.sock"));
	Setup { dir, paths, global }
}

async fn config(s: &Setup, global: &GlobalArgs, args: &[&str]) -> Result<String, CliError> {
	let mut argv = vec!["relay", "config"];
	argv.extend_from_slice(args);
	let cmd = match Cli::try_parse_from(argv).unwrap().command {
		relay_cli::cli::Command::Config(cmd) => cmd,
		other => panic!("{other:?}"),
	};
	let mut buffer = Vec::new();
	let mut out = Out::new(false, &mut buffer);
	run_in(global, &s.paths, cmd, &mut out).await?;
	Ok(String::from_utf8(buffer).unwrap())
}

#[tokio::test]
async fn get_reads_defaults_without_creating_anything() {
	let s = setup();
	let value = config(&s, &s.global, &["get", "audio.microphone_gain"])
		.await
		.unwrap();
	assert_eq!(value.trim(), "1.0");
	let all = config(&s, &s.global, &["get"]).await.unwrap();
	assert!(
		all.contains("hotkeys.push_to_talk = Ctrl+Alt+Space"),
		"{all}"
	);
	assert!(
		!s.paths.database_file().exists(),
		"reading must not create the database"
	);
	let err = config(&s, &s.global, &["get", "audio.nope"])
		.await
		.unwrap_err();
	assert_eq!(err.exit_code(), exit::USAGE);
}

#[tokio::test]
async fn set_without_an_engine_writes_through_the_settings_lock() {
	let s = setup();
	let shown = config(&s, &s.global, &["set", "audio.microphone_gain", "1.5"])
		.await
		.unwrap();
	assert_eq!(shown.trim(), "1.5");
	let again = config(&s, &s.global, &["get", "audio.microphone_gain"])
		.await
		.unwrap();
	assert_eq!(again.trim(), "1.5");
	let bad = config(&s, &s.global, &["set", "audio.microphone_gain", "9"])
		.await
		.unwrap_err();
	assert_eq!(bad.exit_code(), exit::USAGE);
	let unknown = config(&s, &s.global, &["set", "nope.key", "1"])
		.await
		.unwrap_err();
	assert_eq!(unknown.exit_code(), exit::USAGE);
}

#[tokio::test]
async fn set_fails_clearly_when_another_process_holds_the_lock() {
	let s = setup();
	let _holder = Storage::open(&s.paths).unwrap();
	let err = config(&s, &s.global, &["set", "diagnostics.log_level", "debug"])
		.await
		.unwrap_err();
	assert!(
		err.to_string().contains("locked by another process"),
		"{err}"
	);
	assert_eq!(err.exit_code(), exit::FAILURE);
}

#[tokio::test]
async fn export_and_import_round_trip_through_a_file() {
	let s = setup();
	config(&s, &s.global, &["set", "profile.display_name", "Desk"])
		.await
		.unwrap();
	let file = s.dir.path().join("settings.toml");
	config(&s, &s.global, &["export", file.to_str().unwrap()])
		.await
		.unwrap();
	let text = std::fs::read_to_string(&file).unwrap();
	assert!(
		text.contains("display_name = \"Desk\"") && text.contains("Ctrl+Alt+Space"),
		"{text}"
	);

	config(&s, &s.global, &["set", "profile.display_name", "Other"])
		.await
		.unwrap();
	config(&s, &s.global, &["import", file.to_str().unwrap()])
		.await
		.unwrap();
	let name = config(&s, &s.global, &["get", "profile.display_name"])
		.await
		.unwrap();
	assert_eq!(name.trim(), "Desk");

	let stdout = config(&s, &s.global, &["export"]).await.unwrap();
	assert!(stdout.starts_with("# Relay settings export"), "{stdout}");

	std::fs::write(&file, "[nonsense]\nx = 1\n").unwrap();
	assert!(
		config(&s, &s.global, &["import", file.to_str().unwrap()])
			.await
			.is_err()
	);
}

#[tokio::test]
async fn with_an_engine_running_live_settings_route_through_it() {
	let s = setup();
	let engine = EngineHarness::start().await;
	let live = global(&engine.socket);
	let out = config(&s, &live, &["set", "profile.display_name", "Kitchen"])
		.await
		.unwrap();
	assert!(out.contains("applied"), "{out}");
	config(&s, &live, &["set", "privacy.advert_mode", "hidden"])
		.await
		.unwrap();
	let log = engine.log();
	assert_eq!(log[0], "profile Kitchen");
	assert!(
		log[1].starts_with("privacy inbound=None advert=Some(2)"),
		"{log:?}"
	);
	assert!(
		!s.paths.database_file().exists(),
		"the engine owns the store; the CLI wrote nothing"
	);
}

#[tokio::test]
async fn settings_the_engine_cannot_change_live_and_import_are_refused_while_it_runs() {
	let s = setup();
	let engine = EngineHarness::start().await;
	let live = global(&engine.socket);
	let err = config(&s, &live, &["set", "diagnostics.log_level", "debug"])
		.await
		.unwrap_err();
	assert_eq!(err.exit_code(), exit::USAGE);
	for key in LIVE_KEYS {
		assert!(err.to_string().contains(key), "{err}");
	}
	let file = s.dir.path().join("in.toml");
	std::fs::write(&file, "[profile]\ndisplay_name = \"X\"\n").unwrap();
	let err = config(&s, &live, &["import", file.to_str().unwrap()])
		.await
		.unwrap_err();
	assert!(
		err.to_string().contains("stop it before importing"),
		"{err}"
	);
	assert!(engine.log().is_empty());
}

#[tokio::test]
async fn live_values_are_validated_before_the_engine_sees_them() {
	let mut c = FakeControl::new();
	let mut buffer = Vec::new();
	let mut out = Out::new(false, &mut buffer);
	for (key, value) in [
		("audio.microphone_gain", "loud"),
		("audio.playback_gain", "5"),
		("privacy.inbound_policy", "nobody"),
		("privacy.advert_mode", "shy"),
	] {
		let err = set_live(&mut c, key, value, &mut out).await.unwrap_err();
		assert_eq!(err.exit_code(), exit::USAGE, "{key}");
	}
	assert!(c.log.is_empty());
	set_live(
		&mut c,
		"privacy.inbound_policy",
		"shared_channel_only",
		&mut out,
	)
	.await
	.unwrap();
	assert_eq!(c.log[0], "privacy Some(SharedChannelOnly) None");
}
