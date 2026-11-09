//! The one place [`Command`]s become gRPC calls on a registered
//! [`RelayClient`]: fresh idempotent mutation contexts, controller leases and
//! the telemetry stream.

use relay_api::pb::{
	AcquireControllerRequest, CallMutation, ConnectAndTrustRequest, CreateChannelRequest,
	DecidePeerTrustRequest, ForgetPeerRequest, JoinChannelRequest, LeaveChannelRequest,
	MutationContext, Operation, ReplacePinnedKeyRequest, RevokeChannelMemberRequest,
	SelectDevicesRequest, SetCallControlsRequest, SetClientQuietRequest, SetFavoritesRequest,
	SetGlobalControlsRequest, SetPeerAutoAnswerRequest, SetPrivacyRequest, SetProfileRequest,
	SetRingtoneRequest, SetSettingsRequest, SetTransmitModeRequest, StartCallRequest,
	StartMicTestRequest, TelemetryRequest, TelemetrySnapshot, UploadAvatarRequest,
	start_call_request,
};
use relay_api::{ApiError, Endpoint, RelayClient};
use tonic::Streaming;

use relay_api::pb::{
	ApprovePreviewRequest, ChannelPolicy, ClearSessionRequest, OfferFileRequest, OfferRequest,
	PinMessageRequest, RespondOfferRequest, RetrySessionMessageRequest, SendSessionImageRequest,
	SendSessionMessageRequest, SetMessagingSettingsRequest, SetSessionPermissionsRequest,
};

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
		operation_id: relay_io::entropy::random_bytes::<16>().to_vec(),
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

