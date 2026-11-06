//! Channels, privacy, avatar and connection-info operations.
//!
//! These go straight to the generated RPCs: they are specific to this app's
//! commands and not part of the wire vocabulary shared with other clients.

use std::collections::BTreeMap;

use relay_api::ApiError;
use relay_api::pb::{
	AdvertMode, CallHistoryPage, ChannelInfo, ChannelPolicy, ConnectionInfo, CreateChannelRequest,
	CreateInviteRequest, CreateInviteResponse, InboundPolicy, JoinChannelRequest,
	LeaveChannelRequest, ListChannelsRequest, Operation, RevokeChannelMemberRequest,
	SetPrivacyRequest, SettingResult, StartCallRequest, start_call_request,
};
use relay_client::operation::settle;
use relay_client::wire::mutation;

use super::real::{POLL_INTERVAL, RealClient};
use crate::secret::Secret;

/// Which channel a join refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelRef {
	/// A 16-byte channel id.
	Id(Vec<u8>),
	/// A listed channel name.
	Name(String),
}

/// What a joiner proves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
	/// An open channel needs nothing.
	None,
	/// The channel password.
	Password(Secret),
	/// An invite token.
	Invite(Secret),
}

/// Channel, privacy, avatar and connection-info operations.
#[allow(async_fn_in_trait, reason = "only used with concrete generics")]
pub trait Manage {
	/// What a remote user needs to reach this device.
	async fn connection_info(&mut self) -> Result<ConnectionInfo, ApiError>;
	/// Start a call to a pasted `relay://` string.
	async fn start_call_string(&mut self, connection_string: &str) -> Result<Operation, ApiError>;
	/// Change who may ring this device and how it is advertised.
	async fn set_privacy(
		&mut self,
		inbound: Option<InboundPolicy>,
		advert: Option<AdvertMode>,
	) -> Result<Operation, ApiError>;
	/// Channels this device belongs to.
	async fn list_channels(&mut self) -> Result<Vec<ChannelInfo>, ApiError>;
	/// Create a channel owned by this device.
	async fn create_channel(
		&mut self,
		name: &str,
		policy: ChannelPolicy,
		password: Option<&Secret>,
	) -> Result<Operation, ApiError>;
	/// Join a channel.
	async fn join_channel(
		&mut self,
		target: ChannelRef,
		credential: Credential,
	) -> Result<Operation, ApiError>;
	/// Leave a channel.
	async fn leave_channel(&mut self, channel_id: &[u8]) -> Result<Operation, ApiError>;
	/// Mint an invite token.
	async fn create_invite(
		&mut self,
		channel_id: &[u8],
		ttl_seconds: u32,
		uses: u32,
	) -> Result<CreateInviteResponse, ApiError>;
	/// Revoke a member by identity fingerprint.
	async fn revoke_member(
		&mut self,
		channel_id: &[u8],
		fingerprint: &[u8],
	) -> Result<Operation, ApiError>;
	/// Upload the local avatar (encoded image bytes).
	async fn upload_avatar(
		&mut self,
		bytes: Vec<u8>,
		media_type: &str,
	) -> Result<Operation, ApiError>;
	/// Remove the stored avatar.
	async fn clear_avatar(&mut self) -> Result<Operation, ApiError>;
	/// Forget a stored trust decision by peer id or by `query` (fingerprint prefix or name).
	async fn forget_peer(&mut self, peer_id: &[u8], query: &str) -> Result<Operation, ApiError>;
	/// Replace the favorites by this ordered list of trusted peer ids.
	async fn set_favorites(&mut self, peer_ids: &[Vec<u8>]) -> Result<Operation, ApiError>;
	/// One page of the call history, newest first.
	async fn call_history(
		&mut self,
		limit: u32,
		before_id: Option<u64>,
	) -> Result<CallHistoryPage, ApiError>;
	/// Delete the whole call history.
	async fn clear_call_history(&mut self) -> Result<Operation, ApiError>;
	/// Every preference as flat `section.key` text.
	async fn get_settings(&mut self) -> Result<BTreeMap<String, String>, ApiError>;
	/// Change preferences by key; one result per key, sorted by key.
	async fn set_settings(
		&mut self,
		values: Vec<(String, String)>,
	) -> Result<Vec<SettingResult>, ApiError>;
	/// Bytes in use next to the limit that bounds them (keyed by the limit's preference key).
	async fn get_limit_usage(&mut self) -> Result<BTreeMap<String, u64>, ApiError> {
		Ok(BTreeMap::new())
	}
}

impl RealClient {
	async fn settled(&mut self, op: Operation) -> Result<Operation, ApiError> {
		let timeout = self.timeout();
		settle(self.client_mut(), op, POLL_INTERVAL, timeout).await
	}
}

impl Manage for RealClient {
	async fn connection_info(&mut self) -> Result<ConnectionInfo, ApiError> {
		let ctx = self.client_mut().context().ok_or(ApiError::NotRegistered)?;
		Ok(self
			.client_mut()
			.raw()
			.get_connection_info(ctx)
			.await?
			.into_inner())
	}

	async fn start_call_string(&mut self, connection_string: &str) -> Result<Operation, ApiError> {
		let request = StartCallRequest {
			context: Some(mutation(self.client_mut())?),
			target: Some(start_call_request::Target::ManualConnectionString(
				connection_string.to_owned(),
			)),
			allow_parallel_same_peer: false,
		};
		let op = self
			.client_mut()
			.raw()
			.start_call(request)
			.await?
			.into_inner();
		self.settled(op).await
	}

