//! `relay config` and `relay hotkey set` against a temporary settings directory.

mod common;

use clap::Parser;
use common::FakeControl;
#[cfg(unix)]
use common::engine::EngineHarness;
use relay_cli::cli::{Cli, CliError, GlobalArgs, exit};
use relay_cli::commands::Out;
use relay_cli::commands::config::{run_in, set_live};
use relay_config::{Paths, Storage};

/// `--socket` exists on Unix only; elsewhere the CLI reaches an engine through a TCP profile.
#[cfg(unix)]
const ENDPOINT_FLAG: &str = "--socket";
#[cfg(not(unix))]
const ENDPOINT_FLAG: &str = "--tcp-profile";

fn global(socket: &std::path::Path) -> GlobalArgs {
	Cli::try_parse_from(["relay", ENDPOINT_FLAG, socket.to_str().unwrap(), "status"])
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

#[cfg(unix)]
#[tokio::test]
async fn with_an_engine_running_every_key_goes_through_it_with_its_outcome() {
	let s = setup();
	let engine = EngineHarness::start().await;
	let live = global(&engine.socket);
	let out = config(&s, &live, &["set", "profile.display_name", "Kitchen"])
		.await
		.unwrap();
	assert_eq!(out.trim(), "profile.display_name: applied live");
	let out = config(&s, &live, &["set", "network.transfer_port", "47900"])
		.await
		.unwrap();
	assert!(
		out.contains("network.transfer_port: saved, needs restart (read at the next start)"),
		"{out}"
	);
	let err = config(&s, &live, &["set", "network.transfer_port", "x"]).await;
	assert!(err.is_ok(), "the fake engine accepts any text");
	assert_eq!(engine.log()[0], "set_settings profile.display_name=Kitchen");
	assert!(
		!s.paths.database_file().exists(),
		"the engine owns the store; the CLI wrote nothing"
	);
}

#[cfg(unix)]
#[tokio::test]
async fn only_sensitive_keys_need_the_daemon_stopped() {
	let s = setup();
	let engine = EngineHarness::start().await;
	let live = global(&engine.socket);
	let err = config(&s, &live, &["set", "network.allow_wan", "true"])
		.await
		.unwrap_err();
	assert_eq!(err.exit_code(), exit::REJECTED);
	assert_eq!(err.code(), "SETTING_REJECTED");
	// Setting a restricted key to its current value is a no-op, not a refusal.
	let same = config(
		&s,
		&live,
		&["set", "identity.key_storage", "os_credential_store"],
	)
	.await
	.unwrap();
	assert!(
		same.contains("identity.key_storage: applied live"),
		"{same}"
	);
}

#[cfg(unix)]
#[tokio::test]
async fn import_reports_each_key_and_sets_restricted_ones_aside() {
	let s = setup();
	let engine = EngineHarness::start().await;
	let live = global(&engine.socket);
	let file = s.dir.path().join("in.toml");
	std::fs::write(
		&file,
		"[profile]\ndisplay_name = \"X\"\n[network]\nallow_wan = true\n",
	)
	.unwrap();
	let mut out = Vec::new();
	let mut o = Out::new(false, &mut out);
	let cmd = relay_cli::cli::ConfigCommand::Import { file: file.clone() };
	let err = run_in(&live, &s.paths, cmd, &mut o).await.unwrap_err();
	let text = String::from_utf8(out).unwrap();
	assert_eq!(err.exit_code(), exit::REJECTED);
	assert!(
		text.contains("profile.display_name: applied live"),
		"{text}"
	);
	assert!(
		text.contains("network.allow_wan: rejected: security-sensitive"),
		"{text}"
	);
	let log = engine.log();
	assert_eq!(log.len(), 2, "{log:?}");
	assert!(log[0].contains("network.allow_wan=true"), "{log:?}");
	assert_eq!(
		log[1], "set_settings profile.display_name=X",
		"only the changed keys, minus the refused one"
	);
}

#[cfg(unix)]
#[tokio::test]
async fn get_and_export_read_the_running_daemon() {
	let s = setup();
	let engine = EngineHarness::start().await;
	let live = global(&engine.socket);
	let name = config(&s, &live, &["get", "profile.display_name"])
		.await
		.unwrap();
	assert_eq!(name.trim(), "Desk");
	let all = config(&s, &live, &["get"]).await.unwrap();
	assert!(
		all.contains("identity.key_storage = os_credential_store"),
		"{all}"
	);
	let err = config(&s, &live, &["get", "audio.nope"]).await.unwrap_err();
	assert_eq!(err.exit_code(), exit::USAGE);
	assert!(
		!s.paths.database_file().exists(),
		"nothing read from the database"
	);
}

#[tokio::test]
async fn set_live_prints_the_scripted_outcomes_and_fails_on_a_rejection() {
	use relay_api::pb::{SettingOutcome, SettingResult};
	let mut c = FakeControl::new();
	c.setting_results.insert(
		"network.transfer_port".into(),
		SettingResult {
			key: "network.transfer_port".into(),
			outcome: SettingOutcome::Rejected as i32,
			reason: "must be 1024 to 65535".into(),
		},
	);
	let mut buffer = Vec::new();
	let mut out = Out::new(true, &mut buffer);
	let err = set_live(
		&mut c,
		vec![
			("network.transfer_port".into(), "1".into()),
			("audio.playback_gain".into(), "2".into()),
		],
		&mut out,
	)
	.await
	.unwrap_err();
	assert_eq!(err.exit_code(), exit::REJECTED);
	let json: serde_json::Value = serde_json::from_slice(&buffer).unwrap();
	assert_eq!(json[0]["key"], "audio.playback_gain");
	assert_eq!(json[0]["outcome"], "applied_live");
	assert_eq!(json[1]["outcome"], "rejected");
	assert_eq!(json[1]["reason"], "must be 1024 to 65535");
}
