//! An in-process engine speaking the real API over a Unix socket.
//!
//! It serves a fixed snapshot and records mutations as text, which is enough
//! to prove the CLI's wire behavior end to end.

use std::sync::{Arc, Mutex};

use relay_api::pb::engine_server::Engine;
use relay_api::pb::state_envelope::Body;
use relay_api::pb::*;
use relay_api::{ApiLimits, ClientRegistry, Endpoint, UdsListener, serve_unix};
use tempfile::TempDir;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use super::fixtures;

type Resp<T> = Result<Response<T>, Status>;

#[derive(Clone)]
pub struct FakeEngine {
	registry: Arc<ClientRegistry>,
	pub log: Arc<Mutex<Vec<String>>>,
	pub snapshot: Arc<Mutex<Snapshot>>,
	pub fail_with: Arc<Mutex<Option<Problem>>>,
	/// The capabilities each client asked for when it registered.
	pub registered: Arc<Mutex<Vec<Vec<String>>>>,
	/// Every secret the engine received, to prove they arrive and never leak.
	pub secrets: Arc<Mutex<Vec<String>>>,
}

impl FakeEngine {
	fn op(&self, ctx: Option<&MutationContext>, what: String) -> Resp<Operation> {
		self.log.lock().unwrap().push(what);
		let operation_id = ctx.map(|c| c.operation_id.clone()).unwrap_or_default();
		let problem = self.fail_with.lock().unwrap().clone();
		Ok(Response::new(Operation {
			operation_id,
			state: if problem.is_some() {
				OperationState::Failed as i32
			} else {
				OperationState::Applied as i32
			},
			revision: 11,
			call_id: fixtures::CALL.to_vec(),
			problem,
		}))
	}
}

fn unsupported<T>() -> Resp<T> {
	Err(Status::unimplemented("not in the test engine"))
}

#[tonic::async_trait]
impl Engine for FakeEngine {
	async fn get_info(&self, _: Request<Empty>) -> Resp<EngineInfo> {
		Ok(Response::new(EngineInfo {
			engine_incarnation: self.registry.incarnation().to_vec(),
			application_version: "test-engine".into(),
			api_major: 1,
			api_minor: 0,
			capabilities: vec!["observe".into(), "call-control".into()],
			limits: None,
		}))
	}

	async fn register_client(&self, r: Request<RegisterClientRequest>) -> Resp<ClientSession> {
		self.registered
			.lock()
			.unwrap()
			.push(r.into_inner().requested_capabilities);
		let id = self.registry.register()?;
		Ok(Response::new(ClientSession {
			context: Some(ClientContext {
				client_id: id.to_vec(),
				engine_incarnation: self.registry.incarnation().to_vec(),
			}),
			granted_capabilities: vec!["observe".into()],
			revision: 10,
		}))
	}

	async fn get_snapshot(&self, _: Request<ClientContext>) -> Resp<Snapshot> {
		Ok(Response::new(self.snapshot.lock().unwrap().clone()))
	}

	type WatchStateStream = ReceiverStream<Result<StateEnvelope, Status>>;
	async fn watch_state(&self, _: Request<WatchStateRequest>) -> Resp<Self::WatchStateStream> {
		let snapshot = self.snapshot.lock().unwrap().clone();
		let (tx, rx) = mpsc::channel(4);
		let envelope = StateEnvelope {
			engine_incarnation: self.registry.incarnation().to_vec(),
			revision: snapshot.revision,
			body: Some(Body::Snapshot(snapshot)),
		};
		tokio::spawn(async move {
			let _ = tx.send(Ok(envelope)).await;
			tx.closed().await;
		});
		Ok(Response::new(ReceiverStream::new(rx)))
	}

	type WatchTelemetryStream = ReceiverStream<Result<TelemetrySnapshot, Status>>;
	async fn watch_telemetry(
		&self,
		_: Request<TelemetryRequest>,
	) -> Resp<Self::WatchTelemetryStream> {
		let (tx, rx) = mpsc::channel(8);
		tokio::spawn(async move {
			for (i, db) in [-40.0_f32, -12.5, -30.0].into_iter().enumerate() {
				let sample = TelemetrySnapshot {
					sample_sequence: i as u64,
					microphone_dbfs: Some(db),
					..TelemetrySnapshot::default()
				};
				if tx.send(Ok(sample)).await.is_err() {
					return;
				}
			}
			tx.closed().await;
		});
		Ok(Response::new(ReceiverStream::new(rx)))
	}

