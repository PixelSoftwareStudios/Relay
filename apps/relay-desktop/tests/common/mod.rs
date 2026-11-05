//! Fake engine for host tests: scripted connections, recorded commands.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use relay_api::{WatchItem, pb};
use relay_desktop::controller::{Controller, SessionSlot};
use relay_desktop::engine::{
	Command, Connection, Engine, EngineError, EventSource, Operations, Outcome, TelemetrySource,
};
use relay_desktop::hub::Hub;
use tokio::sync::mpsc;

pub const ME: [u8; 16] = [7; 16];
pub const CALL: [u8; 16] = [1; 16];
pub const PEER: [u8; 32] = [2; 32];

pub type WatchMsg = Result<Option<WatchItem>, EngineError>;

#[derive(Default)]
pub struct FakeOps {
	pub commands: Mutex<Vec<Command>>,
	pub fail_with: Mutex<Option<String>>,
	pub lease_generation: u64,
	pub png: Vec<u8>,
}

impl FakeOps {
	pub fn recorded(&self) -> Vec<Command> {
		self.commands.lock().unwrap().clone()
	}
}

#[async_trait]
impl Operations for FakeOps {
	async fn connection_info(&self) -> Result<relay_desktop::model::ConnectionView, EngineError> {
		Ok(relay_desktop::model::ConnectionView {
			addresses: vec!["10.0.0.2".into()],
			port: 7000,
			fingerprint: "ab".repeat(32),
			connection_string: format!("relay://10.0.0.2:7000#{}", "ab".repeat(32)),
		})
	}

	async fn avatar(&self, _sha256: Vec<u8>) -> Result<Vec<u8>, EngineError> {
		Ok(self.png.clone())
	}

	async fn run(&self, command: Command) -> Result<Outcome, EngineError> {
		self.commands.lock().unwrap().push(command.clone());
		if let Some(message) = self.fail_with.lock().unwrap().clone() {
			return Err(EngineError::Rejected {
				code: "FAKE".into(),
				message,
			});
		}
		let generation =
			matches!(command, Command::AcquireController { .. }).then_some(self.lease_generation);
		let invite = matches!(command, Command::CreateInvite { .. })
			.then(|| relay_client::Secret::new("RLYI1-test-token"));
		Ok(Outcome { generation, invite })
	}
}

pub struct ChannelWatch(pub mpsc::UnboundedReceiver<WatchMsg>);

#[async_trait]
impl EventSource for ChannelWatch {
	async fn next(&mut self) -> Result<Option<WatchItem>, EngineError> {
		self.0.recv().await.unwrap_or(Ok(None))
	}
}

pub struct ChannelTelemetry(pub mpsc::UnboundedReceiver<pb::TelemetrySnapshot>);

#[async_trait]
impl TelemetrySource for ChannelTelemetry {
	async fn next(&mut self) -> Result<Option<pb::TelemetrySnapshot>, EngineError> {
		Ok(self.0.recv().await)
	}
}

/// Handles to drive one scripted connection from a test.
pub struct Feed {
	pub watch: mpsc::UnboundedSender<WatchMsg>,
	pub telemetry: mpsc::UnboundedSender<pb::TelemetrySnapshot>,
	pub ops: Arc<FakeOps>,
}

pub fn connection(snapshot: pb::Snapshot, lease_generation: u64) -> (Connection, Feed) {
	let (wtx, wrx) = mpsc::unbounded_channel();
	let (ttx, trx) = mpsc::unbounded_channel();
	let ops = Arc::new(FakeOps {
		lease_generation,
		..FakeOps::default()
	});
	let conn = Connection {
		info: pb::EngineInfo {
			application_version: "9.9".into(),
			api_major: 1,
			..Default::default()
		},
		client_id: ME.to_vec(),
		snapshot,
		watch: Box::new(ChannelWatch(wrx)),
		telemetry: Some(Box::new(ChannelTelemetry(trx))),
		ops: ops.clone(),
	};
	(
		conn,
		Feed {
			watch: wtx,
			telemetry: ttx,
			ops,
		},
	)
}

