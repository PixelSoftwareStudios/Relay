//! Minimal `Engine` implementation: enough state to test the API contract.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::channels::Channels;
use super::messages::Messages;
use super::settings::Settings;
use prost::Message;
use relay_api::pb::engine_server::Engine;
use relay_api::pb::state_envelope::Body;
use relay_api::pb::*;
use relay_api::{ApiLimits, ClientRegistry, StreamLimiter};
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

const HISTORY: usize = 1024;

#[derive(Default)]
struct State {
	revision: u64,
	history: Vec<StateEnvelope>,
	operations: HashMap<Vec<u8>, (Vec<u8>, Operation)>,
}

pub struct Inner {
	registry: Arc<ClientRegistry>,
	limits: ApiLimits,
	streams: StreamLimiter,
	state: Mutex<State>,
	events: broadcast::Sender<StateEnvelope>,
	pub channels: Channels,
	pub messages: Messages,
	pub settings: Settings,
}

/// Cheaply cloneable handle; the server and the test share one state.
#[derive(Clone)]
pub struct FakeEngine(Arc<Inner>);

impl std::ops::Deref for FakeEngine {
	type Target = Inner;
	fn deref(&self) -> &Inner {
		&self.0
	}
}

type Resp<T> = Result<Response<T>, Status>;
type Watch = ReceiverStream<Result<StateEnvelope, Status>>;

impl FakeEngine {
	pub fn new(registry: Arc<ClientRegistry>, limits: ApiLimits) -> Self {
		Self(Arc::new(Inner {
			registry,
			limits,
			streams: StreamLimiter::new(limits.max_streams),
			state: Mutex::new(State::default()),
			events: broadcast::channel(limits.subscriber_queue).0,
			channels: Channels::default(),
			messages: Messages::default(),
			settings: Settings::default(),
		}))
	}

	/// Publish a call-state event and return its revision.
	pub fn publish_call(&self, call_id: &[u8]) -> u64 {
		let mut state = self.state.lock().unwrap();
		Self::publish_locked(&mut state, &self.registry, &self.events, call_id)
	}

	fn publish_locked(
		state: &mut State,
		registry: &ClientRegistry,
		events: &broadcast::Sender<StateEnvelope>,
		call_id: &[u8],
	) -> u64 {
		state.revision += 1;
		let call = Call {
			call_id: call_id.to_vec(),
			state: CallState::Negotiating as i32,
			revision: state.revision,
			..Call::default()
		};
		let envelope = StateEnvelope {
			engine_incarnation: registry.incarnation().to_vec(),
			revision: state.revision,
			body: Some(Body::Event(StateEvent {
				event: Some(state_event::Event::Call(call)),
			})),
		};
		state.history.push(envelope.clone());
		if state.history.len() > HISTORY {
			state.history.remove(0);
		}
		let _ = events.send(envelope);
		state.revision
	}

	fn snapshot_locked(&self, state: &State) -> Snapshot {
		Snapshot {
			engine_incarnation: self.registry.incarnation().to_vec(),
			revision: state.revision,
			..Snapshot::default()
		}
	}
}

fn unsupported<T>() -> Resp<T> {
	Err(Status::unimplemented("not part of the fake engine"))
}

#[tonic::async_trait]
impl Engine for FakeEngine {
	async fn get_info(&self, _: Request<Empty>) -> Resp<EngineInfo> {
		Ok(Response::new(EngineInfo {
			engine_incarnation: self.registry.incarnation().to_vec(),
			application_version: "fake".into(),
			api_major: 1,
			api_minor: 0,
			build_revision: "fake".into(),
			api_revision: 0,
			capabilities: vec!["observe".into(), "call-control".into()],
			limits: Some(ResourceLimits {
				clients: self.limits.max_clients as u32,
				..ResourceLimits::default()
			}),
		}))
	}

	async fn register_client(&self, r: Request<RegisterClientRequest>) -> Resp<ClientSession> {
		let id = self.registry.register()?;
		self.messages
			.grant(id.to_vec(), r.into_inner().requested_capabilities);
		let revision = self.state.lock().unwrap().revision;
		Ok(Response::new(ClientSession {
			context: Some(ClientContext {
				client_id: id.to_vec(),
				engine_incarnation: self.registry.incarnation().to_vec(),
			}),
			granted_capabilities: vec!["observe".into()],
			revision,
			daemon_build_revision: "fake".into(),
			daemon_api_revision: 0,
		}))
	}

	async fn get_snapshot(&self, _: Request<ClientContext>) -> Resp<Snapshot> {
		let state = self.state.lock().unwrap();
		Ok(Response::new(self.snapshot_locked(&state)))
	}

	type WatchStateStream = Watch;

