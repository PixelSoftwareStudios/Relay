//! Avatar removal, trust forgetting and generic settings helpers on [`RelayClient`].

use std::collections::BTreeMap;

use crate::pb::{
	CallHistoryPage, ForgetPeerRequest, ListCallHistoryRequest, Operation, SetSettingsRequest,
	SetSettingsResponse,
};
use crate::{ApiError, RelayClient};

impl RelayClient {
	/// Remove the local avatar; trusted peers stop showing it.
	pub async fn clear_avatar(&mut self) -> Result<Operation, ApiError> {
		let context = self.new_mutation()?;
		Ok(self.inner.clear_avatar(context).await?.into_inner())
	}

	/// Forget a stored trust decision. `peer_id` (32 bytes) wins when given; otherwise `query`
	/// is a hex fingerprint prefix (at least 8 digits) or an exact display name. An ambiguous
	/// or unknown query fails with `PEER_AMBIGUOUS` or `NOT_FOUND`.
	pub async fn forget_peer(
		&mut self,
		peer_id: Option<&[u8]>,
		query: &str,
	) -> Result<Operation, ApiError> {
		let request = ForgetPeerRequest {
			context: Some(self.new_mutation()?),
			peer_id: peer_id.map(<[u8]>::to_vec).unwrap_or_default(),
			query: query.to_owned(),
		};
		Ok(self.inner.forget_peer(request).await?.into_inner())
	}

	/// Every preference as flat `section.key` text, for export.
	pub async fn get_settings(&mut self) -> Result<BTreeMap<String, String>, ApiError> {
		let context = self.require_context()?;
		let dump = self.inner.get_settings(context).await?.into_inner();
		Ok(dump.values.into_iter().collect())
	}

	/// Bytes in use next to the limit that bounds them, keyed by the limit's preference key
	/// (`limits.avatar_cache_mib`, `limits.messages_retained_mib`,
	/// `limits.messages_session_mib`). A limit whose usage the engine does not know has no entry.
	pub async fn get_limit_usage(&mut self) -> Result<BTreeMap<String, u64>, ApiError> {
		let context = self.require_context()?;
		let dump = self.inner.get_settings(context).await?.into_inner();
		Ok(dump.usage.into_iter().collect())
	}

	/// Change preferences while the daemon runs. The answer lists one result per key
	/// (applied live, saved for the next start, or rejected with a reason). A security
	/// sensitive key makes the whole call `PERMISSION_DENIED`.
	pub async fn set_settings<K, V>(
		&mut self,
		values: impl IntoIterator<Item = (K, V)>,
	) -> Result<SetSettingsResponse, ApiError>
	where
		K: Into<String>,
		V: Into<String>,
	{
		let request = SetSettingsRequest {
			context: Some(self.new_mutation()?),
			values: values
				.into_iter()
				.map(|(k, v)| (k.into(), v.into()))
				.collect(),
		};
		Ok(self.inner.set_settings(request).await?.into_inner())
	}

	/// One page of the call history, newest first. `limit` 0 takes the engine default;
	/// `before_id` continues after the last `id` of the previous page.
	pub async fn list_call_history(
		&mut self,
		limit: u32,
		before_id: Option<u64>,
	) -> Result<CallHistoryPage, ApiError> {
		let request = ListCallHistoryRequest {
			client: Some(self.require_context()?),
			limit,
			before_id,
		};
		Ok(self.inner.list_call_history(request).await?.into_inner())
	}

	/// Delete every call history entry on this device.
	pub async fn clear_call_history(&mut self) -> Result<Operation, ApiError> {
		let context = self.new_mutation()?;
		Ok(self.inner.clear_call_history(context).await?.into_inner())
	}
}