/// Hands out queued connect results in order, then reports the daemon down.
#[derive(Default)]
pub struct ScriptedEngine {
	queue: Mutex<VecDeque<Result<Connection, EngineError>>>,
}

impl ScriptedEngine {
	pub fn push(&self, item: Result<Connection, EngineError>) {
		self.queue.lock().unwrap().push_back(item);
	}
}

#[async_trait]
impl Engine for ScriptedEngine {
	async fn connect(&self) -> Result<Connection, EngineError> {
		self.queue
			.lock()
			.unwrap()
			.pop_front()
			.unwrap_or_else(|| Err(EngineError::Unavailable("no daemon".into())))
	}
}

pub fn pb_peer(name: &str) -> pb::Peer {
	pb::Peer {
		peer_id: PEER.to_vec(),
		display_name: name.into(),
		reachable: true,
		trust: pb::TrustState::Verified as i32,
		..Default::default()
	}
}

pub fn pb_call(state: pb::CallState, controller: Option<(&[u8], u64)>) -> pb::Call {
	pb::Call {
		call_id: CALL.to_vec(),
		peer_id: PEER.to_vec(),
		state: state as i32,
		controller: controller.map(|(id, generation)| pb::ControllerLease {
			controller_client_id: id.to_vec(),
			generation,
			..Default::default()
		}),
		receive_gain: 1.0,
		..Default::default()
	}
}

pub fn pb_device(id: &str, direction: pb::DeviceDirection, available: bool) -> pb::Device {
	pb::Device {
		device_id: id.into(),
		name: id.into(),
		direction: direction as i32,
		available,
		..Default::default()
	}
}

pub fn snapshot(revision: u64, calls: Vec<pb::Call>) -> pb::Snapshot {
	pb::Snapshot {
		engine_incarnation: vec![9; 16],
		revision,
		peers: vec![pb_peer("Ada")],
		calls,
		devices: vec![
			pb_device("mic", pb::DeviceDirection::Input, true),
			pb_device("gone", pb::DeviceDirection::Output, false),
			pb_device("spk", pb::DeviceDirection::Output, true),
		],
		..Default::default()
	}
}

pub fn event(revision: u64, event: pb::state_event::Event) -> WatchItem {
	WatchItem::Event {
		revision,
		event: pb::StateEvent { event: Some(event) },
	}
}

/// A controller wired to a hub already holding `snapshot` and a fake session.
pub fn controller_with(
	snapshot: pb::Snapshot,
	lease_generation: u64,
) -> (Controller, Arc<Hub>, Arc<FakeOps>) {
	let hub = Arc::new(Hub::default());
	hub.update(|s| {
		s.client_id = ME.to_vec();
		s.load_snapshot(&snapshot);
	});
	let slot = Arc::new(SessionSlot::default());
	let (conn, feed) = connection(snapshot, lease_generation);
	slot.set(conn.ops);
	(Controller::new(hub.clone(), slot), hub, feed.ops)
}

pub const CHAN: [u8; 16] = [3; 16];

pub fn pb_channel(role: &str) -> pb::ChannelInfo {
	pb::ChannelInfo {
		channel_id: CHAN.to_vec(),
		name: "dorm".into(),
		policy: Some(pb::ChannelPolicy {
			listed: true,
			password: true,
			invite_only: false,
		}),
		role: role.into(),
		shared_peers: 1,
		cert_expires_unix: 0,
	}
}

/// A snapshot where the one peer has proven membership of `CHAN`.
pub fn snapshot_in_channel(role: &str) -> pb::Snapshot {
	let mut s = snapshot(1, vec![]);
	s.channels = vec![pb_channel(role)];
	s.peers[0].shared_channel_ids = vec![CHAN.to_vec()];
	s
}