	async fn get_operation(&self, _: Request<GetOperationRequest>) -> Resp<Operation> {
		Err(Status::not_found("unknown operation"))
	}

	async fn start_call(&self, r: Request<StartCallRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		let target = match r.target {
			Some(start_call_request::Target::PeerId(id)) => format!("peer={}", hex::encode(id)),
			Some(start_call_request::Target::ManualLocalAddress(a)) => format!("addr={a}"),
			Some(start_call_request::Target::ManualConnectionString(s)) => format!("string={s}"),
			None => "none".into(),
		};
		self.op(r.context.as_ref(), format!("start_call {target}"))
	}
	async fn accept_call(&self, r: Request<CallMutation>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!("accept gen={}", r.controller_generation),
		)
	}
	async fn decline_call(&self, r: Request<CallMutation>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(r.context.as_ref(), "decline".into())
	}
	async fn end_call(&self, r: Request<CallMutation>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!("end call={}", hex::encode(&r.call_id)),
		)
	}
	async fn acquire_controller(
		&self,
		r: Request<AcquireControllerRequest>,
	) -> Resp<ControllerLease> {
		let r = r.into_inner();
		self.log
			.lock()
			.unwrap()
			.push(format!("acquire takeover={}", r.request_explicit_takeover));
		Ok(Response::new(ControllerLease {
			call_id: r.call_id,
			generation: 5,
			expires_in_ms: 10_000,
			..ControllerLease::default()
		}))
	}
	async fn release_controller(&self, _: Request<ReleaseControllerRequest>) -> Resp<Operation> {
		unsupported()
	}
	async fn set_transmit_route(&self, r: Request<SetTransmitRouteRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!(
				"route mode={} consent={}",
				r.mode, r.explicit_microphone_consent
			),
		)
	}
	async fn set_call_controls(&self, r: Request<SetCallControlsRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!(
				"call_controls muted={:?} gain={:?}",
				r.muted, r.receive_gain
			),
		)
	}
	async fn set_global_controls(&self, r: Request<SetGlobalControlsRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!("global muted={:?} mic={:?}", r.muted, r.microphone_gain),
		)
	}
	async fn select_devices(&self, r: Request<SelectDevicesRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!("devices {} {}", r.input_device_id, r.output_device_id),
		)
	}
	async fn set_noise_suppression(
		&self,
		r: Request<SetNoiseSuppressionRequest>,
	) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(r.context.as_ref(), format!("ns {}", r.qualified_backend_id))
	}
	async fn start_mic_test(&self, r: Request<StartMicTestRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(r.context.as_ref(), "mic_test_start".into())
	}
	async fn stop_mic_test(&self, r: Request<MutationContext>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(Some(&r), "mic_test_stop".into())
	}
	async fn preview_tone(&self, r: Request<MutationContext>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(Some(&r), "preview_tone".into())
	}
	async fn set_profile(&self, r: Request<SetProfileRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(r.context.as_ref(), format!("profile {}", r.display_name))
	}
	async fn upload_avatar(&self, r: Request<UploadAvatarRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!(
				"avatar {} bytes {}",
				r.encoded_image.len(),
				r.declared_media_type
			),
		)
	}
	async fn decide_peer_trust(&self, r: Request<DecidePeerTrustRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!("trust {} oob={}", r.requested_trust, r.out_of_band_verified),
		)
	}

	async fn create_channel(&self, r: Request<CreateChannelRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.secrets.lock().unwrap().push(r.password.clone());
		let policy = r.policy.unwrap_or_default();
		self.op(
			r.context.as_ref(),
			format!(
				"create_channel {} listed={} password={} invite_only={}",
				r.name,
				policy.listed,
				!r.password.is_empty(),
				policy.invite_only
			),
		)
	}
	async fn join_channel(&self, r: Request<JoinChannelRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.secrets.lock().unwrap().push(r.password.clone());
		self.secrets.lock().unwrap().push(r.invite.clone());
		let by = if r.channel_id.is_empty() {
			format!("name={}", r.name)
		} else {
			format!("id={}", hex::encode(&r.channel_id))
		};
		self.op(
			r.context.as_ref(),
			format!(
				"join_channel {by} password={} invite={}",
				!r.password.is_empty(),
				!r.invite.is_empty()
			),
		)
	}
	async fn leave_channel(&self, r: Request<LeaveChannelRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!("leave_channel {}", hex::encode(&r.channel_id)),
		)
	}
	async fn list_channels(&self, _: Request<ListChannelsRequest>) -> Resp<ListChannelsResponse> {
		let channels = self.snapshot.lock().unwrap().channels.clone();
		Ok(Response::new(ListChannelsResponse { channels }))
	}
	async fn create_invite(&self, r: Request<CreateInviteRequest>) -> Resp<CreateInviteResponse> {
		let r = r.into_inner();
		self.log.lock().unwrap().push(format!(
			"create_invite {} ttl={} uses={}",
			hex::encode(&r.channel_id),
			r.ttl_seconds,
			r.uses
		));
		Ok(Response::new(CreateInviteResponse {
			invite: "RLYI1-test-token".into(),
			expires_unix: 1_900_000_000,
			operation: None,
		}))
	}
	async fn revoke_channel_member(
		&self,
		r: Request<RevokeChannelMemberRequest>,
	) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!(
				"revoke {} {}",
				hex::encode(&r.channel_id),
				hex::encode(&r.fingerprint)
			),
		)
	}
	async fn set_privacy(&self, r: Request<SetPrivacyRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!(
				"privacy inbound={:?} advert={:?}",
				r.inbound_policy, r.advert_mode
			),
		)
	}
	async fn get_avatar(&self, _: Request<GetAvatarRequest>) -> Resp<AvatarImage> {
		unsupported()
	}
	async fn shutdown(&self, r: Request<MutationContext>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(Some(&r), "shutdown".into())
	}
	async fn get_connection_info(&self, _: Request<ClientContext>) -> Resp<ConnectionInfo> {
		Ok(Response::new(ConnectionInfo {
			local_addresses: vec!["192.168.1.20".into()],
			listen_port: 47_800,
			fingerprint: vec![0x0f; 32],
			connection_string: format!("relay://192.168.1.20:47800#{}", "0f".repeat(32)),
		}))
	}

	type InputControlStream = ReceiverStream<Result<InputControlAck, Status>>;
	async fn input_control(
		&self,
		_: Request<tonic::Streaming<InputControlMessage>>,
	) -> Resp<Self::InputControlStream> {
		unsupported()
	}
}

