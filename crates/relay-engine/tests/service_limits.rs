#![cfg(unix)]
//! `limits.*` through `GetSettings` / `SetSettings`: validated per key, applied live, kept
//! across a restart, with the usage next to the limit.

#[macro_use]
mod service_support;
mod support_daemon;

use std::collections::BTreeMap;

use relay_api::RelayClient;
use relay_api::pb::{SetSettingsResponse, SettingOutcome};
use relay_config::prefs::limits::SPECS;
use relay_engine::daemon::DaemonHandle;
use support_daemon::engine_options;

type Outcomes = BTreeMap<String, (i32, String)>;

fn outcomes(answer: &SetSettingsResponse) -> Outcomes {
	answer
		.results
		.iter()
		.map(|r| (r.key.clone(), (r.outcome, r.reason.clone())))
		.collect()
}

fn outcome(map: &Outcomes, key: &str) -> SettingOutcome {
	SettingOutcome::try_from(map[key].0).unwrap()
}

async fn set(cli: &mut RelayClient, pairs: &[(&str, &str)]) -> Outcomes {
	outcomes(&cli.set_settings(pairs.iter().copied()).await.unwrap())
}

async fn start(home: &std::path::Path) -> (DaemonHandle, RelayClient) {
	let (options, _sim) = engine_options(home, 440.0);
	let daemon = DaemonHandle::start(options).await.unwrap();
	let mut cli = RelayClient::connect(daemon.endpoint()).await.unwrap();
	cli.register(
		"relay-cli",
		"test",
		&["observe", "call-control", "messages"],
	)
	.await
	.unwrap();
	(daemon, cli)
}

fn home() -> tempfile::TempDir {
	tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.unwrap()
}

#[tokio::test]
async fn every_limit_starts_at_its_default_and_is_reported() {
	let home = home();
	let (daemon, mut cli) = start(home.path()).await;
	let all = cli.get_settings().await.unwrap();
	for spec in &SPECS {
		assert_eq!(all[spec.key], spec.default.to_string(), "{}", spec.key);
	}
	let usage = cli.get_limit_usage().await.unwrap();
	assert!(usage.contains_key("limits.avatar_cache_mib"), "{usage:?}");
	assert_eq!(usage["limits.messages_retained_mib"], 0);
	daemon.shutdown().await.unwrap();
}

#[tokio::test]
async fn limits_apply_live_are_rejected_with_a_reason_and_survive_a_restart() {
	let home = home();
	let (daemon, mut cli) = start(home.path()).await;
	let done = set(
		&mut cli,
		&[
			("limits.avatar_cache_mib", "32"),
			("limits.messages_session_mib", "16"),
			("limits.messages_retained_mib", "20"),
			("limits.history_max_entries", "0"),
			("limits.history_max_age_days", "30"),
			("limits.transfer_concurrency", "4"),
			("limits.messages_retained_calls", "0"),
		],
	)
	.await;
	for (key, (state, reason)) in &done {
		assert_eq!(
			SettingOutcome::try_from(*state).unwrap(),
			SettingOutcome::AppliedLive,
			"{key}: {reason}"
		);
	}

	let bad = set(
		&mut cli,
		&[
			("limits.avatar_cache_mib", "4"),
			("limits.history_max_entries", "5001"),
			("limits.history_max_age_days", "0"),
			("limits.transfer_concurrency", "9"),
			("limits.messages_retained_calls", "21"),
			("limits.messages_session_mib", "1025"),
			("limits.messages_retained_mib", "10"),
		],
	)
	.await;
	for key in bad.keys() {
		assert_eq!(outcome(&bad, key), SettingOutcome::Rejected, "{key}");
		assert!(bad[key].1.contains(key), "{key}: {}", bad[key].1);
	}
	assert!(bad["limits.messages_retained_mib"].1.contains("session"));

	let all = cli.get_settings().await.unwrap();
	assert_eq!(all["limits.avatar_cache_mib"], "32");
	assert_eq!(all["limits.history_max_entries"], "0");
	daemon.shutdown().await.unwrap();

	let (daemon, mut cli) = start(home.path()).await;
	let all = cli.get_settings().await.unwrap();
	assert_eq!(all["limits.messages_session_mib"], "16");
	assert_eq!(all["limits.messages_retained_mib"], "20");
	assert_eq!(all["limits.transfer_concurrency"], "4");
	assert_eq!(all["limits.messages_retained_calls"], "0");
	daemon.shutdown().await.unwrap();
}

#[tokio::test]
async fn retained_budget_may_not_drop_below_the_session_budget_in_one_request() {
	let home = home();
	let (daemon, mut cli) = start(home.path()).await;
	// Lowering both in one request is accepted whatever the order inside it.
	let done = set(
		&mut cli,
		&[
			("limits.messages_session_mib", "8"),
			("limits.messages_retained_mib", "8"),
		],
	)
	.await;
	assert_eq!(
		outcome(&done, "limits.messages_session_mib"),
		SettingOutcome::AppliedLive
	);
	assert_eq!(
		outcome(&done, "limits.messages_retained_mib"),
		SettingOutcome::AppliedLive
	);
	let refused = set(&mut cli, &[("limits.messages_session_mib", "64")]).await;
	assert_eq!(
		outcome(&refused, "limits.messages_session_mib"),
		SettingOutcome::Rejected
	);
	daemon.shutdown().await.unwrap();
}
