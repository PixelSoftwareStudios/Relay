//! `relay limits` and the `limits.*` keys of `relay config`.

mod common;

use clap::Parser;
#[cfg(unix)]
use common::engine::EngineHarness;
use relay_cli::cli::{Cli, CliError, GlobalArgs, exit};
use relay_cli::commands::Out;
use relay_cli::commands::config::run_in as config_in;
use relay_cli::commands::limits::run_in;
use relay_config::Paths;
use relay_config::prefs::limits::SPECS;

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

async fn limits(s: &Setup, global: &GlobalArgs, json: bool) -> String {
	let mut buffer = Vec::new();
	let mut out = Out::new(json, &mut buffer);
	run_in(global, &s.paths, &mut out).await.unwrap();
	String::from_utf8(buffer).unwrap()
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
	config_in(global, &s.paths, cmd, &mut out).await?;
	Ok(String::from_utf8(buffer).unwrap())
}

#[test]
fn the_command_is_part_of_the_cli() {
	let cli = Cli::try_parse_from(["relay", "limits"]).unwrap();
	assert!(matches!(cli.command, relay_cli::cli::Command::Limits));
}

#[tokio::test]
async fn the_table_lists_every_limit_with_current_default_and_range() {
	let s = setup();
	let text = limits(&s, &s.global, false).await;
	let header = text.lines().next().unwrap();
	for column in ["LIMIT", "CURRENT", "DEFAULT", "RANGE", "IN USE"] {
		assert!(header.contains(column), "{header}");
	}
	for spec in &SPECS {
		let line = text
			.lines()
			.find(|l| l.starts_with(spec.key))
			.unwrap_or_else(|| panic!("{} missing in\n{text}", spec.key));
		assert!(
			line.contains(&format!("{} {}", spec.default, spec.unit)),
			"{line}"
		);
		assert!(
			line.contains(&format!("{}..{} {}", spec.min, spec.max, spec.unit)),
			"{line}"
		);
	}
	assert!(!s.paths.database_file().exists(), "reading creates nothing");
}

#[tokio::test]
async fn json_has_one_object_per_limit_and_follows_config_set() {
	let s = setup();
	config(&s, &s.global, &["set", "limits.avatar_cache_mib", "32"])
		.await
		.unwrap();
	let rows: serde_json::Value = serde_json::from_str(&limits(&s, &s.global, true).await).unwrap();
	let rows = rows.as_array().unwrap();
	assert_eq!(rows.len(), SPECS.len());
	assert_eq!(rows[0]["key"], "limits.avatar_cache_mib");
	assert_eq!(rows[0]["current"], 32);
	assert_eq!(rows[0]["default"], 128);
	assert_eq!(rows[0]["min"], 16);
	assert_eq!(rows[0]["max"], 2048);
	assert_eq!(rows[0]["unit"], "MiB");
	assert!(rows[0].get("used_bytes").is_none(), "no daemon, no usage");
	assert_eq!(rows[1]["current"], 64);
}

#[tokio::test]
async fn config_get_set_export_and_import_cover_the_limits() {
	let s = setup();
	let all = config(&s, &s.global, &["get"]).await.unwrap();
	for spec in &SPECS {
		assert!(
			all.contains(&format!("{} = {}", spec.key, spec.default)),
			"{all}"
		);
	}
	let bad = config(&s, &s.global, &["set", "limits.avatar_cache_mib", "1"])
		.await
		.unwrap_err();
	assert_eq!(bad.exit_code(), exit::USAGE);
	assert!(bad.to_string().contains("limits.avatar_cache_mib"), "{bad}");
	let pair = config(&s, &s.global, &["set", "limits.messages_retained_mib", "9"])
		.await
		.unwrap_err();
	assert!(
		pair.to_string().contains("limits.messages_session_mib"),
		"{pair}"
	);

	config(&s, &s.global, &["set", "limits.history_max_entries", "10"])
		.await
		.unwrap();
	let file = s.dir.path().join("settings.toml");
	config(&s, &s.global, &["export", file.to_str().unwrap()])
		.await
		.unwrap();
	assert!(
		std::fs::read_to_string(&file)
			.unwrap()
			.contains("history_max_entries = 10")
	);
	config(&s, &s.global, &["set", "limits.history_max_entries", "99"])
		.await
		.unwrap();
	config(&s, &s.global, &["import", file.to_str().unwrap()])
		.await
		.unwrap();
	let back = config(&s, &s.global, &["get", "limits.history_max_entries"])
		.await
		.unwrap();
	assert_eq!(back.trim(), "10");
}

#[cfg(unix)]
#[tokio::test]
async fn with_a_daemon_the_table_shows_what_is_in_use() {
	let s = setup();
	let engine = EngineHarness::start().await;
	let live = global(&engine.socket);
	let rows: serde_json::Value = serde_json::from_str(&limits(&s, &live, true).await).unwrap();
	assert_eq!(rows[0]["used_bytes"], 3 * 1024 * 1024);
	let text = limits(&s, &live, false).await;
	assert!(text.contains("3.0 MiB"), "{text}");
}
