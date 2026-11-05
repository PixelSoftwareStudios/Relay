//! The one place [`Command`]s become gRPC calls on a registered
//! [`RelayClient`]: fresh idempotent mutation contexts, controller leases and
//! the telemetry stream.

use relay_api::pb::{
	AcquireControllerRequest, CallMutation, CreateChannelRequest, DecidePeerTrustRequest,
	JoinChannelRequest, LeaveChannelRequest, MutationContext, Operation,
	RevokeChannelMemberRequest, SelectDevicesRequest, SetCallControlsRequest,
	SetGlobalControlsRequest, SetNoiseSuppressionRequest, SetPrivacyRequest, SetProfileRequest,
	SetTransmitRouteRequest, StartCallRequest, StartMicTestRequest, TelemetryRequest,
	TelemetrySnapshot, UploadAvatarRequest, start_call_request,
};
use relay_api::{ApiError, Endpoint, RelayClient};
use tonic::Streaming;

use relay_api::pb::ChannelPolicy;

use crate::command::{CallAction, CallTarget, ChannelOptions, Command};

/// Connect and register under `name` with `capabilities`.
pub async fn connect_registered(
	endpoint: &Endpoint,
	name: &str,
	version: &str,
	capabilities: &[&str],
) -> Result<RelayClient, ApiError> {
	let mut client = RelayClient::connect(endpoint).await?;
	client.register(name, version, capabilities).await?;
	Ok(client)
}

/// A fresh mutation context. The new operation id makes a retried request
/// idempotent on the engine side.
pub fn mutation(client: &RelayClient) -> Result<MutationContext, ApiError> {
	Ok(MutationContext {
		client: Some(client.context().ok_or(ApiError::NotRegistered)?),
		operation_id: uuid::Uuid::new_v4().as_bytes().to_vec(),
		expected_revision: None,
	})
}

/// Take (or keep) the controller lease of a call and return its generation.
/// `takeover` asks for an explicit takeover from another controller.
pub async fn acquire_generation(
	client: &mut RelayClient,
	call_id: &[u8],
	takeover: bool,
) -> Result<u64, ApiError> {
	let request = AcquireControllerRequest {
		context: Some(mutation(client)?),
		call_id: call_id.to_vec(),
		request_explicit_takeover: takeover,
	};
	let lease = client.raw().acquire_controller(request).await?.into_inner();
	Ok(lease.generation)
}

/// Open the telemetry stream (meters and statistics).
pub async fn open_telemetry(
	client: &mut RelayClient,
	meters_hz: u32,
	stats_hz: u32,
) -> Result<Streaming<TelemetrySnapshot>, ApiError> {
	let request = TelemetryRequest {
		client: Some(client.context().ok_or(ApiError::NotRegistered)?),
		call_ids: Vec::new(),
		meters_hz,
		stats_hz,
	};
	Ok(client.raw().watch_telemetry(request).await?.into_inner())
}

fn call_mutation(context: MutationContext, call_id: Vec<u8>, generation: u64) -> CallMutation {
	CallMutation {
		context: Some(context),
		call_id,
		controller_generation: generation,
	}
}

