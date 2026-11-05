//! Mutations executed by adapters, with per-client idempotency.

use prost::Message;
use relay_api::pb::{
	DecidePeerTrustRequest, MutationContext, Operation, SelectDevicesRequest,
	SetGlobalControlsRequest, SetNoiseSuppressionRequest, SetProfileRequest, StartMicTestRequest,
	TrustState, UploadAvatarRequest,
};
use relay_core::{Command, SettingsRevision};
use tonic::{Response, Status};

use super::EngineService;
use super::commands::gain;
use crate::service::convert::{Mutation, peer_id};
use crate::service::error::ServiceError;
use crate::service::handle::ServiceOp;
use crate::service::ports::{PeerTrust, TrustRequest};

const MAX_NAME_CHARS: usize = 64;
const MAX_DEVICE_ID_BYTES: usize = 512;
const MAX_AVATAR_BYTES: usize = 256 * 1024;
const MAX_MIC_GAIN: f32 = 4.0;

fn invalid(message: &str) -> ServiceError {
	ServiceError::InvalidArgument(message.to_owned())
}

async fn service_op(
	service: &EngineService,
	mutation: &Mutation,
	body: &impl Message,
	op: ServiceOp,
) -> Result<Response<Operation>, Status> {
	let print = *blake3::hash(&body.encode_to_vec()).as_bytes();
	let operation = service
		.shared
		.handle
		.service_op(mutation.client, mutation.operation, print, op)
		.await?;
	Ok(Response::new(operation))
}

pub(super) async fn global_controls(
	service: &EngineService,
	mutation: &Mutation,
	body: &SetGlobalControlsRequest,
) -> Result<Response<Operation>, Status> {
	let command = Command::SetGlobalControls {
		muted: body.muted,
		deafened: body.deafened,
		output_gain: body.playback_gain.map(|g| gain(g, 2.0)).transpose()?,
		expected: mutation.expected_revision.map(SettingsRevision::new),
	};
	let microphone = body
		.microphone_gain
		.map(|g| {
			if g.is_finite() && (0.0..=MAX_MIC_GAIN).contains(&g) {
				Ok(g)
			} else {
				Err(invalid("microphone gain out of range"))
			}
		})
		.transpose()?;
	let domain = service.run(mutation, command, None).await?;
	match microphone {
		Some(gain) if domain.get_ref().problem.is_none() => {
			service_op(service, mutation, body, ServiceOp::MicrophoneGain(gain)).await
		}
		_ => Ok(domain),
	}
}

fn bounded_id(id: &str) -> Result<(), ServiceError> {
	if id.len() > MAX_DEVICE_ID_BYTES || id.chars().any(char::is_control) {
		return Err(invalid("device id"));
	}
	Ok(())
}

pub(super) async fn select_devices(
	service: &EngineService,
	mutation: &Mutation,
	body: &SelectDevicesRequest,
) -> Result<Response<Operation>, Status> {
	bounded_id(&body.input_device_id)?;
	bounded_id(&body.output_device_id)?;
	let op = ServiceOp::SelectDevices {
		input: body.input_device_id.clone(),
		output: body.output_device_id.clone(),
	};
	service_op(service, mutation, body, op).await
}

pub(super) async fn noise_suppression(
	service: &EngineService,
	mutation: &Mutation,
	body: &SetNoiseSuppressionRequest,
) -> Result<Response<Operation>, Status> {
	bounded_id(&body.qualified_backend_id)?;
	let op = ServiceOp::NoiseSuppression(body.qualified_backend_id.clone());
	service_op(service, mutation, body, op).await
}

pub(super) async fn start_mic_test(
	service: &EngineService,
	mutation: &Mutation,
	body: &StartMicTestRequest,
) -> Result<Response<Operation>, Status> {
	if body.local_playback && !body.confirmed_feedback_warning {
		return Err(invalid("local playback needs the feedback warning confirmed").into());
	}
	let op = ServiceOp::StartMicTest {
		local_playback: body.local_playback,
	};
	service_op(service, mutation, body, op).await
}

pub(super) async fn stop_mic_test(
	service: &EngineService,
	mutation: &Mutation,
	body: &MutationContext,
) -> Result<Response<Operation>, Status> {
	service_op(service, mutation, body, ServiceOp::StopMicTest).await
}

pub(super) async fn profile(
	service: &EngineService,
	mutation: &Mutation,
	body: &SetProfileRequest,
) -> Result<Response<Operation>, Status> {
	let name = body.display_name.trim();
	if name.is_empty()
		|| name.chars().count() > MAX_NAME_CHARS
		|| name.chars().any(char::is_control)
	{
		return Err(invalid("display name must be 1 to 64 printable characters").into());
	}
	service_op(
		service,
		mutation,
		body,
		ServiceOp::Profile(relay_net::discovery::sanitize_display_name(name)),
	)
	.await
}

pub(super) async fn avatar(
	service: &EngineService,
	mutation: &Mutation,
	body: &UploadAvatarRequest,
) -> Result<Response<Operation>, Status> {
	if body.encoded_image.is_empty() || body.encoded_image.len() > MAX_AVATAR_BYTES {
		return Err(invalid("avatar must be 1 byte to 256 KiB").into());
	}
	let op = ServiceOp::Avatar(body.encoded_image.clone());
	service_op(service, mutation, body, op).await
}

pub(super) async fn trust(
	service: &EngineService,
	mutation: &Mutation,
	body: &DecidePeerTrustRequest,
) -> Result<Response<Operation>, Status> {
	let requested = match TrustState::try_from(body.requested_trust) {
		Ok(TrustState::TofuUnverified) => PeerTrust::TofuUnverified,
		Ok(TrustState::Verified) => PeerTrust::Verified,
		Ok(TrustState::Blocked) => PeerTrust::Blocked,
		_ => return Err(invalid("requested trust cannot be set by a client").into()),
	};
	if requested == PeerTrust::Verified && !body.out_of_band_verified {
		return Err(invalid("verification needs an out-of-band comparison").into());
	}
	let request = TrustRequest {
		peer: peer_id(&body.peer_id)?,
		expected_key: peer_id(&body.expected_public_key_sha256)?,
		requested,
		out_of_band_verified: body.out_of_band_verified,
	};
	service_op(service, mutation, body, ServiceOp::Trust(request)).await
}
