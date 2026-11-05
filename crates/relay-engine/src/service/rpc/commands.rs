//! Wire requests turned into validated domain commands.

use relay_api::pb::{
	CallMutation, ReleaseControllerRequest, SetCallControlsRequest, SetTransmitRouteRequest,
	StartCallRequest, TransmitMode, start_call_request::Target,
};
use relay_core::{CallId, Command, ControllerLeaseId, Gain, Reject, TransmitRoute};
use tonic::Status;

use super::EngineService;
use crate::service::convert::{
	Mutation, call_id, lease_bytes, lease_from_bytes, peer_id, problem_of,
};
use crate::service::error::ServiceError;

fn invalid(message: &str) -> ServiceError {
	ServiceError::InvalidArgument(message.to_owned())
}

pub(super) async fn start_call(
	service: &EngineService,
	mutation: &Mutation,
	body: &StartCallRequest,
) -> Result<(Command, CallId), ServiceError> {
	let peer = match body
		.target
		.as_ref()
		.ok_or_else(|| invalid("missing call target"))?
	{
		Target::PeerId(bytes) => peer_id(bytes)?,
		Target::ManualLocalAddress(address) => {
			let peers = service.shared.peers.clone();
			let address = address.clone();
			tokio::task::spawn_blocking(move || peers.resolve_address(&address))
				.await
				.map_err(|_| ServiceError::Stopped)??
		}
	};
	let ids = &service.shared.ids;
	let call = ids.call_for(mutation.client, mutation.operation);
	let command = Command::StartCall {
		call,
		session: ids.session_for(mutation.client, mutation.operation),
		peer,
		allow_parallel_same_peer: body.allow_parallel_same_peer,
	};
	Ok((command, call))
}

pub(super) fn call_and_lease(
	body: &CallMutation,
) -> Result<(CallId, ControllerLeaseId), ServiceError> {
	Ok((
		call_id(&body.call_id)?,
		ControllerLeaseId::new(body.controller_generation),
	))
}

pub(super) fn release(body: &ReleaseControllerRequest) -> Result<Command, ServiceError> {
	let call = call_id(&body.call_id)?;
	let lease = ControllerLeaseId::new(body.controller_generation);
	if let Some(named) = lease_from_bytes(&body.lease_id)?
		&& named != lease
	{
		return Err(invalid("lease id does not match controller generation"));
	}
	debug_assert_eq!(lease_bytes(lease).len(), 16);
	Ok(Command::ReleaseController { call, lease })
}

pub(super) fn set_route(body: &SetTransmitRouteRequest) -> Result<(Command, CallId), ServiceError> {
	let call = call_id(&body.call_id)?;
	let lease = ControllerLeaseId::new(body.controller_generation);
	let mode = TransmitMode::try_from(body.mode).unwrap_or(TransmitMode::Unspecified);
	let route = match mode {
		TransmitMode::Unspecified => return Err(invalid("transmit mode is required")),
		TransmitMode::ReceiveOnly => TransmitRoute::RECEIVE_ONLY,
		TransmitMode::PushToTalk | TransmitMode::OpenMic => {
			if !body.explicit_microphone_consent {
				return Err(invalid("microphone consent must be explicit"));
			}
			match (mode, body.authorize_background_open_mic) {
				(TransmitMode::PushToTalk, false) => TransmitRoute::push_to_talk(),
				(TransmitMode::OpenMic, false) => TransmitRoute::open_mic(),
				(TransmitMode::OpenMic, true) => TransmitRoute::open_mic_background(),
				_ => return Err(invalid("background authority applies to open mic only")),
			}
		}
	};
	Ok((Command::SetTransmitRoute { call, lease, route }, call))
}

/// A finite gain within `0.0..=max`, as thousandths.
pub(super) fn gain(value: f32, max: f32) -> Result<Gain, ServiceError> {
	if !value.is_finite() || value < 0.0 {
		return Err(invalid("gain must be finite and not negative"));
	}
	let permille = (value.min(max) * 1000.0).round();
	Ok(Gain::from_permille(permille as u16))
}

pub(super) fn call_controls(
	body: &SetCallControlsRequest,
) -> Result<(Command, CallId), ServiceError> {
	let call = call_id(&body.call_id)?;
	let command = Command::SetCallControls {
		call,
		lease: ControllerLeaseId::new(body.controller_generation),
		muted: body.muted,
		deafened: body.deafened,
		gain: body.receive_gain.map(|g| gain(g, 2.0)).transpose()?,
	};
	Ok((command, call))
}

/// Rejections of calls that return something other than an `Operation`.
pub(super) fn reject_status(reject: &Reject) -> Status {
	let problem = problem_of(reject);
	match reject {
		Reject::UnknownCall(_) | Reject::UnknownClient => Status::not_found(problem.code),
		Reject::OperationIdReused => Status::already_exists(problem.code),
		Reject::CallCapReached | Reject::ClientLimit => Status::resource_exhausted(problem.code),
		_ => Status::failed_precondition(problem.code),
	}
}
