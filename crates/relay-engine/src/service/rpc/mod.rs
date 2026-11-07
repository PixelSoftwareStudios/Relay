//! The local API: `relay.v1.Engine` implemented over the control actor.

mod avatar;
mod call_stats;
mod channels;
mod commands;
mod connect_trust;
mod connection;
mod favorites;
mod forget;
mod history;
mod input;
mod messages;
mod messages_watch;
mod ops;
mod privacy;
mod replace_key;
mod session;
mod settings;
mod telemetry;
mod watch;

use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use relay_api::pb::{
	AcquireControllerRequest, ApprovePreviewRequest, AvatarImage, CallHistoryPage, CallMutation,
	ClearSessionRequest, ClientContext, ClientSession, ConnectionInfo, ControllerLease,
	CreateChannelRequest, CreateInviteRequest, CreateInviteResponse, DecidePeerTrustRequest, Empty,
	EngineInfo, ExportSessionRequest, ExportSessionResponse, ForgetPeerRequest, GetAvatarRequest,
	GetOperationRequest, GetTranscriptRequest, InputControlAck, InputControlMessage,
	JoinChannelRequest, LeaveChannelRequest, ListCallHistoryRequest, ListChannelsRequest,
	ListChannelsResponse, ListSessionsResponse, MutationContext, OfferFileRequest, OfferRequest,
	Operation, OperationState, PinMessageRequest, RegisterClientRequest, ReleaseControllerRequest,
	RespondOfferRequest, RetrySessionMessageRequest, RevokeChannelMemberRequest,
	SearchSessionRequest, SearchSessionResponse, SelectDevicesRequest, SendSessionImageRequest,
	SendSessionMessageRequest, SessionEnvelope, SessionOpResult, SetCallControlsRequest,
	SetFavoritesRequest, SetGlobalControlsRequest, SetMessagingSettingsRequest, SetPrivacyRequest,
	SetProfileRequest, SetRingtoneRequest, SetSessionPermissionsRequest, SetSettingsRequest,
	SetSettingsResponse, SetTransmitModeRequest, SettingsDump, Snapshot, StartCallRequest,
	StartMicTestRequest, StateEnvelope, TelemetryRequest, TelemetrySnapshot, Transcript,
	UploadAvatarRequest, WatchSessionRequest, WatchStateRequest, engine_server::Engine,
};
use relay_api::{ApiLimits, CLIENT_ID_HEADER, ClientRegistry, StreamLimiter};
use relay_core::{CallId, ClientId, Limits, Reject, Request as CoreRequest};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};

use super::clients::{Capability, ClientBook};
use super::convert::{Mutation, check_context, check_mutation, client_id, lease_bytes};
use super::error::ServiceError;
use super::handle::{Applied, ControlHandle};
use super::ids::Ids;
use super::ports::{MediaControl, PeerControl, SettingsStore};
use relay_core::Command;

pub(crate) const API_MAJOR: u32 = 1;

/// State shared by every handler.
pub(crate) struct Shared {
	/// Set when an admin client asked the engine to stop.
	pub shutdown: Arc<tokio::sync::watch::Sender<bool>>,
	/// Which clients hold an open state watch.
	pub presence: Arc<super::presence::PresenceLink>,
	pub handle: ControlHandle,
	pub registry: Arc<ClientRegistry>,
	pub book: Arc<ClientBook>,
	pub limiter: StreamLimiter,
	pub ids: Ids,
	pub peers: Arc<dyn PeerControl>,
	pub media: Arc<dyn MediaControl>,
	pub settings: Arc<dyn SettingsStore>,
	pub started: tokio::time::Instant,
	pub api: ApiLimits,
	pub limits: Limits,
	pub next_source: AtomicU64,
	/// The channel driver, when the engine runs real adapters.
	pub channels: Option<crate::channels::ChannelHub>,
	/// The session messaging actor, when the engine runs real adapters.
	pub messaging: Option<crate::messaging::MessagingHandle>,
}

/// The `relay.v1.Engine` implementation. Cheap to clone.
#[derive(Clone)]
pub struct EngineService {
	pub(crate) shared: Arc<Shared>,
}

impl EngineService {
	pub(crate) fn new(shared: Shared) -> Self {
		Self {
			shared: Arc::new(shared),
		}
	}

