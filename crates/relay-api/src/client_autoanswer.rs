//! Per-contact auto-answer on [`RelayClient`].

use crate::pb::{Operation, SetPeerAutoAnswerRequest};
use crate::{ApiError, RelayClient};

impl RelayClient {
	/// Switch auto-answer for one contact (a 32-byte peer id) on or off. Switching on fails the
	/// operation with a reason unless the contact is verified; switching off always works.
	pub async fn set_peer_auto_answer(
		&mut self,
		peer_id: &[u8],
		enabled: bool,
	) -> Result<Operation, ApiError> {
		let request = SetPeerAutoAnswerRequest {
			context: Some(self.new_mutation()?),
			peer_id: peer_id.to_vec(),
			enabled,
		};
		Ok(self.inner.set_peer_auto_answer(request).await?.into_inner())
	}
}

impl RelayClient {
	/// Tell the engine this client alerts incoming calls itself (`quiet`): its ringtone stays
	/// silent while every attached client is quiet. Lasts while this client holds a state watch.
	pub async fn set_client_quiet(&mut self, quiet: bool) -> Result<Operation, ApiError> {
		let request = crate::pb::SetClientQuietRequest {
			context: Some(self.new_mutation()?),
			quiet,
		};
		Ok(self.inner.set_client_quiet(request).await?.into_inner())
	}
}
