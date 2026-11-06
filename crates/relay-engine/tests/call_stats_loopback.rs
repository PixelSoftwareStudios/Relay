#![cfg(unix)]
//! `WatchCallStats` on two whole engines over loopback: a connected call with the microphone
//! open on both sides reports a connected path, packets in both directions and the addresses.

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_media;

use relay_api::RelayClient;
use relay_api::pb::{CallState, CallStats, MediaPathState, TransmitMode, WatchCallStatsRequest};
use service_support::*;
use support_channels::*;

async fn open(cli: &mut RelayClient, call: &[u8]) -> tonic::Streaming<CallStats> {
	let request = WatchCallStatsRequest {
		client: cli.context(),
		call_id: call.to_vec(),
	};
	cli.raw()
		.watch_call_stats(request)
		.await
		.expect("stats stream")
		.into_inner()
}

/// Reads samples until `done` accepts one (at most ten seconds of them).
async fn until(
	stream: &mut tonic::Streaming<CallStats>,
	done: impl Fn(&CallStats) -> bool,
) -> CallStats {
	for _ in 0..10 {
		let sample = stream.message().await.expect("stream").expect("sample");
		if done(&sample) {
			return sample;
		}
	}
	panic!("the statistics never reached the expected state");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_connected_call_reports_path_packets_and_addresses_both_ways() {
	let mut a = side(440.0).await;
	let mut b = side(1_700.0).await;
	pair(&mut a, &mut b, 1).await;
	select_microphone(&mut a.cli, 100).await;
	select_microphone(&mut b.cli, 101).await;
	let call = ring(&mut a, &b, 30).await;
	answer(&mut b.cli, &call, 40).await;
	wait_state(&mut a.cli, &call, CallState::Connected).await;
	wait_state(&mut b.cli, &call, CallState::Connected).await;

	let mut at_a = open(&mut a.cli, &call).await;
	let mut at_b = open(&mut b.cli, &call).await;

	// Muted by choice: connected, nothing sent.
	route(&mut a.cli, 50, &call, TransmitMode::Muted).await;
	route(&mut b.cli, 51, &call, TransmitMode::Muted).await;
	let quiet = until(&mut at_a, |s| {
		s.path_state == MediaPathState::Connected as i32 && !s.sending
	})
	.await;
	assert_eq!(quiet.packets_sent, 0);
	assert!(!quiet.local_addr.is_empty() && !quiet.remote_addr.is_empty());
	assert_eq!(quiet.transport, "lan");

	// Choosing unmuted is the whole consent: packets flow both ways.
	route(&mut a.cli, 52, &call, TransmitMode::Unmuted).await;
	route(&mut b.cli, 53, &call, TransmitMode::Unmuted).await;
	for stream in [&mut at_a, &mut at_b] {
		let live = until(stream, |s| s.sending && s.receiving).await;
		assert_eq!(live.path_state, MediaPathState::Connected as i32);
		assert!(
			live.packets_sent > 0 && live.packets_received > 0,
			"{live:?}"
		);
		assert!(live.codec_bitrate_bps > 0);
		assert!(live.packet_bytes > 0);
		assert!(
			live.mic_level_dbfs.is_some(),
			"the fake microphone plays a tone"
		);
	}
	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}
