//! Session messaging RPCs. Every call, reads and streams included, needs the `messages`
//! capability: a client without it gets `PERMISSION_DENIED` and never sees an event.

use std::io::Write;

use prost::Message;
use relay_api::pb::{
	ApprovePreviewRequest, ClearSessionRequest, ClientContext, ExportSessionRequest,
	ExportSessionResponse, GetTranscriptRequest, ListSessionsResponse, MutationContext,
	OfferFileRequest, OfferRequest, OperationState, PinMessageRequest, RespondOfferRequest,
	RetrySessionMessageRequest, SearchSessionRequest, SearchSessionResponse,
	SendSessionImageRequest, SendSessionMessageRequest, SessionOpResult,
	SetMessagingSettingsRequest, SetSessionPermissionsRequest, Transcript,
};
use relay_core::ClientId;
use tonic::{Request, Response, Status};

use super::EngineService;
use crate::messaging::{
	Caller, EXPORT_INLINE_MAX, MessagingHandle, Refusal, SourceError, prepare, preview_text,
};
use crate::service::clients::Capability;
use crate::service::convert::{check_context, check_mutation};
use crate::service::error::ServiceError;

type Resp<T> = Result<Response<T>, Status>;

fn handle(service: &EngineService) -> Result<MessagingHandle, Status> {
	service
		.shared
		.messaging
		.clone()
		.ok_or_else(|| ServiceError::Unimplemented("session messaging").into())
}

pub(super) fn stopped(_: crate::messaging::Stopped) -> Status {
	ServiceError::Stopped.into()
}

/// Capability check first, so a client without `messages` learns nothing else.
fn allowed<T>(service: &EngineService, request: &Request<T>) -> Result<ClientId, Status> {
	let caller = service.caller(request)?;
	service.shared.book.require(caller, Capability::Messages)?;
	Ok(caller)
}

pub(super) fn reader<T>(
	service: &EngineService,
	request: &Request<T>,
	context: impl FnOnce(&T) -> Option<&ClientContext>,
) -> Result<(MessagingHandle, ClientId), Status> {
	let caller = allowed(service, request)?;
	check_context(caller, context(request.get_ref()), &service.incarnation())?;
	Ok((handle(service)?, caller))
}

fn writer<T: Message>(
	service: &EngineService,
	request: &Request<T>,
	context: impl FnOnce(&T) -> Option<&MutationContext>,
) -> Result<(MessagingHandle, Caller), Status> {
	let caller = allowed(service, request)?;
	let body = request.get_ref();
	let mutation = check_mutation(caller, context(body), &service.incarnation())?;
	let who = Caller {
		client: caller.to_bytes(),
		operation: mutation.operation.to_bytes(),
		print: *blake3::hash(&body.encode_to_vec()).as_bytes(),
	};
	Ok((handle(service)?, who))
}

/// A reused operation id is an error status, like every other mutation.
fn answer(result: SessionOpResult) -> Resp<SessionOpResult> {
	let reused = result
		.operation
		.as_ref()
		.and_then(|o| o.problem.as_ref())
		.is_some_and(|p| p.code == "OPERATION_ID_REUSED");
	if reused {
		return Err(ServiceError::OperationReused.into());
	}
	Ok(Response::new(result))
}

fn refusal(code: &'static str, message: &str) -> Refusal {
	Refusal::new(code, message)
}

fn source_refusal(error: &SourceError) -> Refusal {
	match error {
		SourceError::TooLarge => {
			refusal("FILE_TOO_LARGE", "the file is larger than the offer limit")
		}
		SourceError::Changed => refusal("FILE_CHANGED", "the file changed while it was read"),
		other => refusal("FILE_REFUSED", &other.to_string()),
	}
}

pub(super) async fn list_sessions(
	service: &EngineService,
	request: Request<ClientContext>,
) -> Resp<ListSessionsResponse> {
	let (hub, _) = reader(service, &request, |b| Some(b))?;
	Ok(Response::new(hub.ask(|h| h.list()).await.map_err(stopped)?))
}

pub(super) async fn get_transcript(
	service: &EngineService,
	request: Request<GetTranscriptRequest>,
) -> Resp<Transcript> {
	let (hub, _) = reader(service, &request, |b| b.client.as_ref())?;
	let b = request.into_inner();
	let page = hub
		.ask(move |h| h.transcript(&b.session_id, b.from_ordinal, b.limit))
		.await
		.map_err(stopped)?;
	page.map(Response::new)
		.map_err(|r| Status::not_found(r.code))
}

