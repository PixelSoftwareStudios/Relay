//! Shared fixtures: a real daemon on a real Unix socket with fake adapters.
#![allow(dead_code)]

use std::path::PathBuf;

use relay_api::pb::{
	Call, CallMutation, CallState, ControllerLease, MutationContext, Operation, OperationState,
	SetCallControlsRequest, Snapshot, StartCallRequest, start_call_request::Target,
};
use relay_api::{Endpoint, RelayClient};
use relay_config::Paths;
use relay_config::identity::MemoryStore;
use relay_core::{CallId, DeviceKeyId, PeerEvent, PeerMessage, SessionId, SystemEvent};
use relay_engine::daemon::{DaemonHandle, RunOptions, Transport};
use relay_engine::service::fakes::FakePorts;
use tempfile::TempDir;

pub const PEER: u8 = 7;

pub struct Rig {
	pub daemon: DaemonHandle,
	pub fakes: FakePorts,
	pub socket: PathBuf,
	pub dir: TempDir,
}

impl Rig {
	pub async fn start() -> Self {
		Self::with(|_| {}).await
	}

	pub async fn with(tune: impl FnOnce(&mut RunOptions)) -> Self {
		// Unix socket paths are short; keep the directory under /tmp.
		let dir = tempfile::Builder::new()
			.prefix("rly")
			.tempdir_in("/tmp")
			.expect("tempdir");
		let socket = dir.path().join("e.sock");
		let fakes = FakePorts::default();
		let ports = fakes.ports();
		let mut options = RunOptions::new(Paths::under(dir.path().join("cfg")));
		options.transport = Transport::Unix(Some(socket.clone()));
		options.identity_store = Some(Box::new(MemoryStore::default()));
		options.adapters = Some(Box::new(move |_| ports));
		tune(&mut options);
		let daemon = DaemonHandle::start(options).await.expect("daemon starts");
		Self {
			daemon,
			fakes,
			socket,
			dir,
		}
	}

	pub async fn client(&self, name: &str, caps: &[&str]) -> RelayClient {
		let mut client = RelayClient::connect(&Endpoint::Unix(self.socket.clone()))
			.await
			.expect("connect");
		client.register(name, "test", caps).await.expect("register");
		client
	}

	pub async fn full_client(&self, name: &str) -> RelayClient {
		self.client(name, &["observe", "call-control"]).await
	}
}

pub fn ctx(client: &RelayClient, op: u8) -> MutationContext {
	mctx(client.context(), op)
}

pub fn mctx(client: Option<relay_api::pb::ClientContext>, op: u8) -> MutationContext {
	MutationContext {
		client,
		operation_id: vec![op; 16],
		expected_revision: None,
	}
}

pub fn peer_id() -> DeviceKeyId {
	DeviceKeyId::from_bytes([PEER; 32])
}

pub fn call_id_of(bytes: &[u8]) -> CallId {
	CallId::from_u128(u128::from_be_bytes(bytes.try_into().expect("16 bytes")))
}

pub async fn start_call(client: &mut RelayClient, op: u8) -> Operation {
	let request = StartCallRequest {
		context: Some(ctx(client, op)),
		target: Some(Target::PeerId(vec![PEER; 32])),
		allow_parallel_same_peer: false,
	};
	client
		.raw()
		.start_call(request)
		.await
		.expect("start")
		.into_inner()
}

pub fn mutation(client: &RelayClient, op: u8, call: &[u8], generation: u64) -> CallMutation {
	CallMutation {
		context: Some(ctx(client, op)),
		call_id: call.to_vec(),
		controller_generation: generation,
	}
}

pub fn assert_applied(op: &Operation) {
	assert_eq!(op.state, OperationState::Applied as i32, "{op:?}");
}

pub fn assert_failed(op: &Operation, code: &str) {
	assert_eq!(op.state, OperationState::Failed as i32, "{op:?}");
	assert_eq!(op.problem.as_ref().expect("problem").code, code);
}

/// Polls an `Option`-valued async expression until it yields a value. The budget is a wall
/// clock deadline, generous because a loaded CI runner can starve the engines for seconds;
/// it only matters when the condition is never reached.
macro_rules! eventually {
	($body:expr) => {{
		let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
		let mut found = None;
		while tokio::time::Instant::now() < deadline {
			if let Some(value) = $body {
				found = Some(value);
				break;
			}
			tokio::time::sleep(std::time::Duration::from_millis(10)).await;
		}
		found.expect("condition not reached in time")
	}};
}

/// Waits until the daemon on `endpoint` has finished starting. The socket is served before
/// the identity is loaded, so its file existing does not mean registering is allowed yet.
pub async fn wait_ready(endpoint: &Endpoint) {
	for _ in 0..600 {
		if let Ok(client) = RelayClient::connect(endpoint).await
			&& client.startup().state() == relay_api::pb::StartupState::Ready
		{
			return;
		}
		tokio::time::sleep(std::time::Duration::from_millis(10)).await;
	}
	panic!("the daemon did not finish starting in time");
}

pub async fn call_in(client: &mut RelayClient, call: &[u8]) -> Option<Call> {
	let snapshot: Snapshot = client.snapshot().await.expect("snapshot");
	snapshot.calls.into_iter().find(|c| c.call_id == call)
}

pub async fn wait_state(client: &mut RelayClient, call: &[u8], state: CallState) -> Call {
	eventually!(
		call_in(client, call)
			.await
			.filter(|c| c.state == state as i32)
	)
}

/// Drives a started call through link-up, accept, media and path to CONNECTED.
pub async fn connect_call(rig: &Rig, client: &mut RelayClient, call: &[u8]) -> (CallId, SessionId) {
	let control = rig.daemon.control();
	control
		.peer(PeerEvent::LinkUp(peer_id()))
		.await
		.expect("link");
	let session = eventually!(
		rig.fakes
			.peers
			.sent()
			.into_iter()
			.find_map(|(_, m)| match m {
				PeerMessage::Invite { session, .. } => Some(session),
				_ => None,
			})
	);
	let id = call_id_of(call);
	control
		.peer(PeerEvent::Accepted { call: id, session })
		.await
		.expect("accept");
	wait_state(client, call, CallState::Negotiating).await;
	control
		.system(SystemEvent::MediaPrepared { call: id, session })
		.await
		.expect("prepared");
	control
		.peer(PeerEvent::PathValidated { call: id, session })
		.await
		.expect("path");
	wait_state(client, call, CallState::Connected).await;
	(id, session)
}

pub fn controls(
	client: &RelayClient,
	op: u8,
	call: &[u8],
	generation: u64,
	muted: bool,
	gain: Option<f32>,
) -> SetCallControlsRequest {
	SetCallControlsRequest {
		context: Some(ctx(client, op)),
		call_id: call.to_vec(),
		controller_generation: generation,
		muted: Some(muted),
		deafened: None,
		receive_gain: gain,
	}
}

pub fn controller_of(call: &Call) -> ControllerLease {
	call.controller.clone().expect("controller")
}

/// The gRPC code carried by a client error.
pub fn code_of(error: &relay_api::ApiError) -> tonic::Code {
	match error {
		relay_api::ApiError::Status(status) => status.code(),
		other => panic!("not a status error: {other}"),
	}
}
