//! Whole engines on loopback for the channel tests: pairing, pinning and the
//! small helpers every scenario needs. Needs `service_support` and
//! `support_daemon` in the including test.
#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use relay_api::RelayClient;
use relay_api::pb::{
	AcquireControllerRequest, CallState, ChannelInfo, ChannelPolicy, DecidePeerTrustRequest,
	DeviceDirection, Operation, Peer, SelectDevicesRequest, SetTransmitModeRequest,
	StartCallRequest, TransmitMode, TrustState, start_call_request::Target,
};
use relay_config::identity::{IdentityStore, MemoryStore, StoredIdentity};
use relay_config::vault::MemoryVault;
use relay_engine::channels::Clock;
use relay_engine::daemon::{DaemonHandle, RunOptions};
use relay_net::discovery::DiscoveredPeer;

use crate::service_support::*;
use crate::support_daemon::{Sim, engine_options};

/// An identity store several starts of the same device can share.
struct KeptIdentity(Arc<MemoryStore>);

impl IdentityStore for KeptIdentity {
	fn describe(&self) -> &'static str {
		"kept in memory for the test"
	}
	fn load(&self) -> relay_config::Result<Option<StoredIdentity>> {
		self.0.load()
	}
	fn save(&self, identity: &StoredIdentity) -> relay_config::Result<()> {
		self.0.save(identity)
	}
	fn delete(&self) -> relay_config::Result<()> {
		self.0.delete()
	}
}

pub struct Side {
	pub daemon: DaemonHandle,
	pub sim: Sim,
	pub cli: RelayClient,
	/// An open state watch: the client counts as attached, so its calls live.
	pub watch: Option<relay_api::StateWatch>,
	pub _home: tempfile::TempDir,
	tone: f32,
	identity: Arc<MemoryStore>,
	vault: Arc<MemoryVault>,
	clock: Option<Clock>,
}

pub async fn side(tone: f32) -> Side {
	side_with(tone, None).await
}

pub async fn side_with(tone: f32, clock: Option<Clock>) -> Side {
	side_tuned(tone, clock, true, |_| {}).await
}

/// A side with changed engine options, attached (watching) or not.
pub async fn side_tuned(
	tone: f32,
	clock: Option<Clock>,
	attached: bool,
	tune: impl FnOnce(&mut RunOptions),
) -> Side {
	let home = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.expect("home");
	start(
		home,
		tone,
		Arc::new(MemoryStore::default()),
		Arc::new(MemoryVault::default()),
		clock,
		attached,
		tune,
	)
	.await
}

async fn start(
	home: tempfile::TempDir,
	tone: f32,
	identity: Arc<MemoryStore>,
	vault: Arc<MemoryVault>,
	clock: Option<Clock>,
	attached: bool,
	tune: impl FnOnce(&mut RunOptions),
) -> Side {
	let (mut options, sim) = engine_options(home.path(), tone);
	tune(&mut options);
	options.identity_store = Some(Box::new(KeptIdentity(Arc::clone(&identity))));
	options.channel_vault = Some(vault.clone());
	options.channel_clock = clock.clone();
	let daemon = DaemonHandle::start(options).await.expect("engine starts");
	let mut cli = RelayClient::connect(daemon.endpoint())
		.await
		.expect("connect");
	cli.register("relay-cli", "test", &[])
		.await
		.expect("register");
	// Engines on one host share a hostname; trust is by name, so tell them apart.
	let named = relay_api::pb::SetProfileRequest {
		context: Some(ctx(&cli, 250)),
		display_name: format!("node-{}", tone as u32),
	};
	assert_applied(
		&cli.raw()
			.set_profile(named)
			.await
			.expect("name")
			.into_inner(),
	);
	let watch = if attached {
		Some(
			cli.watch(relay_api::WatchOptions::default())
				.await
				.expect("watch"),
		)
	} else {
		None
	};
	Side {
		daemon,
		sim,
		cli,
		watch,
		_home: home,
		tone,
		identity,
		vault,
		clock,
	}
}