	async fn watch_state(&self, request: Request<WatchStateRequest>) -> Resp<Watch> {
		let guard = self.streams.acquire()?;
		let after = request.into_inner().after_revision;
		let incarnation = self.registry.incarnation().to_vec();
		// Snapshot/replay and subscription happen under one lock: no gap.
		let (initial, mut live) = {
			let state = self.state.lock().unwrap();
			let live = self.events.subscribe();
			let initial = match after {
				None => vec![StateEnvelope {
					engine_incarnation: incarnation.clone(),
					revision: state.revision,
					body: Some(Body::Snapshot(self.snapshot_locked(&state))),
				}],
				Some(r)
					if r <= state.revision
						&& state.history.first().is_none_or(|h| h.revision <= r + 1) =>
				{
					state
						.history
						.iter()
						.filter(|e| e.revision > r)
						.cloned()
						.collect()
				}
				Some(_) => vec![StateEnvelope {
					engine_incarnation: incarnation.clone(),
					revision: state.revision,
					body: Some(Body::Resync(ResyncRequired {
						current_revision: state.revision,
						reason: "replay window exceeded".into(),
					})),
				}],
			};
			(initial, live)
		};
		let mut last = initial.last().map_or(after.unwrap_or(0), |e| e.revision);
		let (tx, rx) = mpsc::channel(self.limits.subscriber_queue);
		tokio::spawn(async move {
			let _guard = guard;
			for envelope in initial {
				if tx.send(Ok(envelope)).await.is_err() {
					return;
				}
			}
			while let Ok(envelope) = live.recv().await {
				if envelope.revision <= last {
					continue;
				}
				last = envelope.revision;
				if tx.send(Ok(envelope)).await.is_err() {
					return;
				}
			}
		});
		Ok(Response::new(ReceiverStream::new(rx)))
	}

	type WatchTelemetryStream = ReceiverStream<Result<TelemetrySnapshot, Status>>;

	async fn watch_telemetry(
		&self,
		_: Request<TelemetryRequest>,
	) -> Resp<Self::WatchTelemetryStream> {
		unsupported()
	}

	async fn get_operation(&self, request: Request<GetOperationRequest>) -> Resp<Operation> {
		let id = request.into_inner().operation_id;
		let state = self.state.lock().unwrap();
		state
			.operations
			.get(&id)
			.map(|(_, op)| Response::new(op.clone()))
			.ok_or_else(|| Status::not_found("unknown operation"))
	}

	async fn start_call(&self, request: Request<StartCallRequest>) -> Resp<Operation> {
		let req = request.into_inner();
		let op_id = req
			.context
			.as_ref()
			.map(|c| c.operation_id.clone())
			.filter(|id| id.len() == 16)
			.ok_or_else(|| Status::invalid_argument("operation_id must be 16 bytes"))?;
		if let Some(start_call_request::Target::ManualConnectionString(s)) = &req.target {
			*self.channels.last_connection_string.lock().unwrap() = Some(s.clone());
		}
		let fingerprint = req.encode_to_vec();
		let mut state = self.state.lock().unwrap();
		if let Some((seen, op)) = state.operations.get(&op_id) {
			return if *seen == fingerprint {
				Ok(Response::new(op.clone()))
			} else {
				Err(Status::already_exists("OPERATION_ID_REUSED"))
			};
		}
		let call_id = op_id.clone();
		let revision = Self::publish_locked(&mut state, &self.registry, &self.events, &call_id);
		let op = Operation {
			operation_id: op_id.clone(),
			state: OperationState::Applied as i32,
			revision,
			call_id,
			problem: None,
		};
		state.operations.insert(op_id, (fingerprint, op.clone()));
		Ok(Response::new(op))
	}