pub(super) async fn search_session(
	service: &EngineService,
	request: Request<SearchSessionRequest>,
) -> Resp<SearchSessionResponse> {
	let (hub, _) = reader(service, &request, |b| b.client.as_ref())?;
	let b = request.into_inner();
	let hits = hub
		.ask(move |h| h.search(&b.session_id, &b.query))
		.await
		.map_err(stopped)?;
	hits.map(Response::new).map_err(|r| match r.code {
		"SESSION_NOT_FOUND" => Status::not_found(r.code),
		_ => Status::invalid_argument(r.code),
	})
}

pub(super) async fn send_message(
	service: &EngineService,
	request: Request<SendSessionMessageRequest>,
) -> Resp<SessionOpResult> {
	let (hub, who) = writer(service, &request, |b| b.context.as_ref())?;
	let b = request.into_inner();
	answer(
		hub.ask(move |h| h.send_text(who, &b.session_id, &b.text))
			.await
			.map_err(stopped)?,
	)
}

pub(super) async fn send_image(
	service: &EngineService,
	request: Request<SendSessionImageRequest>,
) -> Resp<SessionOpResult> {
	let (hub, who) = writer(service, &request, |b| b.context.as_ref())?;
	let b = request.into_inner();
	answer(
		hub.ask(move |h| h.send_image(who, &b.session_id, b.image))
			.await
			.map_err(stopped)?,
	)
}

pub(super) async fn retry_message(
	service: &EngineService,
	request: Request<RetrySessionMessageRequest>,
) -> Resp<SessionOpResult> {
	let (hub, who) = writer(service, &request, |b| b.context.as_ref())?;
	let b = request.into_inner();
	answer(
		hub.ask(move |h| h.retry(who, &b.session_id, b.seq))
			.await
			.map_err(stopped)?,
	)
}

/// The daemon opens the path itself, on a blocking worker: hashing a large file never
/// touches the actor or the async threads.
pub(super) async fn offer_file(
	service: &EngineService,
	request: Request<OfferFileRequest>,
) -> Resp<SessionOpResult> {
	let (hub, who) = writer(service, &request, |b| b.context.as_ref())?;
	let b = request.into_inner();
	let session = b.session_id.clone();
	let begun = hub
		.ask(move |h| h.begin_offer(&who, &session))
		.await
		.map_err(stopped)?;
	let max = match begun {
		Ok(max) => max,
		Err(done) => return answer(*done),
	};
	let path = b.path;
	let prepared = tokio::task::spawn_blocking(move || prepare(&path, max))
		.await
		.map_err(|_| Status::internal("hashing task failed"))?;
	let result = match prepared {
		Ok(file) => {
			hub.ask(move |h| h.offer_prepared(who, &b.session_id, file))
				.await
		}
		Err(error) => {
			hub.ask(move |h| h.refused(&who, source_refusal(&error)))
				.await
		}
	};
	answer(result.map_err(stopped)?)
}

pub(super) async fn respond_offer(
	service: &EngineService,
	request: Request<RespondOfferRequest>,
) -> Resp<SessionOpResult> {
	let (hub, who) = writer(service, &request, |b| b.context.as_ref())?;
	let b = request.into_inner();
	answer(
		hub.ask(move |h| {
			h.respond(
				who,
				&b.session_id,
				&b.offer_id,
				b.accept,
				&b.destination_dir,
				&b.file_name,
			)
		})
		.await
		.map_err(stopped)?,
	)
}

pub(super) async fn request_preview(
	service: &EngineService,
	request: Request<OfferRequest>,
) -> Resp<SessionOpResult> {
	let (hub, who) = writer(service, &request, |b| b.context.as_ref())?;
	let b = request.into_inner();
	answer(
		hub.ask(move |h| h.request_preview(who, &b.session_id, &b.offer_id))
			.await
			.map_err(stopped)?,
	)
}

/// Approving reads the first lines of the file on a blocking worker; the actor only sends
/// the reply.
pub(super) async fn approve_preview(
	service: &EngineService,
	request: Request<ApprovePreviewRequest>,
) -> Resp<SessionOpResult> {
	let (hub, who) = writer(service, &request, |b| b.context.as_ref())?;
	let b = request.into_inner();
	let (session, offer) = (b.session_id.clone(), b.offer_id.clone());
	let text = if b.approve {
		let source = hub
			.ask(move |h| h.preview_source(&session, &offer))
			.await
			.map_err(stopped)?;
		match source {
			Ok(path) => {
				let read = tokio::task::spawn_blocking(move || preview_text(&path))
					.await
					.map_err(|_| Status::internal("preview task failed"))?;
				read.map(Some)
					.map_err(|_| refusal("PREVIEW_UNAVAILABLE", "the file has no text preview"))
			}
			Err(r) => Err(r),
		}
	} else {
		Ok(None)
	};
	answer(
		hub.ask(move |h| h.answer_preview(who, &b.session_id, &b.offer_id, text))
			.await
			.map_err(stopped)?,
	)
}