impl Side {
	/// Opens a state watch: this side's client is now attached.
	pub async fn attach(&mut self) {
		let watch = self
			.cli
			.watch(relay_api::WatchOptions::default())
			.await
			.expect("watch");
		self.watch = Some(watch);
	}

	/// Closes the state watch: nobody is attached any more.
	pub fn detach(&mut self) {
		self.watch = None;
	}

	/// Stops the engine and starts it again on the same home, identity and vault:
	/// the same device, on a new port, with its stored channels and trust.
	pub async fn restart(self) -> Side {
		let Side {
			daemon,
			sim,
			cli,
			watch,
			_home,
			tone,
			identity,
			vault,
			clock,
		} = self;
		drop((cli, watch));
		daemon.shutdown().await.expect("stops");
		drop(sim);
		start(_home, tone, identity, vault, clock, true, |_| {}).await
	}

	/// What discovery would hand a neighbour for this device right now.
	pub fn discovered(&self) -> DiscoveredPeer {
		let spec = self.daemon.channels().expect("hub").advert();
		DiscoveredPeer {
			fullname: "test._praxis-relay._tcp.local.".to_owned(),
			instance_id: spec.instance_id,
			display_name: String::new(),
			fingerprint_hint: None,
			capabilities: String::new(),
			addresses: vec![self.daemon.listen_addr().expect("listens")],
			txt: spec.extra_txt.into_iter().collect(),
		}
	}
}

pub fn policy(listed: bool, password: bool, invite_only: bool) -> ChannelPolicy {
	ChannelPolicy {
		listed,
		password,
		invite_only,
	}
}

/// The newest `ms` milliseconds the side's speaker played.
pub fn tail(side: &Side, ms: usize) -> Vec<f32> {
	let heard = side.sim.heard();
	heard[heard.len().saturating_sub(48 * ms)..].to_vec()
}

pub async fn pin_unknown(cli: &mut RelayClient, op: u8) {
	let waiting = eventually!(
		Some(
			cli.snapshot()
				.await
				.expect("snapshot")
				.peers
				.into_iter()
				.filter(|p| p.trust == TrustState::Unknown as i32 && !p.peer_id.is_empty())
				.collect::<Vec<Peer>>()
		)
		.filter(|p| !p.is_empty())
	);
	for (i, peer) in waiting.iter().enumerate() {
		let request = DecidePeerTrustRequest {
			context: Some(ctx(cli, op + i as u8)),
			peer_id: peer.peer_id.clone(),
			expected_public_key_sha256: peer.peer_id.clone(),
			requested_trust: TrustState::TofuUnverified as i32,
			out_of_band_verified: false,
		};
		assert_applied(
			&cli.raw()
				.decide_peer_trust(request)
				.await
				.expect("pin")
				.into_inner(),
		);
	}
}

/// `a` dials `b` by address and both users pin the other: a trusted link.
pub async fn pair(a: &mut Side, b: &mut Side, op: u8) {
	let addr = b.daemon.listen_addr().expect("listens");
	let request = StartCallRequest {
		context: Some(ctx(&a.cli, op)),
		target: Some(Target::ManualLocalAddress(addr.to_string())),
		allow_parallel_same_peer: false,
	};
	// The peer is unknown, so the engine dials and asks to retry.
	let _ = a.cli.raw().start_call(request).await;
	pin_unknown(&mut a.cli, op + 1).await;
	pin_unknown(&mut b.cli, op + 1).await;
	let (a_id, b_id) = (id_of(a), id_of(b));
	eventually!(
		b.cli
			.snapshot()
			.await
			.expect("snapshot")
			.peers
			.iter()
			.find(|p| p.peer_id == a_id && p.reachable && p.trust != TrustState::Unknown as i32)
			.map(|_| ())
	);
	eventually!(
		a.cli
			.snapshot()
			.await
			.expect("snapshot")
			.peers
			.iter()
			.find(|p| p.peer_id == b_id && p.reachable && p.trust != TrustState::Unknown as i32)
			.map(|_| ())
	);
}

pub fn id_of(side: &Side) -> Vec<u8> {
	side.daemon.local_peer().as_bytes().to_vec()
}

