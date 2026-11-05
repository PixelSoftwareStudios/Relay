#![cfg(unix)]
//! Ring, ringback and blips driven from call events on a real daemon with fake
//! adapters: the media fake records every tone command.

#[macro_use]
mod service_support;

use relay_api::pb::{CallState, SetGlobalControlsRequest};
use relay_api::{RelayClient, WatchOptions};
use relay_audio::tones::Sound;
use relay_core::{CallId, PeerEvent, SessionId};
use relay_engine::service::fakes::MediaCall;
use relay_engine::service::{SettingsStore, ToneCommand};
use service_support::*;

fn tones(rig: &Rig) -> Vec<ToneCommand> {
	rig.fakes
		.media
		.calls()
		.into_iter()
		.filter_map(|c| match c {
			MediaCall::Tone(t) => Some(t),
			_ => None,
		})
		.collect()
}

fn ring(volume: f32) -> ToneCommand {
	ToneCommand::Start {
		sound: Sound::Ring,
		looping: true,
		volume,
	}
}

async fn settle() {
	tokio::time::sleep(std::time::Duration::from_millis(300)).await;
}

/// Opens a state watch so the client counts as attached.
async fn attach(rig: &Rig) -> (RelayClient, relay_api::StateWatch) {
	let mut cli = rig.full_client("ui").await;
	let mut watch = cli.watch(WatchOptions::default()).await.expect("watch");
	let _ = watch.next().await;
	(cli, watch)
}

async fn invite(rig: &Rig, call: u128) -> (CallId, SessionId) {
	let control = rig.daemon.control();
	let (call, session) = (CallId::from_u128(call), SessionId::from_u128(call + 100));
	control
		.peer(PeerEvent::LinkUp(peer_id()))
		.await
		.expect("link");
	control
		.peer(PeerEvent::Invite {
			call,
			session,
			peer: peer_id(),
			allow_parallel_same_peer: false,
		})
		.await
		.expect("invite");
	(call, session)
}

async fn controls(cli: &mut RelayClient, op: u8, edit: impl FnOnce(&mut SetGlobalControlsRequest)) {
	let mut request = SetGlobalControlsRequest {
		context: Some(ctx(cli, op)),
		..SetGlobalControlsRequest::default()
	};
	edit(&mut request);
	let applied = cli
		.raw()
		.set_global_controls(request)
		.await
		.expect("controls")
		.into_inner();
	assert_applied(&applied);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unattended_daemon_declines_the_invite_and_stays_silent() {
	let rig = Rig::start().await;
	invite(&rig, 1).await;
	settle().await;
	assert!(tones(&rig).is_empty(), "{:?}", tones(&rig));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_ring_starts_with_a_client_attached_and_stops_when_the_caller_cancels() {
	let rig = Rig::start().await;
	let (mut cli, _watch) = attach(&rig).await;
	let (call, session) = invite(&rig, 2).await;
	let id = call.to_bytes().to_vec();
	wait_state(&mut cli, &id, CallState::IncomingRinging).await;
	eventually!((tones(&rig) == [ring(0.7)]).then_some(()));
	rig.daemon
		.control()
		.peer(PeerEvent::Ended { call, session })
		.await
		.expect("ended");
	eventually!((tones(&rig) == [ring(0.7), ToneCommand::Stop]).then_some(()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deafen_silences_the_ring_and_settings_apply_live() {
	let rig = Rig::start().await;
	let (mut cli, _watch) = attach(&rig).await;
	invite(&rig, 3).await;
	eventually!((tones(&rig) == [ring(0.7)]).then_some(()));

	controls(&mut cli, 1, |r| r.ringtone_volume = Some(0.3)).await;
	eventually!(
		tones(&rig)
			.contains(&ToneCommand::Volume(0.3))
			.then_some(())
	);

	controls(&mut cli, 2, |r| r.deafened = Some(true)).await;
	eventually!(
		tones(&rig)
			.ends_with(&[ToneCommand::Muted(true), ToneCommand::Stop])
			.then_some(())
	);
	controls(&mut cli, 3, |r| r.deafened = Some(false)).await;
	eventually!(
		tones(&rig)
			.ends_with(&[ToneCommand::Muted(false), ring(0.3)])
			.then_some(())
	);

	controls(&mut cli, 4, |r| r.ringtone_enabled = Some(false)).await;
	eventually!(tones(&rig).ends_with(&[ToneCommand::Stop]).then_some(()));
	assert!(!rig.fakes.settings.load().ring.ringtone_enabled);
	assert!((rig.fakes.settings.load().ring.ringtone_volume - 0.3).abs() < 1e-6);
	let bad = SetGlobalControlsRequest {
		context: Some(ctx(&cli, 5)),
		ringtone_volume: Some(1.5),
		..SetGlobalControlsRequest::default()
	};
	assert!(cli.raw().set_global_controls(bad).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ringback_plays_while_an_outgoing_call_rings_and_stops_on_accept() {
	let rig = Rig::start().await;
	let (mut cli, _watch) = attach(&rig).await;
	let op = start_call(&mut cli, 1).await;
	assert_applied(&op);
	let control = rig.daemon.control();
	control
		.peer(PeerEvent::LinkUp(peer_id()))
		.await
		.expect("link");
	let back = ToneCommand::Start {
		sound: Sound::Ringback,
		looping: true,
		volume: 0.7,
	};
	eventually!((tones(&rig) == [back]).then_some(()));
	let session = eventually!(
		rig.fakes
			.peers
			.sent()
			.into_iter()
			.find_map(|(_, m)| match m {
				relay_core::PeerMessage::Invite { call, session, .. } => Some((call, session)),
				_ => None,
			})
	);
	control
		.peer(PeerEvent::Accepted {
			call: session.0,
			session: session.1,
		})
		.await
		.expect("accepted");
	eventually!(tones(&rig).contains(&ToneCommand::Stop).then_some(()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_test_ring_plays_for_two_seconds_then_stops() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("ui").await;
	let context = ctx(&cli, 1);
	let operation = cli
		.raw()
		.preview_tone(context)
		.await
		.expect("preview")
		.into_inner();
	assert_applied(&operation);
	eventually!((tones(&rig) == [ring(0.7)]).then_some(()));
	tokio::time::sleep(std::time::Duration::from_millis(1_500)).await;
	assert_eq!(tones(&rig), [ring(0.7)], "still ringing before two seconds");
	eventually!((tones(&rig) == [ring(0.7), ToneCommand::Stop]).then_some(()));
}