	async fn accept_call(&self, _: Request<CallMutation>) -> Resp<Operation> {
		unsupported()
	}
	async fn decline_call(&self, _: Request<CallMutation>) -> Resp<Operation> {
		unsupported()
	}
	async fn end_call(&self, _: Request<CallMutation>) -> Resp<Operation> {
		unsupported()
	}
	async fn acquire_controller(
		&self,
		_: Request<AcquireControllerRequest>,
	) -> Resp<ControllerLease> {
		unsupported()
	}
	async fn release_controller(&self, _: Request<ReleaseControllerRequest>) -> Resp<Operation> {
		unsupported()
	}
	async fn set_transmit_mode(&self, _: Request<SetTransmitModeRequest>) -> Resp<Operation> {
		unsupported()
	}
	async fn connect_and_trust(&self, _: Request<ConnectAndTrustRequest>) -> Resp<Operation> {
		unsupported()
	}
	type WatchCallStatsStream = ReceiverStream<Result<CallStats, Status>>;
	async fn watch_call_stats(
		&self,
		_: Request<WatchCallStatsRequest>,
	) -> Resp<Self::WatchCallStatsStream> {
		unsupported()
	}
	async fn set_call_controls(&self, _: Request<SetCallControlsRequest>) -> Resp<Operation> {
		unsupported()
	}
	async fn set_global_controls(&self, _: Request<SetGlobalControlsRequest>) -> Resp<Operation> {
		unsupported()
	}
	async fn select_devices(&self, _: Request<SelectDevicesRequest>) -> Resp<Operation> {
		unsupported()
	}
	async fn set_noise_suppression(
		&self,
		_: Request<SetNoiseSuppressionRequest>,
	) -> Resp<Operation> {
		unsupported()
	}
	async fn start_mic_test(&self, _: Request<StartMicTestRequest>) -> Resp<Operation> {
		unsupported()
	}
	async fn stop_mic_test(&self, _: Request<MutationContext>) -> Resp<Operation> {
		unsupported()
	}
	async fn preview_tone(&self, _: Request<MutationContext>) -> Resp<Operation> {
		unsupported()
	}
	async fn set_profile(&self, _: Request<SetProfileRequest>) -> Resp<Operation> {
		unsupported()
	}
	async fn upload_avatar(&self, _: Request<UploadAvatarRequest>) -> Resp<Operation> {
		unsupported()
	}
	async fn decide_peer_trust(&self, _: Request<DecidePeerTrustRequest>) -> Resp<Operation> {
		unsupported()
	}
	async fn clear_avatar(&self, r: Request<MutationContext>) -> Resp<Operation> {
		Ok(Response::new(self.settings.clear_avatar(r.get_ref())))
	}
	async fn forget_peer(&self, r: Request<ForgetPeerRequest>) -> Resp<Operation> {
		self.settings.forget(r.get_ref()).map(Response::new)
	}
	async fn set_favorites(&self, r: Request<SetFavoritesRequest>) -> Resp<Operation> {
		Ok(Response::new(self.settings.set_favorites(r.get_ref())))
	}
	async fn list_call_history(&self, r: Request<ListCallHistoryRequest>) -> Resp<CallHistoryPage> {
		Ok(Response::new(self.settings.list_history(r.get_ref())))
	}
	async fn clear_call_history(&self, r: Request<MutationContext>) -> Resp<Operation> {
		Ok(Response::new(self.settings.clear_history(r.get_ref())))
	}
	async fn get_settings(&self, _: Request<ClientContext>) -> Resp<SettingsDump> {
		Ok(Response::new(self.settings.dump()))
	}
	async fn set_settings(&self, r: Request<SetSettingsRequest>) -> Resp<SetSettingsResponse> {
		self.settings.apply(r.get_ref()).map(Response::new)
	}

	async fn create_channel(&self, r: Request<CreateChannelRequest>) -> Resp<Operation> {
		Ok(Response::new(self.channels.create(r.get_ref())))
	}
	async fn join_channel(&self, r: Request<JoinChannelRequest>) -> Resp<Operation> {
		Ok(Response::new(self.channels.join(r.get_ref())))
	}
	async fn leave_channel(&self, r: Request<LeaveChannelRequest>) -> Resp<Operation> {
		Ok(Response::new(self.channels.leave(r.get_ref())))
	}
	async fn list_channels(&self, _: Request<ListChannelsRequest>) -> Resp<ListChannelsResponse> {
		Ok(Response::new(ListChannelsResponse {
			channels: self.channels.list(),
		}))
	}
	async fn create_invite(&self, r: Request<CreateInviteRequest>) -> Resp<CreateInviteResponse> {
		self.channels
			.invite(r.get_ref())
			.map(Response::new)
			.ok_or_else(|| Status::permission_denied("CHANNEL_DENIED"))
	}
	async fn revoke_channel_member(
		&self,
		r: Request<RevokeChannelMemberRequest>,
	) -> Resp<Operation> {
		Ok(Response::new(self.channels.revoke(r.get_ref())))
	}
	async fn set_privacy(&self, _: Request<SetPrivacyRequest>) -> Resp<Operation> {
		unsupported()
	}
	async fn get_avatar(&self, r: Request<GetAvatarRequest>) -> Resp<AvatarImage> {
		self.channels
			.avatar(&r.get_ref().avatar_sha256)
			.map(Response::new)
			.ok_or_else(|| Status::not_found("unknown avatar"))
	}
	async fn shutdown(&self, _: Request<MutationContext>) -> Resp<Operation> {
		unsupported()
	}
	async fn get_connection_info(&self, _: Request<ClientContext>) -> Resp<ConnectionInfo> {
		Ok(Response::new(ConnectionInfo {
			local_addresses: vec!["192.168.1.20".into()],
			listen_port: 47000,
			fingerprint: vec![7; 32],
			connection_string: "RLYN1-fake".into(),
		}))
	}