	pub(crate) fn incarnation(&self) -> [u8; 16] {
		self.shared.registry.incarnation()
	}

	/// The caller named by the transport header.
	fn caller<T>(&self, request: &Request<T>) -> Result<ClientId, Status> {
		let text = request
			.metadata()
			.get(CLIENT_ID_HEADER)
			.and_then(|value| value.to_str().ok())
			.ok_or_else(|| Status::unauthenticated("missing client context"))?;
		let bytes =
			hex::decode(text).map_err(|_| Status::invalid_argument("malformed client context"))?;
		Ok(client_id(&bytes)?)
	}

	/// Validates the header against the body context for read-only calls.
	fn observer<T>(
		&self,
		request: &Request<T>,
		context: impl FnOnce(&T) -> Option<&ClientContext>,
	) -> Result<ClientId, Status> {
		let caller = self.caller(request)?;
		check_context(caller, context(request.get_ref()), &self.incarnation())?;
		self.shared.book.require(caller, Capability::Observe)?;
		Ok(caller)
	}

	/// Validates the envelope of a mutation and the caller's capability.
	fn begin<R>(
		&self,
		request: Request<R>,
		context: impl FnOnce(&R) -> Option<&MutationContext>,
	) -> Result<(Mutation, R), Status> {
		let caller = self.caller(&request)?;
		let body = request.into_inner();
		let mutation = check_mutation(caller, context(&body), &self.incarnation())?;
		self.shared.book.require(caller, Capability::CallControl)?;
		Ok((mutation, body))
	}

	/// Runs a domain command and returns its raw result.
	async fn dispatch(&self, mutation: &Mutation, command: Command) -> Result<Applied, Status> {
		let request = CoreRequest {
			client: mutation.client,
			operation: mutation.operation,
			command,
		};
		Ok(self.shared.handle.submit(request).await?)
	}

	/// Runs a domain command and renders the outcome as an [`Operation`].
	async fn run(
		&self,
		mutation: &Mutation,
		command: Command,
		call: Option<CallId>,
	) -> Result<Response<Operation>, Status> {
		let applied = self.dispatch(mutation, command).await?;
		let mut operation = Operation {
			operation_id: mutation.operation.to_bytes().to_vec(),
			state: OperationState::Applied as i32,
			revision: applied.api_revision,
			call_id: call.map(|c| c.to_bytes().to_vec()).unwrap_or_default(),
			problem: None,
		};
		match applied.result {
			Ok(_) => {}
			Err(Reject::OperationIdReused) => return Err(ServiceError::OperationReused.into()),
			Err(reject) => {
				operation.state = OperationState::Failed as i32;
				operation.problem = Some(super::convert::problem_of(&reject));
			}
		}
		Ok(Response::new(operation))
	}
}

type Stream<T> = ReceiverStream<Result<T, Status>>;

#[tonic::async_trait]
impl Engine for EngineService {
	async fn get_info(&self, _: Request<Empty>) -> Result<Response<EngineInfo>, Status> {
		Ok(Response::new(session::info(self)))
	}

	// The three start-up calls are answered by `relay_api::EngineSwitch` in front of this
	// service, in every state; they never reach it.
	async fn retry_keychain(&self, _: Request<Empty>) -> Result<Response<EngineInfo>, Status> {
		Err(Status::unimplemented("answered before the engine"))
	}

	async fn use_file_store(&self, _: Request<Empty>) -> Result<Response<EngineInfo>, Status> {
		Err(Status::unimplemented("answered before the engine"))
	}

	async fn reset_identity(&self, _: Request<Empty>) -> Result<Response<EngineInfo>, Status> {
		Err(Status::unimplemented("answered before the engine"))
	}

	async fn register_client(
		&self,
		request: Request<RegisterClientRequest>,
	) -> Result<Response<ClientSession>, Status> {
		session::register(self, request.into_inner()).await
	}

	async fn get_snapshot(
		&self,
		request: Request<ClientContext>,
	) -> Result<Response<Snapshot>, Status> {
		self.observer(&request, |body| Some(body))?;
		Ok(Response::new(self.shared.handle.snapshot().await?))
	}

	type WatchStateStream = Stream<StateEnvelope>;

