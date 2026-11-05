//! Session messaging RPCs (placeholder until the messaging actor is wired).

use relay_api::pb::*;
use tonic::{Request, Response, Status};

use super::EngineService;

type Resp<T> = Result<Response<T>, Status>;

fn unwired<T>() -> Resp<T> {
	Err(Status::unimplemented("session messaging is not wired"))
}

macro_rules! stubs {
	($($name:ident($req:ty) -> $resp:ty;)*) => {
		$(pub(super) async fn $name(_: &EngineService, _: Request<$req>) -> Resp<$resp> { unwired() })*
	};
}

stubs! {
	list_sessions(ClientContext) -> ListSessionsResponse;
	get_transcript(GetTranscriptRequest) -> Transcript;
	watch_session(WatchSessionRequest) -> super::Stream<SessionEnvelope>;
	send_message(SendSessionMessageRequest) -> SessionOpResult;
	send_image(SendSessionImageRequest) -> SessionOpResult;
	retry_message(RetrySessionMessageRequest) -> SessionOpResult;
	offer_file(OfferFileRequest) -> SessionOpResult;
	respond_offer(RespondOfferRequest) -> SessionOpResult;
	request_preview(OfferRequest) -> SessionOpResult;
	approve_preview(ApprovePreviewRequest) -> SessionOpResult;
	cancel_transfer(OfferRequest) -> SessionOpResult;
	set_permissions(SetSessionPermissionsRequest) -> SessionOpResult;
	pin_message(PinMessageRequest) -> SessionOpResult;
	search_session(SearchSessionRequest) -> SearchSessionResponse;
	clear_session(ClearSessionRequest) -> SessionOpResult;
	export_session(ExportSessionRequest) -> ExportSessionResponse;
	set_settings(SetMessagingSettingsRequest) -> SessionOpResult;
}