/// Send one command and return the operation as the engine first reports it
/// (possibly still `ACCEPTED`; see [`crate::operation::settle`]).
pub async fn send(client: &mut RelayClient, command: Command) -> Result<Operation, ApiError> {
	let context = mutation(client)?;
	let rpc = client.raw();
	let response = match command {
		Command::StartCall(target) => {
			let target = match target {
				CallTarget::Peer(id) => start_call_request::Target::PeerId(id),
				CallTarget::Address(addr) => start_call_request::Target::ManualLocalAddress(addr),
				CallTarget::ConnectionString(text) => {
					start_call_request::Target::ManualConnectionString(text)
				}
			};
			rpc.start_call(StartCallRequest {
				context: Some(context),
				target: Some(target),
				allow_parallel_same_peer: false,
			})
			.await
		}
		Command::CallAction {
			action,
			call_id,
			generation,
		} => {
			let request = call_mutation(context, call_id, generation);
			match action {
				CallAction::Accept => rpc.accept_call(request).await,
				CallAction::Decline => rpc.decline_call(request).await,
				CallAction::End => rpc.end_call(request).await,
			}
		}
		Command::SetTransmit {
			call_id,
			generation,
			mode,
			consent,
		} => {
			rpc.set_transmit_route(SetTransmitRouteRequest {
				context: Some(context),
				call_id,
				controller_generation: generation,
				mode: mode as i32,
				explicit_microphone_consent: consent,
				authorize_background_open_mic: false,
			})
			.await
		}
		Command::SetCallControls {
			call_id,
			generation,
			muted,
			deafened,
			receive_gain,
		} => {
			rpc.set_call_controls(SetCallControlsRequest {
				context: Some(context),
				call_id,
				controller_generation: generation,
				muted,
				deafened,
				receive_gain,
			})
			.await
		}
		Command::SetGlobalControls {
			muted,
			deafened,
			microphone_gain,
			playback_gain,
		} => {
			rpc.set_global_controls(SetGlobalControlsRequest {
				context: Some(context),
				muted,
				deafened,
				microphone_gain,
				playback_gain,
			})
			.await
		}
		Command::SelectDevices {
			input_id,
			output_id,
		} => {
			rpc.select_devices(SelectDevicesRequest {
				context: Some(context),
				input_device_id: input_id,
				output_device_id: output_id,
			})
			.await
		}
		Command::SetNoiseSuppression { backend } => {
			rpc.set_noise_suppression(SetNoiseSuppressionRequest {
				context: Some(context),
				qualified_backend_id: backend,
			})
			.await
		}
		Command::SetProfile { display_name } => {
			rpc.set_profile(SetProfileRequest {
				context: Some(context),
				display_name,
			})
			.await
		}
		Command::UploadAvatar { bytes, media_type } => {
			rpc.upload_avatar(UploadAvatarRequest {
				context: Some(context),
				encoded_image: bytes,
				declared_media_type: media_type,
			})
			.await
		}
		Command::StartMicTest {
			local_playback,
			confirmed_feedback,
		} => {
			rpc.start_mic_test(StartMicTestRequest {
				context: Some(context),
				local_playback,
				confirmed_feedback_warning: confirmed_feedback,
			})
			.await
		}
		Command::StopMicTest => rpc.stop_mic_test(context).await,
		Command::CreateChannel {
			name,
			options,
			password,
		} => {
			let ChannelOptions {
				listed,
				password: needs_password,
				invite_only,
			} = options;
			rpc.create_channel(CreateChannelRequest {
				context: Some(context),
				name,
				policy: Some(ChannelPolicy {
					listed,
					password: needs_password,
					invite_only,
				}),
				password: password.expose().to_owned(),
			})
			.await
		}
		Command::JoinChannel {
			channel_id,
			password,
			invite,
		} => {
			rpc.join_channel(JoinChannelRequest {
				context: Some(context),
				channel_id,
				name: String::new(),
				password: password.expose().to_owned(),
				invite: invite.expose().to_owned(),
			})
			.await
		}
		Command::LeaveChannel { channel_id } => {
			rpc.leave_channel(LeaveChannelRequest {
				context: Some(context),
				channel_id,
			})
			.await
		}
		Command::RevokeMember {
			channel_id,
			fingerprint,
		} => {
			rpc.revoke_channel_member(RevokeChannelMemberRequest {
				context: Some(context),
				channel_id,
				fingerprint,
			})
			.await
		}
		Command::SetPrivacy { inbound, advert } => {
			rpc.set_privacy(SetPrivacyRequest {
				context: Some(context),
				inbound_policy: inbound.map(|p| p as i32),
				advert_mode: advert.map(|m| m as i32),
			})
			.await
		}
		Command::DecideTrust {
			peer_id,
			public_key_sha256,
			requested,
			out_of_band_verified,
		} => {
			rpc.decide_peer_trust(DecidePeerTrustRequest {
				context: Some(context),
				peer_id,
				expected_public_key_sha256: public_key_sha256,
				requested_trust: requested as i32,
				out_of_band_verified,
			})
			.await
		}
	};
	Ok(response?.into_inner())
}
