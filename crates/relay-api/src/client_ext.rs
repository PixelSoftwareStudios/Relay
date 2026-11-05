//! Channel, avatar, privacy and connection-info helpers on [`RelayClient`].

use uuid::Uuid;

use crate::pb::{
	AdvertMode, AvatarImage, ChannelInfo, ChannelPolicy, ConnectionInfo, CreateChannelRequest,
	CreateInviteRequest, CreateInviteResponse, GetAvatarRequest, InboundPolicy, JoinChannelRequest,
	LeaveChannelRequest, ListChannelsRequest, MutationContext, Operation,
	RevokeChannelMemberRequest, SetPrivacyRequest, StartCallRequest, start_call_request::Target,
};
use crate::{AVATAR_MAX_BYTES, ApiError, RelayClient};

impl RelayClient {
	/// Ask the engine to stop gracefully (ends calls, releases the microphone,
	/// removes the endpoint). Needs the `admin` capability, which the client
	/// must have named when it registered. Repeating it is harmless.
	pub async fn shutdown(&mut self) -> Result<Operation, ApiError> {
		let context = self.new_mutation()?;
		Ok(self.inner.shutdown(context).await?.into_inner())
	}

	/// Mutation context with a fresh random operation id. Reuse the returned id
	/// when retrying the same command.
	pub fn new_mutation(&self) -> Result<MutationContext, ApiError> {
		Ok(MutationContext {
			client: Some(self.require_context()?),
			operation_id: Uuid::new_v4().as_bytes().to_vec(),
			expected_revision: None,
		})
	}

	/// Create a channel. `password` is sent only in this request body.
	pub async fn create_channel(
		&mut self,
		name: &str,
		policy: ChannelPolicy,
		password: &str,
	) -> Result<Operation, ApiError> {
		let request = CreateChannelRequest {
			context: Some(self.new_mutation()?),
			name: name.to_owned(),
			policy: Some(policy),
			password: password.to_owned(),
		};
		Ok(self.inner.create_channel(request).await?.into_inner())
	}

	/// Join by channel id or by invite token (either may be empty).
	pub async fn join_channel(
		&mut self,
		channel_id: &[u8],
		password: &str,
		invite: &str,
	) -> Result<Operation, ApiError> {
		let request = JoinChannelRequest {
			context: Some(self.new_mutation()?),
			channel_id: channel_id.to_vec(),
			name: String::new(),
			password: password.to_owned(),
			invite: invite.to_owned(),
		};
		Ok(self.inner.join_channel(request).await?.into_inner())
	}

	/// Leave a channel and delete its local secrets.
	pub async fn leave_channel(&mut self, channel_id: &[u8]) -> Result<Operation, ApiError> {
		let request = LeaveChannelRequest {
			context: Some(self.new_mutation()?),
			channel_id: channel_id.to_vec(),
		};
		Ok(self.inner.leave_channel(request).await?.into_inner())
	}

	/// Channels this device belongs to.
	pub async fn list_channels(&mut self) -> Result<Vec<ChannelInfo>, ApiError> {
		let request = ListChannelsRequest {
			client: Some(self.require_context()?),
		};
		Ok(self
			.inner
			.list_channels(request)
			.await?
			.into_inner()
			.channels)
	}

	/// Mint an invite token; show it once, it is a secret.
	pub async fn create_invite(
		&mut self,
		channel_id: &[u8],
		ttl_seconds: u32,
		uses: u32,
	) -> Result<CreateInviteResponse, ApiError> {
		let request = CreateInviteRequest {
			context: Some(self.new_mutation()?),
			channel_id: channel_id.to_vec(),
			ttl_seconds,
			uses,
		};
		Ok(self.inner.create_invite(request).await?.into_inner())
	}

	/// Revoke one member by identity fingerprint.
	pub async fn revoke_channel_member(
		&mut self,
		channel_id: &[u8],
		fingerprint: &[u8],
	) -> Result<Operation, ApiError> {
		let request = RevokeChannelMemberRequest {
			context: Some(self.new_mutation()?),
			channel_id: channel_id.to_vec(),
			fingerprint: fingerprint.to_vec(),
		};
		Ok(self
			.inner
			.revoke_channel_member(request)
			.await?
			.into_inner())
	}

	/// Change inbound policy and/or advertisement mode (`None` keeps the value).
	pub async fn set_privacy(
		&mut self,
		inbound_policy: Option<InboundPolicy>,
		advert_mode: Option<AdvertMode>,
	) -> Result<Operation, ApiError> {
		let request = SetPrivacyRequest {
			context: Some(self.new_mutation()?),
			inbound_policy: inbound_policy.map(|p| p as i32),
			advert_mode: advert_mode.map(|m| m as i32),
		};
		Ok(self.inner.set_privacy(request).await?.into_inner())
	}

	/// Fetch a local or cached peer avatar. Responses over the size cap are
	/// rejected even if the server sent them.
	pub async fn get_avatar(&mut self, sha256: &[u8]) -> Result<AvatarImage, ApiError> {
		let request = GetAvatarRequest {
			client: Some(self.require_context()?),
			avatar_sha256: sha256.to_vec(),
		};
		let image = self.inner.get_avatar(request).await?.into_inner();
		if image.image.len() > AVATAR_MAX_BYTES {
			return Err(ApiError::InvalidResponse("avatar exceeds size limit"));
		}
		Ok(image)
	}

	/// Addresses, port, fingerprint and connection string to show or copy.
	pub async fn connection_info(&mut self) -> Result<ConnectionInfo, ApiError> {
		let ctx = self.require_context()?;
		Ok(self.inner.get_connection_info(ctx).await?.into_inner())
	}

	/// Start a call to a pasted connection string.
	pub async fn start_call_to_connection_string(
		&mut self,
		connection_string: &str,
	) -> Result<Operation, ApiError> {
		let request = StartCallRequest {
			context: Some(self.new_mutation()?),
			target: Some(Target::ManualConnectionString(connection_string.to_owned())),
			allow_parallel_same_peer: false,
		};
		Ok(self.inner.start_call(request).await?.into_inner())
	}
}