	async fn set_privacy(
		&mut self,
		inbound: Option<InboundPolicy>,
		advert: Option<AdvertMode>,
	) -> Result<Operation, ApiError> {
		let request = SetPrivacyRequest {
			context: Some(mutation(self.client_mut())?),
			inbound_policy: inbound.map(|p| p as i32),
			advert_mode: advert.map(|m| m as i32),
		};
		let op = self
			.client_mut()
			.raw()
			.set_privacy(request)
			.await?
			.into_inner();
		self.settled(op).await
	}

	async fn list_channels(&mut self) -> Result<Vec<ChannelInfo>, ApiError> {
		let client = self.client_mut().context().ok_or(ApiError::NotRegistered)?;
		let response = self
			.client_mut()
			.raw()
			.list_channels(ListChannelsRequest {
				client: Some(client),
			})
			.await?
			.into_inner();
		Ok(response.channels)
	}

	async fn create_channel(
		&mut self,
		name: &str,
		policy: ChannelPolicy,
		password: Option<&Secret>,
	) -> Result<Operation, ApiError> {
		let request = CreateChannelRequest {
			context: Some(mutation(self.client_mut())?),
			name: name.to_owned(),
			policy: Some(policy),
			password: password.map(|p| p.expose().to_owned()).unwrap_or_default(),
		};
		let op = self
			.client_mut()
			.raw()
			.create_channel(request)
			.await?
			.into_inner();
		self.settled(op).await
	}

	async fn join_channel(
		&mut self,
		target: ChannelRef,
		credential: Credential,
	) -> Result<Operation, ApiError> {
		let (channel_id, name) = match target {
			ChannelRef::Id(id) => (id, String::new()),
			ChannelRef::Name(name) => (Vec::new(), name),
		};
		let (password, invite) = match &credential {
			Credential::None => (String::new(), String::new()),
			Credential::Password(p) => (p.expose().to_owned(), String::new()),
			Credential::Invite(i) => (String::new(), i.expose().to_owned()),
		};
		let request = JoinChannelRequest {
			context: Some(mutation(self.client_mut())?),
			channel_id,
			name,
			password,
			invite,
		};
		let op = self
			.client_mut()
			.raw()
			.join_channel(request)
			.await?
			.into_inner();
		self.settled(op).await
	}

	async fn leave_channel(&mut self, channel_id: &[u8]) -> Result<Operation, ApiError> {
		let request = LeaveChannelRequest {
			context: Some(mutation(self.client_mut())?),
			channel_id: channel_id.to_vec(),
		};
		let op = self
			.client_mut()
			.raw()
			.leave_channel(request)
			.await?
			.into_inner();
		self.settled(op).await
	}

	async fn create_invite(
		&mut self,
		channel_id: &[u8],
		ttl_seconds: u32,
		uses: u32,
	) -> Result<CreateInviteResponse, ApiError> {
		let request = CreateInviteRequest {
			context: Some(mutation(self.client_mut())?),
			channel_id: channel_id.to_vec(),
			ttl_seconds,
			uses,
		};
		Ok(self
			.client_mut()
			.raw()
			.create_invite(request)
			.await?
			.into_inner())
	}

	async fn revoke_member(
		&mut self,
		channel_id: &[u8],
		fingerprint: &[u8],
	) -> Result<Operation, ApiError> {
		let request = RevokeChannelMemberRequest {
			context: Some(mutation(self.client_mut())?),
			channel_id: channel_id.to_vec(),
			fingerprint: fingerprint.to_vec(),
		};
		let op = self
			.client_mut()
			.raw()
			.revoke_channel_member(request)
			.await?
			.into_inner();
		self.settled(op).await
	}

	async fn upload_avatar(
		&mut self,
		bytes: Vec<u8>,
		media_type: &str,
	) -> Result<Operation, ApiError> {
		self.run_command(relay_client::Command::UploadAvatar {
			bytes,
			media_type: media_type.to_owned(),
		})
		.await
	}

	async fn clear_avatar(&mut self) -> Result<Operation, ApiError> {
		self.run_command(relay_client::Command::ClearAvatar).await
	}

	async fn forget_peer(&mut self, peer_id: &[u8], query: &str) -> Result<Operation, ApiError> {
		self.run_command(relay_client::Command::ForgetPeer {
			peer_id: peer_id.to_vec(),
			query: query.to_owned(),
		})
		.await
	}

	async fn set_favorites(&mut self, peer_ids: &[Vec<u8>]) -> Result<Operation, ApiError> {
		self.run_command(relay_client::Command::SetFavorites {
			peer_ids: peer_ids.to_vec(),
		})
		.await
	}

	async fn call_history(
		&mut self,
		limit: u32,
		before_id: Option<u64>,
	) -> Result<CallHistoryPage, ApiError> {
		self.client_mut().list_call_history(limit, before_id).await
	}

	async fn clear_call_history(&mut self) -> Result<Operation, ApiError> {
		self.run_command(relay_client::Command::ClearCallHistory)
			.await
	}

	async fn get_settings(&mut self) -> Result<BTreeMap<String, String>, ApiError> {
		self.client_mut().get_settings().await
	}

	async fn get_limit_usage(&mut self) -> Result<BTreeMap<String, u64>, ApiError> {
		self.client_mut().get_limit_usage().await
	}

	async fn set_settings(
		&mut self,
		values: Vec<(String, String)>,
	) -> Result<Vec<SettingResult>, ApiError> {
		Ok(self.client_mut().set_settings(values).await?.results)
	}
}
