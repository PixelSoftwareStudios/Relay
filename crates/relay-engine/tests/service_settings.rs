#![cfg(unix)]
//! `GetSettings` / `SetSettings`: any preference by key, live where the daemon can.

#[macro_use]
mod service_support;
mod support_daemon;

use std::collections::BTreeMap;

use relay_api::RelayClient;
use relay_api::pb::{InboundPolicy, SetSettingsResponse, SettingOutcome};
use relay_engine::daemon::DaemonHandle;
use service_support::*;
use support_daemon::engine_options;
use tonic::Code;

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
	let answer = cli
		.set_settings(pairs.iter().copied())
		.await
		.expect("set_settings");
	assert_eq!(
		answer.operation.as_ref().expect("operation").state,
		relay_api::pb::OperationState::Applied as i32
	);
	outcomes(&answer)
}

#[tokio::test]
async fn the_full_map_is_readable_and_importable_again() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let all = cli.get_settings().await.expect("get");
	for key in [
		"hotkeys.push_to_talk",
		"ptt.release_delay_ms",
		"audio.ringtone_volume",
		"messages.enabled",
		"network.transfer_port",
		"identity.key_storage",
	] {
		assert!(all.contains_key(key), "{key} missing from {all:?}");
	}
	assert!(!all.contains_key("config_version"));
	// An export imports again, sensitive keys included, because they are unchanged.
	let again = set(
		&mut cli,
		&all.iter()
			.map(|(k, v)| (k.as_str(), v.as_str()))
			.collect::<Vec<_>>(),
	)
	.await;
	let rejected: Vec<_> = again
		.iter()
		.filter(|(_, (o, _))| *o == SettingOutcome::Rejected as i32)
		.collect();
	assert!(rejected.is_empty(), "{rejected:?}");
	assert_eq!(cli.get_settings().await.unwrap(), all);

	// Reading needs observe only.
	let mut reader = rig.client("reader", &["observe"]).await;
	assert_eq!(reader.get_settings().await.unwrap(), all);
}

#[tokio::test]
async fn live_keys_take_effect_and_reach_watchers() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let done = set(
		&mut cli,
		&[
			("profile.display_name", "Kitchen"),
			("ptt.release_delay_ms", "350"),
			("audio.playback_gain", "0.5"),
			("audio.ringtone_volume", "0.25"),
			("audio.ringback_enabled", "false"),
			("privacy.inbound_policy", "shared_channel_only"),
			("clients.autostart_daemon", "false"),
		],
	)
	.await;
	for (key, (o, why)) in &done {
		assert_eq!(*o, SettingOutcome::AppliedLive as i32, "{key}: {why}");
	}
	let settings = cli.snapshot().await.unwrap().settings.unwrap();
	assert_eq!(settings.display_name, "Kitchen");
	assert_eq!(settings.ptt_release_delay_ms, 350);
	assert!((settings.playback_gain - 0.5).abs() < 1e-3);
	assert!((settings.ringtone_volume - 0.25).abs() < 1e-6);
	assert!(!settings.ringback_enabled && !settings.autostart_daemon);
	assert_eq!(
		settings.inbound_policy,
		InboundPolicy::SharedChannelOnly as i32
	);
	let advertised = rig.fakes.peers.profiles();
	assert_eq!(advertised.last().expect("profile").display_name, "Kitchen");
	assert_eq!(
		cli.get_settings().await.unwrap()["profile.display_name"],
		"Kitchen"
	);
}

#[tokio::test]
async fn keys_read_at_start_are_saved_and_say_so() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let done = set(
		&mut cli,
		&[
			("hotkeys.push_to_talk", "Ctrl+F9"),
			("network.transfer_port", "40123"),
			("audio.noise_backend", "off"),
			("audio.microphone_gain", "1.5"),
		],
	)
	.await;
	for key in [
		"hotkeys.push_to_talk",
		"network.transfer_port",
		"audio.noise_backend",
	] {
		assert_eq!(
			outcome(&done, key),
			SettingOutcome::SavedNeedsRestart,
			"{key}"
		);
		assert!(!done[key].1.is_empty(), "a reason is given");
	}
	// No microphone path in this build: saved, not applied.
	assert_eq!(
		outcome(&done, "audio.microphone_gain"),
		SettingOutcome::SavedNeedsRestart
	);
	let all = cli.get_settings().await.unwrap();
	assert_eq!(all["hotkeys.push_to_talk"], "Ctrl+F9");
	assert_eq!(all["network.transfer_port"], "40123");
}

