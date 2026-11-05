//! A minimal in-process engine over a Unix socket that records mutations.
#![cfg(unix)]

use std::sync::{Arc, Mutex};

use relay_api::pb::engine_server::Engine;
use relay_api::pb::*;
use relay_api::{ApiLimits, ClientRegistry, Endpoint, UdsListener, serve_unix};
use tempfile::TempDir;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

type Resp<T> = Result<Response<T>, Status>;

#[derive(Clone)]
pub struct FakeEngine {
	registry: Arc<ClientRegistry>,
	pub log: Arc<Mutex<Vec<String>>>,
}

impl FakeEngine {
	fn op(&self, ctx: Option<&MutationContext>, what: String) -> Resp<Operation> {
		self.log.lock().unwrap().push(what);
		Ok(Response::new(Operation {
			operation_id: ctx.map(|c| c.operation_id.clone()).unwrap_or_default(),
			state: OperationState::Applied as i32,
			..Operation::default()
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
	async fn register_client(&self, _: Request<RegisterClientRequest>) -> Resp<ClientSession> {
		let id = self.registry.register()?;
		Ok(Response::new(ClientSession {
			context: Some(ClientContext {
				client_id: id.to_vec(),
				engine_incarnation: self.registry.incarnation().to_vec(),
			}),
			granted_capabilities: vec!["observe".into()],
			revision: 1,
		}))
	}
	async fn get_snapshot(&self, _: Request<ClientContext>) -> Resp<Snapshot> {
		unsupported()
	}
	type WatchStateStream = ReceiverStream<Result<StateEnvelope, Status>>;
	async fn watch_state(&self, _: Request<WatchStateRequest>) -> Resp<Self::WatchStateStream> {
		unsupported()
	}
	type WatchTelemetryStream = ReceiverStream<Result<TelemetrySnapshot, Status>>;
	async fn watch_telemetry(
		&self,
		r: Request<TelemetryRequest>,
	) -> Resp<Self::WatchTelemetryStream> {
		let r = r.into_inner();
		self.log
			.lock()
			.unwrap()
			.push(format!("telemetry {} {}", r.meters_hz, r.stats_hz));
		let (tx, rx) = mpsc::channel(2);
		tokio::spawn(async move {
			let _ = tx.send(Ok(TelemetrySnapshot::default())).await;
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
			Some(start_call_request::Target::ManualConnectionString(s)) => format!("conn={s}"),
			None => "none".into(),
		};
		self.op(r.context.as_ref(), format!("start_call {target}"))
	}
	async fn create_channel(&self, r: Request<CreateChannelRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		let policy = r.policy.unwrap_or_default();
		self.op(
			r.context.as_ref(),
			format!(
				"create_channel {} listed={} pw={} invite={} secret_len={}",
				r.name,
				policy.listed,
				policy.password,
				policy.invite_only,
				r.password.len()
			),
		)
	}
	async fn join_channel(&self, r: Request<JoinChannelRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!(
				"join_channel pw={} invite={}",
				r.password.len(),
				r.invite.len()
			),
		)
	}
	async fn leave_channel(&self, r: Request<LeaveChannelRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(r.context.as_ref(), "leave_channel".into())
	}
	async fn revoke_channel_member(
		&self,
		r: Request<RevokeChannelMemberRequest>,
	) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!("revoke fp={}", r.fingerprint.len()),
		)
	}
	async fn set_privacy(&self, r: Request<SetPrivacyRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!("privacy {:?} {:?}", r.inbound_policy, r.advert_mode),
		)
	}
	async fn list_channels(&self, _: Request<ListChannelsRequest>) -> Resp<ListChannelsResponse> {
		unsupported()
	}
	async fn create_invite(&self, _: Request<CreateInviteRequest>) -> Resp<CreateInviteResponse> {
		unsupported()
	}
	async fn get_avatar(&self, _: Request<GetAvatarRequest>) -> Resp<AvatarImage> {
		unsupported()
	}
	async fn get_connection_info(&self, _: Request<ClientContext>) -> Resp<ConnectionInfo> {
		unsupported()
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
		self.op(
			r.context.as_ref(),
			format!("decline gen={}", r.controller_generation),
		)
	}
	async fn end_call(&self, r: Request<CallMutation>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!("end gen={}", r.controller_generation),
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
				"route mode={} consent={} gen={}",
				r.mode, r.explicit_microphone_consent, r.controller_generation
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
	async fn set_profile(&self, r: Request<SetProfileRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(r.context.as_ref(), format!("profile {}", r.display_name))
	}
	async fn upload_avatar(&self, r: Request<UploadAvatarRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!("avatar {} {}", r.encoded_image.len(), r.declared_media_type),
		)
	}
	async fn decide_peer_trust(&self, r: Request<DecidePeerTrustRequest>) -> Resp<Operation> {
		let r = r.into_inner();
		self.op(
			r.context.as_ref(),
			format!("trust {} oob={}", r.requested_trust, r.out_of_band_verified),
		)
	}
	type InputControlStream = ReceiverStream<Result<InputControlAck, Status>>;
	async fn input_control(
		&self,
		_: Request<tonic::Streaming<InputControlMessage>>,
	) -> Resp<Self::InputControlStream> {
		unsupported()
	}
}

pub struct Harness {
	_dir: TempDir,
	socket: std::path::PathBuf,
	pub engine: FakeEngine,
	shutdown: Option<oneshot::Sender<()>>,
}

impl Harness {
	pub async fn start() -> Self {
		let dir = tempfile::Builder::new()
			.prefix("rlycl")
			.tempdir()
			.expect("tempdir");
		let socket = dir.path().join("run").join("engine.sock");
		let limits = ApiLimits::DEFAULT;
		let registry = Arc::new(ClientRegistry::new(limits.max_clients));
		let engine = FakeEngine {
			registry: Arc::clone(&registry),
			log: Arc::default(),
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
			_dir: dir,
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
}

impl Drop for Harness {
	fn drop(&mut self) {
		if let Some(tx) = self.shutdown.take() {
			let _ = tx.send(());
		}
	}
}
