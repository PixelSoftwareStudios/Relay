#![cfg(unix)]
//! A client sets and resets this device's ringtone through the API, on a real daemon with fake
//! adapters: the media fake records the ring it was given and the settings show the name.

#[macro_use]
mod service_support;

use relay_api::pb::SetRingtoneRequest;
use relay_api::{RelayClient, WatchOptions};
use relay_engine::service::SettingsStore;
use relay_engine::service::fakes::MediaCall;
use service_support::*;

fn fixture(name: &str) -> Vec<u8> {
	std::fs::read(format!(
		"{}/../relay-audio/tests/fixtures/ringtone/{name}",
		env!("CARGO_MANIFEST_DIR")
	))
	.expect("fixture")
}

fn rings(rig: &Rig) -> Vec<Option<usize>> {
	rig.fakes
		.media
		.calls()
		.into_iter()
		.filter_map(|c| match c {
			MediaCall::Ringtone(len) => Some(len),
			_ => None,
		})
		.collect()
}

async fn set(
	cli: &mut RelayClient,
	op: u8,
	name: &str,
	data: Vec<u8>,
	reset: bool,
) -> relay_api::pb::Operation {
	let request = SetRingtoneRequest {
		context: Some(ctx(cli, op)),
		file_name: name.to_owned(),
		data,
		reset,
	};
	cli.raw()
		.set_ringtone(request)
		.await
		.expect("set_ringtone")
		.into_inner()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_chosen_file_becomes_the_ring_and_reset_brings_the_chime_back() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("ui").await;
	let mut watch = cli.watch(WatchOptions::default()).await.expect("watch");
	let first = watch.next().await.expect("snapshot");
	drop(first);

	let applied = set(
		&mut cli,
		1,
		"/home/me/Music/chime.mp3",
		fixture("tone.mp3"),
		false,
	)
	.await;
	assert_applied(&applied);
	let lengths = rings(&rig);
	assert_eq!(lengths.len(), 1);
	let samples = lengths[0].expect("a custom ring");
	assert!(samples > 48_000 + 48_000, "audio plus its pause: {samples}");
	assert_eq!(
		rig.fakes.settings.load().ringtone_name,
		"chime.mp3",
		"a path is cut to its name"
	);

	let applied = set(&mut cli, 2, "", Vec::new(), true).await;
	assert_applied(&applied);
	assert_eq!(rings(&rig), [Some(samples), None]);
	assert_eq!(rig.fakes.settings.load().ringtone_name, "");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bad_file_is_refused_with_a_code_and_changes_nothing() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("ui").await;

	let refused = set(
		&mut cli,
		1,
		"notes.txt",
		b"not audio, just words".to_vec(),
		false,
	)
	.await;
	assert_failed(&refused, "RINGTONE_UNSUPPORTED");
	let refused = set(&mut cli, 2, "x.wav", vec![0; 10], false).await;
	assert!(
		refused
			.problem
			.expect("problem")
			.code
			.starts_with("RINGTONE_")
	);
	assert!(rings(&rig).is_empty());
	assert_eq!(rig.fakes.settings.load().ringtone_name, "");

	let context = ctx(&cli, 3);
	let empty = cli
		.raw()
		.set_ringtone(SetRingtoneRequest {
			context: Some(context),
			file_name: "a.mp3".into(),
			..SetRingtoneRequest::default()
		})
		.await;
	assert!(empty.is_err(), "an empty file is an invalid argument");
}