#[tokio::test]
async fn bad_keys_and_values_are_rejected_one_by_one() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	let done = set(
		&mut cli,
		&[
			("ptt.release_delay_ms", "9999"),
			("no.such_key", "1"),
			("audio.ringtone_volume", "loud"),
			("config_version", "5"),
			("ptt.release_delay_ms ", "1"),
			("hotkeys.push_to_talk", "F8"),
		],
	)
	.await;
	for key in [
		"ptt.release_delay_ms",
		"no.such_key",
		"audio.ringtone_volume",
		"ptt.release_delay_ms ",
	] {
		assert_eq!(outcome(&done, key), SettingOutcome::Rejected, "{key}");
		assert!(!done[key].1.is_empty());
	}
	assert_eq!(
		outcome(&done, "hotkeys.push_to_talk"),
		SettingOutcome::SavedNeedsRestart
	);
	// A rejected key changed nothing.
	let all = cli.get_settings().await.unwrap();
	assert_eq!(all["ptt.release_delay_ms"], "200");
	assert_eq!(all["hotkeys.push_to_talk"], "F8");
}

#[tokio::test]
async fn security_sensitive_keys_are_refused_and_nothing_is_applied() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("relay-cli").await;
	for key in [
		"identity.key_storage",
		"diagnostics.record_raw_audio",
		"network.allow_wan",
		"network.allow_overlay_manual",
	] {
		let value = if key == "identity.key_storage" {
			"insecure_file"
		} else {
			"true"
		};
		let refused = cli
			.set_settings([("ptt.release_delay_ms", "10"), (key, value)])
			.await
			.expect_err(key);
		assert!(
			format!("{refused:?}").contains("PermissionDenied"),
			"{key}: {refused:?}"
		);
		assert!(
			format!("{refused:?}").contains("SETTING_RESTRICTED"),
			"{key}: {refused:?}"
		);
	}
	assert_eq!(
		cli.get_settings().await.unwrap()["ptt.release_delay_ms"],
		"200"
	);
	assert_eq!(
		cli.get_settings().await.unwrap()["identity.key_storage"],
		"os_credential_store"
	);
}

#[tokio::test]
async fn the_request_needs_call_control_and_a_sane_size() {
	let rig = Rig::start().await;
	let mut watcher = rig.client("watcher", &["observe"]).await;
	let denied = watcher
		.set_settings([("ptt.release_delay_ms", "1")])
		.await
		.unwrap_err();
	assert!(format!("{denied:?}").contains("PermissionDenied"));

	let mut cli = rig.full_client("relay-cli").await;
	let none: [(&str, &str); 0] = [];
	let empty = cli.set_settings(none).await.unwrap_err();
	assert!(format!("{empty:?}").contains("InvalidArgument"));
	let many: Vec<(String, String)> = (0..65).map(|i| (format!("k{i}"), "1".to_owned())).collect();
	let crowded = cli.set_settings(many).await.unwrap_err();
	assert!(format!("{crowded:?}").contains("InvalidArgument"));
	let long = cli
		.set_settings([("profile.display_name", "x".repeat(2000))])
		.await
		.unwrap_err();
	assert!(format!("{long:?}").contains("InvalidArgument"));
	let _ = Code::Ok;
}

#[tokio::test]
async fn messaging_keys_apply_live_and_everything_persists_across_a_restart() {
	let home = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.unwrap();
	let start = || async {
		let (options, sim) = engine_options(home.path(), 440.0);
		let daemon = DaemonHandle::start(options).await.unwrap();
		let mut cli = RelayClient::connect(daemon.endpoint()).await.unwrap();
		cli.register(
			"relay-cli",
			"test",
			&["observe", "call-control", "messages"],
		)
		.await
		.unwrap();
		(daemon, sim, cli)
	};
	let (daemon, _sim, mut cli) = start().await;
	let done = set(
		&mut cli,
		&[
			("messages.max_offer_size_mib", "64"),
			("messages.transfer_rate_limit_mbps", "4"),
			("audio.microphone_gain", "1.5"),
			("hotkeys.push_to_talk", "Ctrl+Shift+T"),
			("profile.display_name", "Hall"),
		],
	)
	.await;
	for key in [
		"messages.max_offer_size_mib",
		"messages.transfer_rate_limit_mbps",
		"profile.display_name",
	] {
		assert_eq!(
			outcome(&done, key),
			SettingOutcome::AppliedLive,
			"{key}: {:?}",
			done[key]
		);
	}
	assert_eq!(
		outcome(&done, "hotkeys.push_to_talk"),
		SettingOutcome::SavedNeedsRestart
	);
	let context = cli.context().unwrap();
	let messaging = cli.raw().list_sessions(context).await.unwrap().into_inner();
	let live = messaging.settings.expect("settings");
	assert_eq!(
		(live.max_offer_size_mib, live.transfer_rate_limit_mbps),
		(64, 4)
	);
	daemon.shutdown().await.unwrap();

	let (daemon, _sim, mut cli) = start().await;
	let all = cli.get_settings().await.unwrap();
	assert_eq!(all["hotkeys.push_to_talk"], "Ctrl+Shift+T");
	assert_eq!(all["messages.max_offer_size_mib"], "64");
	assert_eq!(all["profile.display_name"], "Hall");
	let name = cli.snapshot().await.unwrap().settings.unwrap().display_name;
	assert_eq!(name, "Hall");
	daemon.shutdown().await.unwrap();
}
