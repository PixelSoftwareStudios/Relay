//! Two complete engines in one process, paired (trusted both ways) through the local API.
#![allow(dead_code)]

use relay_api::RelayClient;
use relay_api::pb::{
	DecidePeerTrustRequest, Peer, StartCallRequest, TrustState, start_call_request::Target,
};
use relay_engine::daemon::DaemonHandle;
use tonic::Code;

use crate::service_support::{assert_applied, ctx};
use crate::support_daemon::{Sim, engine_options};

pub struct Side {
	pub daemon: DaemonHandle,
	pub sim: Sim,
	pub cli: RelayClient,
	/// An open watch keeps the client attached, so the engine advertises and links.
	pub _watch: relay_api::StateWatch,
	pub home: tempfile::TempDir,
}

pub async fn side(tone: f32) -> Side {
	let home = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.expect("home");
	let (options, sim) = engine_options(home.path(), tone);
	let daemon = DaemonHandle::start(options).await.expect("engine starts");
	let mut cli = RelayClient::connect(daemon.endpoint())
		.await
		.expect("connect");
	cli.register("relay-cli", "test", &[])
		.await
		.expect("register");
	let watch = cli
		.watch(relay_api::WatchOptions::default())
		.await
		.expect("watch");
	Side {
		daemon,
		sim,
		cli,
		_watch: watch,
		home,
	}
}

pub async fn peer_where(cli: &mut RelayClient, keep: impl Fn(&Peer) -> bool) -> Option<Peer> {
	cli.snapshot()
		.await
		.expect("snapshot")
		.peers
		.into_iter()
		.find(|p| keep(p))
}

pub async fn trust(cli: &mut RelayClient, op: u8, peer: &Peer) {
	let request = DecidePeerTrustRequest {
		context: Some(ctx(cli, op)),
		peer_id: peer.peer_id.clone(),
		expected_public_key_sha256: peer.peer_id.clone(),
		requested_trust: TrustState::TofuUnverified as i32,
		out_of_band_verified: false,
	};
	let done = cli
		.raw()
		.decide_peer_trust(request)
		.await
		.expect("trust")
		.into_inner();
	assert_applied(&done);
}

/// Dials B from A, lets both users trust the other, and returns how each side sees the other.
pub async fn paired() -> (Side, Side, Peer, Peer) {
	let mut a = side(440.0).await;
	let mut b = side(1_700.0).await;
	let b_addr = b.daemon.listen_addr().expect("b listens");
	let request = StartCallRequest {
		context: Some(ctx(&a.cli, 1)),
		target: Some(Target::ManualLocalAddress(b_addr.to_string())),
		allow_parallel_same_peer: false,
	};
	let dialling = a.cli.raw().start_call(request).await.expect_err("dialling");
	assert_eq!(dialling.code(), Code::FailedPrecondition);
	let unknown = |p: &Peer| p.trust == TrustState::Unknown as i32;
	let b_on_a = eventually!(peer_where(&mut a.cli, unknown).await);
	let a_on_b = eventually!(peer_where(&mut b.cli, unknown).await);
	trust(&mut a.cli, 2, &b_on_a).await;
	trust(&mut b.cli, 3, &a_on_b).await;
	let trusted = |p: &Peer| p.reachable && p.trust == TrustState::TofuUnverified as i32;
	let b_on_a = eventually!(peer_where(&mut a.cli, trusted).await);
	let a_on_b = eventually!(peer_where(&mut b.cli, trusted).await);
	(a, b, b_on_a, a_on_b)
}