pub async fn channel_of(cli: &mut RelayClient, id: &[u8]) -> Option<ChannelInfo> {
	cli.list_channels()
		.await
		.expect("list")
		.into_iter()
		.find(|c| c.channel_id == id)
}

/// Waits until `viewer` lists `peer` as having proven `channel`.
pub async fn wait_shared(viewer: &mut RelayClient, peer: &[u8], channel: &[u8]) {
	eventually!(
		viewer
			.snapshot()
			.await
			.expect("snapshot")
			.peers
			.iter()
			.find(|p| p.peer_id == peer && p.shared_channel_ids.iter().any(|c| c == channel))
			.map(|_| ())
	);
}

pub async fn shared_with(viewer: &mut RelayClient, peer: &[u8]) -> Vec<Vec<u8>> {
	viewer
		.snapshot()
		.await
		.expect("snapshot")
		.peers
		.into_iter()
		.find(|p| p.peer_id == peer)
		.map(|p| p.shared_channel_ids)
		.unwrap_or_default()
}

pub fn assert_denied(op: &Operation) {
	assert_failed(op, "CHANNEL_DENIED");
}

/// Owner side: creates an open channel and returns its id.
pub async fn create_open(cli: &mut RelayClient, name: &str) -> Vec<u8> {
	assert_applied(
		&cli.create_channel(name, policy(false, false, false), "")
			.await
			.expect("create"),
	);
	cli.list_channels()
		.await
		.expect("list")
		.into_iter()
		.find(|c| c.name == name)
		.expect("created")
		.channel_id
}

pub async fn select_microphone(cli: &mut RelayClient, op: u8) {
	let devices = eventually!(
		Some(cli.snapshot().await.expect("snapshot").devices).filter(|d| !d.is_empty())
	);
	let mic = devices
		.iter()
		.find(|d| d.direction == DeviceDirection::Input as i32)
		.expect("microphone")
		.device_id
		.clone();
	let request = SelectDevicesRequest {
		context: Some(ctx(cli, op)),
		input_device_id: mic,
		output_device_id: String::new(),
	};
	assert_applied(
		&cli.raw()
			.select_devices(request)
			.await
			.expect("select")
			.into_inner(),
	);
}

pub async fn route(cli: &mut RelayClient, op: u8, call: &[u8], mode: TransmitMode) {
	let generation = controller_of(&call_in(cli, call).await.expect("call")).generation;
	let request = SetTransmitModeRequest {
		context: Some(ctx(cli, op)),
		call_id: call.to_vec(),
		controller_generation: generation,
		mode: mode as i32,
		authorize_background_open_mic: false,
	};
	assert_applied(
		&cli.raw()
			.set_transmit_mode(request)
			.await
			.expect("route")
			.into_inner(),
	);
}

/// `caller` rings `callee`; returns the call id.
pub async fn ring(caller: &mut Side, callee: &Side, op: u8) -> Vec<u8> {
	let request = StartCallRequest {
		context: Some(ctx(&caller.cli, op)),
		target: Some(Target::PeerId(id_of(callee))),
		allow_parallel_same_peer: false,
	};
	caller
		.cli
		.raw()
		.start_call(request)
		.await
		.expect("call")
		.into_inner()
		.call_id
}

/// Takes control of an incoming call and accepts it.
pub async fn answer(cli: &mut RelayClient, call: &[u8], op: u8) {
	eventually!(
		call_in(cli, call)
			.await
			.filter(|c| c.state == CallState::IncomingRinging as i32)
	);
	let request = AcquireControllerRequest {
		context: Some(ctx(cli, op)),
		call_id: call.to_vec(),
		request_explicit_takeover: false,
	};
	let lease = cli
		.raw()
		.acquire_controller(request)
		.await
		.expect("lease")
		.into_inner();
	let accept = mutation(cli, op + 1, call, lease.generation);
	assert_applied(
		&cli.raw()
			.accept_call(accept)
			.await
			.expect("accept")
			.into_inner(),
	);
}

pub async fn settle(ms: u64) {
	tokio::time::sleep(Duration::from_millis(ms)).await;
}