/// A running engine plus the endpoint to reach it.
pub struct EngineHarness {
	pub dir: TempDir,
	pub socket: std::path::PathBuf,
	pub engine: FakeEngine,
	shutdown: Option<oneshot::Sender<()>>,
}

impl EngineHarness {
	pub async fn start() -> Self {
		let dir = tempfile::Builder::new()
			.prefix("rlycli")
			.tempdir()
			.expect("tempdir");
		let socket = dir.path().join("run").join("engine.sock");
		let limits = ApiLimits::DEFAULT;
		let registry = Arc::new(ClientRegistry::new(limits.max_clients));
		let engine = FakeEngine {
			registry: Arc::clone(&registry),
			log: Arc::default(),
			snapshot: Arc::new(Mutex::new(fixtures::snapshot())),
			fail_with: Arc::default(),
			registered: Arc::default(),
			secrets: Arc::default(),
		};
		let listener = UdsListener::bind(&socket).await.expect("bind");
		let (tx, rx) = oneshot::channel();
		let served = engine.clone();
		tokio::spawn(async move {
			let _ = serve_unix(served, listener, registry, limits, async {
				let _ = rx.await;
			})
			.await;
		});
		Self {
			dir,
			socket,
			engine,
			shutdown: Some(tx),
		}
	}

	pub fn endpoint(&self) -> Endpoint {
		Endpoint::Unix(self.socket.clone())
	}

	pub fn log(&self) -> Vec<String> {
		self.engine.log.lock().unwrap().clone()
	}

	/// Run `relay <args>` in-process against this engine and capture stdout.
	pub async fn run(&self, args: &[&str]) -> Result<String, relay_cli::cli::CliError> {
		use clap::Parser;
		let socket = self.socket.to_str().expect("utf8 path");
		let mut argv = vec!["relay", "--socket", socket];
		argv.extend_from_slice(args);
		let cli = relay_cli::cli::Cli::try_parse_from(argv).expect("valid arguments");
		let mut buffer = Vec::new();
		relay_cli::commands::run(cli, &mut buffer).await?;
		Ok(String::from_utf8(buffer).expect("utf8 output"))
	}
}

impl Drop for EngineHarness {
	fn drop(&mut self) {
		if let Some(tx) = self.shutdown.take() {
			let _ = tx.send(());
		}
	}
}