	async fn watch_state(
		&self,
		request: Request<WatchStateRequest>,
	) -> Result<Response<Self::WatchStateStream>, Status> {
		let caller = self.observer(&request, |body| body.client.as_ref())?;
		watch::open(self, caller, request.into_inner()).await
	}

	async fn connect_and_trust(
		&self,
		request: Request<relay_api::pb::ConnectAndTrustRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		connect_trust::connect(self, &mutation, &body).await
	}

	type WatchCallStatsStream = Stream<relay_api::pb::CallStats>;

	async fn watch_call_stats(
		&self,
		request: Request<relay_api::pb::WatchCallStatsRequest>,
	) -> Result<Response<Self::WatchCallStatsStream>, Status> {
		let caller = self.observer(&request, |body| body.client.as_ref())?;
		call_stats::open(self, caller, request.into_inner())
	}

	type WatchTelemetryStream = Stream<TelemetrySnapshot>;

	async fn watch_telemetry(
		&self,
		request: Request<TelemetryRequest>,
	) -> Result<Response<Self::WatchTelemetryStream>, Status> {
		let caller = self.observer(&request, |body| body.client.as_ref())?;
		telemetry::open(self, caller, request.into_inner())
	}

	async fn get_operation(
		&self,
		request: Request<GetOperationRequest>,
	) -> Result<Response<Operation>, Status> {
		let caller = self.observer(&request, |body| body.client.as_ref())?;
		let id = super::convert::operation_id(&request.get_ref().operation_id)?;
		match self.shared.handle.operation(caller, id).await? {
			Some(operation) => Ok(Response::new(operation)),
			None => match self
				.shared
				.channels
				.as_ref()
				.and_then(|c| c.operation(caller, id))
			{
				Some(operation) => Ok(Response::new(operation)),
				None => Err(Status::not_found("OPERATION_UNKNOWN_OR_EXPIRED")),
			},
		}
	}

	async fn start_call(
		&self,
		request: Request<StartCallRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		let (command, call) = commands::start_call(self, &mutation, &body).await?;
		self.run(&mutation, command, Some(call)).await
	}

	async fn accept_call(
		&self,
		request: Request<CallMutation>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		let (call, lease) = commands::call_and_lease(&body)?;
		self.run(&mutation, Command::AcceptCall { call, lease }, Some(call))
			.await
	}

	async fn decline_call(
		&self,
		request: Request<CallMutation>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		let (call, lease) = commands::call_and_lease(&body)?;
		self.run(&mutation, Command::DeclineCall { call, lease }, Some(call))
			.await
	}

	async fn end_call(
		&self,
		request: Request<CallMutation>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		let (call, lease) = commands::call_and_lease(&body)?;
		self.run(&mutation, Command::EndCall { call, lease }, Some(call))
			.await
	}

