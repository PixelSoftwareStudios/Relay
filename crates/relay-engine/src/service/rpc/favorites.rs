//! `SetFavorites`: the quick-call list, replaced as a whole.

use relay_api::pb::{Operation, SetFavoritesRequest};
use relay_config::favorites::MAX_FAVORITES;
use tonic::{Response, Status};

use super::EngineService;
use super::ops::service_op;
use crate::service::convert::{Mutation, peer_id};
use crate::service::error::ServiceError;
use crate::service::handle::ServiceOp;

pub(super) async fn set(
	service: &EngineService,
	mutation: &Mutation,
	body: &SetFavoritesRequest,
) -> Result<Response<Operation>, Status> {
	// A list far beyond the limit is malformed; a list just over it fails with the reason.
	if body.peer_ids.len() > MAX_FAVORITES * 4 {
		return Err(ServiceError::InvalidArgument("too many peers".to_owned()).into());
	}
	let ordered = body
		.peer_ids
		.iter()
		.map(|id| peer_id(id))
		.collect::<Result<Vec<_>, _>>()?;
	service_op(service, mutation, body, ServiceOp::SetFavorites(ordered)).await
}