/// Open one call's live statistics (about one `CallStats` per second). Needs `observe`.
pub async fn open_call_stats(
	client: &mut RelayClient,
	call_id: &[u8],
) -> Result<Streaming<relay_api::pb::CallStats>, ApiError> {
	let request = relay_api::pb::WatchCallStatsRequest {
		client: Some(client.context().ok_or(ApiError::NotRegistered)?),
		call_id: call_id.to_vec(),
	};
	Ok(client.raw().watch_call_stats(request).await?.into_inner())
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
	if command.is_messaging() {
		return send_messaging(client, command).await;
	}
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
		} => {
			rpc.set_transmit_mode(SetTransmitModeRequest {
				context: Some(context),
				call_id,
				controller_generation: generation,
				mode: mode as i32,
				authorize_background_open_mic: false,
			})
			.await
		}
		Command::ConnectAndTrust { connection_string } => {
			rpc.connect_and_trust(ConnectAndTrustRequest {
				context: Some(context),
				connection_string,
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
			ptt_release_delay_ms,
		} => {
			rpc.set_global_controls(SetGlobalControlsRequest {
				context: Some(context),
				muted,
				deafened,
				microphone_gain,
				playback_gain,
				ptt_release_delay_ms,
				autostart_daemon: None,
				ringtone_enabled: None,
				ringtone_volume: None,
				ringback_enabled: None,
				call_sounds_enabled: None,
			})
			.await
		}
		Command::SetRingPreferences {
			ringtone_enabled,
			ringtone_volume,
			ringback_enabled,
			call_sounds_enabled,
		} => {
			rpc.set_global_controls(SetGlobalControlsRequest {
				context: Some(context),
				ringtone_enabled,
				ringtone_volume,
				ringback_enabled,
				call_sounds_enabled,
				..SetGlobalControlsRequest::default()
			})
			.await
		}
		Command::PreviewTone => rpc.preview_tone(context).await,
		Command::SetRingtone { file_name, data } => {
			rpc.set_ringtone(SetRingtoneRequest {
				context: Some(context),
				file_name,
				data,
				reset: false,
			})
			.await
		}
		Command::ResetRingtone => {
			rpc.set_ringtone(SetRingtoneRequest {
				context: Some(context),
				reset: true,
				..SetRingtoneRequest::default()
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
		Command::ClearAvatar => rpc.clear_avatar(context).await,
		Command::ForgetPeer { peer_id, query } => {
			rpc.forget_peer(ForgetPeerRequest {
				context: Some(context),
				peer_id,
				query,
			})
			.await
		}
		Command::ReplacePinnedKey {
			peer_id,
			fingerprint,
			verified,
		} => {
			rpc.replace_pinned_key(ReplacePinnedKeyRequest {
				context: Some(context),
				peer_id,
				expected_fingerprint: fingerprint,
				verified,
			})
			.await
		}
		Command::SetFavorites { peer_ids } => {
			rpc.set_favorites(SetFavoritesRequest {
				context: Some(context),
				peer_ids,
			})
			.await
		}
		Command::SetPeerAutoAnswer { peer_id, enabled } => {
			rpc.set_peer_auto_answer(SetPeerAutoAnswerRequest {
				context: Some(context),
				peer_id,
				enabled,
			})
			.await
		}
		Command::SetClientQuiet { quiet } => {
			rpc.set_client_quiet(SetClientQuietRequest {
				context: Some(context),
				quiet,
			})
			.await
		}
		Command::SetSettings { values } => {
			let request = SetSettingsRequest {
				context: Some(context),
				values: values.into_iter().collect(),
			};
			let answer = rpc.set_settings(request).await?.into_inner();
			return answer.operation.ok_or(ApiError::InvalidResponse(
				"settings answer without operation",
			));
		}
		Command::ClearCallHistory => rpc.clear_call_history(context).await,
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
			name,
			password,
			invite,
		} => {
			rpc.join_channel(JoinChannelRequest {
				context: Some(context),
				channel_id,
				name,
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
		_ => return Err(ApiError::InvalidResponse("not an engine command")),
	};
	Ok(response?.into_inner())
}

/// Messaging commands answer with a [`relay_api::pb::SessionOpResult`]; the operation inside
/// is final. Use the `RelayClient` helpers when the message reference or offer id is needed.
async fn send_messaging(client: &mut RelayClient, command: Command) -> Result<Operation, ApiError> {
	let context = mutation(client)?;
	let rpc = client.raw();
	let result = match command {
		Command::SendSessionMessage { session_id, text } => {
			rpc.send_session_message(SendSessionMessageRequest {
				context: Some(context),
				session_id,
				text,
			})
			.await
		}
		Command::SendSessionImage { session_id, image } => {
			rpc.send_session_image(SendSessionImageRequest {
				context: Some(context),
				session_id,
				image,
			})
			.await
		}
		Command::RetrySessionMessage { session_id, seq } => {
			rpc.retry_session_message(RetrySessionMessageRequest {
				context: Some(context),
				session_id,
				seq,
			})
			.await
		}
		Command::OfferFile { session_id, path } => {
			rpc.offer_file(OfferFileRequest {
				context: Some(context),
				session_id,
				path,
			})
			.await
		}
		Command::RespondOffer {
			session_id,
			offer_id,
			accept,
			destination_dir,
			file_name,
		} => {
			rpc.respond_offer(RespondOfferRequest {
				context: Some(context),
				session_id,
				offer_id,
				accept,
				destination_dir,
				file_name,
			})
			.await
		}
		Command::RequestPreview {
			session_id,
			offer_id,
		} => {
			rpc.request_preview(OfferRequest {
				context: Some(context),
				session_id,
				offer_id,
			})
			.await
		}
		Command::ApprovePreview {
			session_id,
			offer_id,
			approve,
		} => {
			rpc.approve_preview(ApprovePreviewRequest {
				context: Some(context),
				session_id,
				offer_id,
				approve,
			})
			.await
		}
		Command::CancelTransfer {
			session_id,
			offer_id,
		} => {
			rpc.cancel_transfer(OfferRequest {
				context: Some(context),
				session_id,
				offer_id,
			})
			.await
		}
		Command::SetSessionPermissions {
			session_id,
			allow_messages,
			allow_files,
		} => {
			rpc.set_session_permissions(SetSessionPermissionsRequest {
				context: Some(context),
				session_id,
				allow_messages,
				allow_files,
			})
			.await
		}
		Command::PinMessage {
			session_id,
			outgoing,
			seq,
			pinned,
		} => {
			rpc.pin_message(PinMessageRequest {
				context: Some(context),
				session_id,
				outgoing,
				seq,
				pinned,
			})
			.await
		}
		Command::ClearSession { session_id } => {
			rpc.clear_session(ClearSessionRequest {
				context: Some(context),
				session_id,
			})
			.await
		}
		Command::SetMessagingSettings {
			enabled,
			max_offer_size_mib,
			transfer_rate_limit_mbps,
		} => {
			rpc.set_messaging_settings(SetMessagingSettingsRequest {
				context: Some(context),
				enabled,
				max_offer_size_mib,
				transfer_rate_limit_mbps,
			})
			.await
		}
		_ => return Err(ApiError::InvalidResponse("not a messaging command")),
	};
	result?
		.into_inner()
		.operation
		.ok_or(ApiError::InvalidResponse("result without operation"))
}