	async fn list_sessions(&self, r: Request<ClientContext>) -> Resp<ListSessionsResponse> {
		self.messages.require(&r)?;
		Ok(Response::new(self.messages.list()))
	}
	async fn get_transcript(&self, r: Request<GetTranscriptRequest>) -> Resp<Transcript> {
		self.messages.require(&r)?;
		Ok(Response::new(self.messages.transcript(r.get_ref())?))
	}
	type WatchSessionStream = ReceiverStream<Result<SessionEnvelope, Status>>;
	async fn watch_session(
		&self,
		r: Request<WatchSessionRequest>,
	) -> Resp<Self::WatchSessionStream> {
		self.messages.require(&r)?;
		let guard = self.streams.acquire()?;
		let after = r.into_inner().after_revision;
		let incarnation = self.registry.incarnation().to_vec();
		let (initial, mut live) = self.messages.watch(after, &incarnation);
		let mut last = initial.last().map_or(after.unwrap_or(0), |e| e.revision);
		let (tx, rx) = mpsc::channel(self.limits.subscriber_queue);
		tokio::spawn(async move {
			let _guard = guard;
			for envelope in initial {
				if tx.send(Ok(envelope)).await.is_err() {
					return;
				}
			}
			loop {
				match live.recv().await {
					Ok((revision, event)) if revision > last => {
						last = revision;
						let envelope = SessionEnvelope {
							engine_incarnation: incarnation.clone(),
							revision,
							body: Some(session_envelope::Body::Event(event)),
						};
						if tx.send(Ok(envelope)).await.is_err() {
							return;
						}
					}
					Ok(_) => {}
					Err(broadcast::error::RecvError::Lagged(_)) => {
						let envelope = SessionEnvelope {
							engine_incarnation: incarnation.clone(),
							revision: last,
							body: Some(session_envelope::Body::Resync(SessionResync {
								current_revision: last,
								reason: "subscriber lagged".into(),
							})),
						};
						let _ = tx.send(Ok(envelope)).await;
						return;
					}
					Err(_) => return,
				}
			}
		});
		Ok(Response::new(ReceiverStream::new(rx)))
	}
	async fn send_session_message(
		&self,
		r: Request<SendSessionMessageRequest>,
	) -> Resp<SessionOpResult> {
		self.messages.require(&r)?;
		Ok(Response::new(self.messages.send(r.get_ref())))
	}
	async fn search_session(
		&self,
		r: Request<SearchSessionRequest>,
	) -> Resp<SearchSessionResponse> {
		self.messages.require(&r)?;
		Ok(Response::new(self.messages.search(r.get_ref())))
	}
	async fn send_session_image(
		&self,
		_: Request<SendSessionImageRequest>,
	) -> Resp<SessionOpResult> {
		unsupported()
	}
	async fn retry_session_message(
		&self,
		_: Request<RetrySessionMessageRequest>,
	) -> Resp<SessionOpResult> {
		unsupported()
	}
	async fn offer_file(&self, _: Request<OfferFileRequest>) -> Resp<SessionOpResult> {
		unsupported()
	}
	async fn respond_offer(&self, _: Request<RespondOfferRequest>) -> Resp<SessionOpResult> {
		unsupported()
	}
	async fn request_preview(&self, _: Request<OfferRequest>) -> Resp<SessionOpResult> {
		unsupported()
	}
	async fn approve_preview(&self, _: Request<ApprovePreviewRequest>) -> Resp<SessionOpResult> {
		unsupported()
	}
	async fn cancel_transfer(&self, _: Request<OfferRequest>) -> Resp<SessionOpResult> {
		unsupported()
	}
	async fn set_session_permissions(
		&self,
		_: Request<SetSessionPermissionsRequest>,
	) -> Resp<SessionOpResult> {
		unsupported()
	}
	async fn pin_message(&self, _: Request<PinMessageRequest>) -> Resp<SessionOpResult> {
		unsupported()
	}
	async fn clear_session(&self, _: Request<ClearSessionRequest>) -> Resp<SessionOpResult> {
		unsupported()
	}
	async fn export_session(
		&self,
		_: Request<ExportSessionRequest>,
	) -> Resp<ExportSessionResponse> {
		unsupported()
	}
	async fn set_messaging_settings(
		&self,
		_: Request<SetMessagingSettingsRequest>,
	) -> Resp<SessionOpResult> {
		unsupported()
	}

	type InputControlStream = ReceiverStream<Result<InputControlAck, Status>>;

	async fn input_control(
		&self,
		_: Request<tonic::Streaming<InputControlMessage>>,
	) -> Resp<Self::InputControlStream> {
		unsupported()
	}
}
