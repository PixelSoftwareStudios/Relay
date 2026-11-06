//! Favorites on [`RelayClient`].

use crate::pb::{Operation, SetFavoritesRequest};
use crate::{ApiError, RelayClient};

impl RelayClient {
	/// Replace the favorites (the quick-call grid) by this ordered list of trusted peers, each
	/// a 32-byte peer id. More than 6, a repeated peer or one that is not trusted fails the
	/// operation with a reason and changes nothing.
	pub async fn set_favorites(&mut self, peer_ids: &[Vec<u8>]) -> Result<Operation, ApiError> {
		let request = SetFavoritesRequest {
			context: Some(self.new_mutation()?),
			peer_ids: peer_ids.to_vec(),
		};
		Ok(self.inner.set_favorites(request).await?.into_inner())
	}
}
