//! Avatar removal, trust forgetting and generic settings helpers on [`RelayClient`].

use std::collections::BTreeMap;

use crate::pb::{ForgetPeerRequest, Operation, SetSettingsRequest, SetSettingsResponse};
use crate::{ApiError, RelayClient};

impl RelayClient {
	/// Remove the local avatar; trusted peers stop showing it.
	pub async fn clear_avatar(&mut self) -> Result<Operation, ApiError> {
		let context = self.new_mutation()?;
		Ok(self.inner.clear_avatar(context).await?.into_inner())
	}

	/// Forget a stored trust decision. `peer_id` (32 bytes) wins when given; otherwise `query`
	/// is a hex fingerprint prefix (at least 8 digits) or an exact display name. An ambiguous
	/// or unknown query fails with `PEER_AMBIGUOUS` or `PEER_UNKNOWN`.
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
}