	async fn acquire_controller(
		&self,
		request: Request<AcquireControllerRequest>,
	) -> Result<Response<ControllerLease>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		let call = super::convert::call_id(&body.call_id)?;
		let command = Command::AcquireController {
			call,
			takeover: body.request_explicit_takeover,
		};
		let applied = self.dispatch(&mutation, command).await?;
		match applied.result {
			Ok(relay_core::Reply::Controller(lease)) => Ok(Response::new(ControllerLease {
				call_id: call.to_bytes().to_vec(),
				controller_client_id: mutation.client.to_bytes().to_vec(),
				generation: lease.get(),
				lease_id: lease_bytes(lease),
				expires_in_ms: 0,
			})),
			Ok(_) => Err(Status::internal(
				"unexpected reply to controller acquisition",
			)),
			Err(reject) => Err(commands::reject_status(&reject)),
		}
	}

	async fn release_controller(
		&self,
		request: Request<ReleaseControllerRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		let command = commands::release(&body)?;
		let call = super::convert::call_id(&body.call_id)?;
		self.run(&mutation, command, Some(call)).await
	}

	async fn set_transmit_mode(
		&self,
		request: Request<SetTransmitModeRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		let (command, call) = commands::set_mode(&body)?;
		self.run(&mutation, command, Some(call)).await
	}

	async fn set_call_controls(
		&self,
		request: Request<SetCallControlsRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		let (command, call) = commands::call_controls(&body)?;
		self.run(&mutation, command, Some(call)).await
	}

	async fn set_global_controls(
		&self,
		request: Request<SetGlobalControlsRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		ops::global_controls(self, &mutation, &body).await
	}

	async fn select_devices(
		&self,
		request: Request<SelectDevicesRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		ops::select_devices(self, &mutation, &body).await
	}

	async fn start_mic_test(
		&self,
		request: Request<StartMicTestRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		ops::start_mic_test(self, &mutation, &body).await
	}

	async fn stop_mic_test(
		&self,
		request: Request<MutationContext>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| Some(b))?;
		ops::stop_mic_test(self, &mutation, &body).await
	}

	async fn preview_tone(
		&self,
		request: Request<MutationContext>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| Some(b))?;
		ops::preview_tone(self, &mutation, &body).await
	}

	async fn set_ringtone(
		&self,
		request: Request<SetRingtoneRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		ops::set_ringtone(self, &mutation, &body).await
	}

	async fn set_profile(
		&self,
		request: Request<SetProfileRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		ops::profile(self, &mutation, &body).await
	}

	async fn upload_avatar(
		&self,
		request: Request<UploadAvatarRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		ops::avatar(self, &mutation, &body).await
	}

	async fn decide_peer_trust(
		&self,
		request: Request<DecidePeerTrustRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		ops::trust(self, &mutation, &body).await
	}

	async fn clear_avatar(
		&self,
		request: Request<MutationContext>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| Some(b))?;
		ops::clear_avatar(self, &mutation, &body).await
	}

	async fn forget_peer(
		&self,
		request: Request<ForgetPeerRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		forget::forget(self, &mutation, &body).await
	}

	async fn replace_pinned_key(
		&self,
		request: Request<relay_api::pb::ReplacePinnedKeyRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		replace_key::replace(self, &mutation, &body).await
	}

	async fn set_favorites(
		&self,
		request: Request<SetFavoritesRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		favorites::set(self, &mutation, &body).await
	}

	async fn get_settings(
		&self,
		request: Request<ClientContext>,
	) -> Result<Response<SettingsDump>, Status> {
		settings::get(self, request).await
	}

	async fn set_settings(
		&self,
		request: Request<SetSettingsRequest>,
	) -> Result<Response<SetSettingsResponse>, Status> {
		settings::set(self, request).await
	}

	async fn create_channel(
		&self,
		request: Request<CreateChannelRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		channels::create(self, &mutation, body).await
	}

	async fn join_channel(
		&self,
		request: Request<JoinChannelRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		channels::join(self, &mutation, body).await
	}

	async fn leave_channel(
		&self,
		request: Request<LeaveChannelRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		channels::leave(self, &mutation, body).await
	}

	async fn list_channels(
		&self,
		request: Request<ListChannelsRequest>,
	) -> Result<Response<ListChannelsResponse>, Status> {
		self.observer(&request, |body| body.client.as_ref())?;
		channels::list(self)
	}

	async fn create_invite(
		&self,
		request: Request<CreateInviteRequest>,
	) -> Result<Response<CreateInviteResponse>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		channels::invite(self, &mutation, body).await
	}

	async fn revoke_channel_member(
		&self,
		request: Request<RevokeChannelMemberRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		channels::revoke(self, &mutation, body).await
	}

	async fn set_privacy(
		&self,
		request: Request<SetPrivacyRequest>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, body) = self.begin(request, |b| b.context.as_ref())?;
		privacy::set(self, &mutation, &body).await
	}

	async fn get_avatar(
		&self,
		request: Request<GetAvatarRequest>,
	) -> Result<Response<AvatarImage>, Status> {
		self.observer(&request, |body| body.client.as_ref())?;
		avatar::get(self, request.into_inner()).await
	}

	async fn get_connection_info(
		&self,
		request: Request<ClientContext>,
	) -> Result<Response<ConnectionInfo>, Status> {
		self.observer(&request, |body| Some(body))?;
		connection::get(self).await
	}

	async fn shutdown(
		&self,
		request: Request<MutationContext>,
	) -> Result<Response<Operation>, Status> {
		let caller = self.caller(&request)?;
		let body = request.into_inner();
		let mutation = check_mutation(caller, Some(&body), &self.incarnation())?;
		self.shared.book.require(caller, Capability::Admin)?;
		Ok(Response::new(session::shutdown(self, &mutation)))
	}

	async fn list_call_history(
		&self,
		request: Request<ListCallHistoryRequest>,
	) -> Result<Response<CallHistoryPage>, Status> {
		self.observer(&request, |body| body.client.as_ref())?;
		history::list(self, request.into_inner()).await
	}

	async fn clear_call_history(
		&self,
		request: Request<MutationContext>,
	) -> Result<Response<Operation>, Status> {
		let (mutation, _) = self.begin(request, |body| Some(body))?;
		history::clear(self, &mutation).await
	}

	async fn list_sessions(
		&self,
		request: Request<ClientContext>,
	) -> Result<Response<ListSessionsResponse>, Status> {
		messages::list_sessions(self, request).await
	}

	async fn get_transcript(
		&self,
		request: Request<GetTranscriptRequest>,
	) -> Result<Response<Transcript>, Status> {
		messages::get_transcript(self, request).await
	}

	type WatchSessionStream = Stream<SessionEnvelope>;

	async fn watch_session(
		&self,
		request: Request<WatchSessionRequest>,
	) -> Result<Response<Self::WatchSessionStream>, Status> {
		messages_watch::watch_session(self, request).await
	}

	async fn send_session_message(
		&self,
		request: Request<SendSessionMessageRequest>,
	) -> Result<Response<SessionOpResult>, Status> {
		messages::send_message(self, request).await
	}

	async fn send_session_image(
		&self,
		request: Request<SendSessionImageRequest>,
	) -> Result<Response<SessionOpResult>, Status> {
		messages::send_image(self, request).await
	}

	async fn retry_session_message(
		&self,
		request: Request<RetrySessionMessageRequest>,
	) -> Result<Response<SessionOpResult>, Status> {
		messages::retry_message(self, request).await
	}

	async fn offer_file(
		&self,
		request: Request<OfferFileRequest>,
	) -> Result<Response<SessionOpResult>, Status> {
		messages::offer_file(self, request).await
	}

	async fn respond_offer(
		&self,
		request: Request<RespondOfferRequest>,
	) -> Result<Response<SessionOpResult>, Status> {
		messages::respond_offer(self, request).await
	}

	async fn request_preview(
		&self,
		request: Request<OfferRequest>,
	) -> Result<Response<SessionOpResult>, Status> {
		messages::request_preview(self, request).await
	}

	async fn approve_preview(
		&self,
		request: Request<ApprovePreviewRequest>,
	) -> Result<Response<SessionOpResult>, Status> {
		messages::approve_preview(self, request).await
	}

	async fn cancel_transfer(
		&self,
		request: Request<OfferRequest>,
	) -> Result<Response<SessionOpResult>, Status> {
		messages::cancel_transfer(self, request).await
	}

	async fn set_session_permissions(
		&self,
		request: Request<SetSessionPermissionsRequest>,
	) -> Result<Response<SessionOpResult>, Status> {
		messages::set_permissions(self, request).await
	}

	async fn pin_message(
		&self,
		request: Request<PinMessageRequest>,
	) -> Result<Response<SessionOpResult>, Status> {
		messages::pin_message(self, request).await
	}

	async fn search_session(
		&self,
		request: Request<SearchSessionRequest>,
	) -> Result<Response<SearchSessionResponse>, Status> {
		messages::search_session(self, request).await
	}

	async fn clear_session(
		&self,
		request: Request<ClearSessionRequest>,
	) -> Result<Response<SessionOpResult>, Status> {
		messages::clear_session(self, request).await
	}

	async fn export_session(
		&self,
		request: Request<ExportSessionRequest>,
	) -> Result<Response<ExportSessionResponse>, Status> {
		messages::export_session(self, request).await
	}

	async fn set_messaging_settings(
		&self,
		request: Request<SetMessagingSettingsRequest>,
	) -> Result<Response<SessionOpResult>, Status> {
		messages::set_settings(self, request).await
	}

	type InputControlStream = Stream<InputControlAck>;

	async fn input_control(
		&self,
		request: Request<Streaming<InputControlMessage>>,
	) -> Result<Response<Self::InputControlStream>, Status> {
		let caller = self.caller(&request)?;
		self.shared.book.require(caller, Capability::CallControl)?;
		input::open(self, caller, request.into_inner())
	}
}