pub(super) async fn cancel_transfer(
	service: &EngineService,
	request: Request<OfferRequest>,
) -> Resp<SessionOpResult> {
	let (hub, who) = writer(service, &request, |b| b.context.as_ref())?;
	let b = request.into_inner();
	answer(
		hub.ask(move |h| h.cancel_transfer(who, &b.session_id, &b.offer_id))
			.await
			.map_err(stopped)?,
	)
}

pub(super) async fn set_permissions(
	service: &EngineService,
	request: Request<SetSessionPermissionsRequest>,
) -> Resp<SessionOpResult> {
	let (hub, who) = writer(service, &request, |b| b.context.as_ref())?;
	let b = request.into_inner();
	answer(
		hub.ask(move |h| h.set_permissions(who, &b.session_id, b.allow_messages, b.allow_files))
			.await
			.map_err(stopped)?,
	)
}

pub(super) async fn pin_message(
	service: &EngineService,
	request: Request<PinMessageRequest>,
) -> Resp<SessionOpResult> {
	let (hub, who) = writer(service, &request, |b| b.context.as_ref())?;
	let b = request.into_inner();
	answer(
		hub.ask(move |h| h.pin(who, &b.session_id, b.outgoing, b.seq, b.pinned))
			.await
			.map_err(stopped)?,
	)
}

pub(super) async fn clear_session(
	service: &EngineService,
	request: Request<ClearSessionRequest>,
) -> Resp<SessionOpResult> {
	let (hub, who) = writer(service, &request, |b| b.context.as_ref())?;
	let b = request.into_inner();
	answer(
		hub.ask(move |h| h.clear(who, &b.session_id))
			.await
			.map_err(stopped)?,
	)
}

pub(super) async fn set_settings(
	service: &EngineService,
	request: Request<SetMessagingSettingsRequest>,
) -> Resp<SessionOpResult> {
	let (hub, who) = writer(service, &request, |b| b.context.as_ref())?;
	let b = request.into_inner();
	answer(
		hub.ask(move |h| {
			h.set_settings(
				who,
				b.enabled,
				b.max_offer_size_mib,
				b.transfer_rate_limit_mbps,
			)
		})
		.await
		.map_err(stopped)?,
	)
}

/// The export document. It is returned to the caller and written nowhere, unless the
/// caller names a destination: then the daemon creates that new file (owner-only, never
/// replacing anything) and returns its path instead.
pub(super) async fn export_session(
	service: &EngineService,
	request: Request<ExportSessionRequest>,
) -> Resp<ExportSessionResponse> {
	let (hub, who) = writer(service, &request, |b| b.context.as_ref())?;
	let b = request.into_inner();
	let (session, embed) = (b.session_id, b.embed_images);
	let json = hub
		.ask(move |h| h.export(&session, embed))
		.await
		.map_err(stopped)?;
	let operation = |state, problem: Option<Refusal>| relay_api::pb::Operation {
		operation_id: who.operation.to_vec(),
		state: state as i32,
		revision: 0,
		call_id: Vec::new(),
		problem: problem.map(|r| r.problem()),
	};
	let failed = |r: Refusal| {
		Response::new(ExportSessionResponse {
			operation: Some(operation(OperationState::Failed, Some(r))),
			..ExportSessionResponse::default()
		})
	};
	let json = match json {
		Ok(json) => json,
		Err(r) => return Ok(failed(r)),
	};
	if b.destination_path.is_empty() {
		if json.len() > EXPORT_INLINE_MAX {
			return Ok(failed(refusal(
				"EXPORT_TOO_LARGE",
				"too large to return inline; give a destination path",
			)));
		}
		return Ok(Response::new(ExportSessionResponse {
			operation: Some(operation(OperationState::Applied, None)),
			json: json.into_bytes(),
			written_path: String::new(),
		}));
	}
	let path = std::path::PathBuf::from(&b.destination_path);
	let written = tokio::task::spawn_blocking(move || {
		if !path.is_absolute() {
			return Err(refusal(
				"EXPORT_WRITE_FAILED",
				"the destination must be an absolute path",
			));
		}
		let mut file = relay_io::fs::create_private_new(&path)
			.map_err(|_| refusal("EXPORT_WRITE_FAILED", "the export could not be created"))?;
		file.write_all(json.as_bytes())
			.and_then(|()| file.sync_all())
			.map_err(|_| refusal("EXPORT_WRITE_FAILED", "the export could not be written"))?;
		Ok(path)
	})
	.await
	.map_err(|_| Status::internal("export task failed"))?;
	Ok(match written {
		Ok(path) => Response::new(ExportSessionResponse {
			operation: Some(operation(OperationState::Applied, None)),
			json: Vec::new(),
			written_path: path.to_string_lossy().into_owned(),
		}),
		Err(r) => failed(r),
	})
}
