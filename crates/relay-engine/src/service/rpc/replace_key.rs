//! `ReplacePinnedKey`: re-verifying a contact whose key changed.

use relay_api::pb::{Operation, ReplacePinnedKeyRequest};
use tonic::{Response, Status};

use super::EngineService;
use super::ops::service_op;
use crate::service::convert::{Mutation, peer_id};
use crate::service::error::ServiceError;
use crate::service::handle::ServiceOp;

/// More than a fingerprint with separators needs.
const MAX_FINGERPRINT_BYTES: usize = 128;

/// The engine decides whether the key change exists and whether the fingerprint matches;
/// this only checks the shape of the request.
pub(super) async fn replace(
	service: &EngineService,
	mutation: &Mutation,
	body: &ReplacePinnedKeyRequest,
) -> Result<Response<Operation>, Status> {
	let peer = peer_id(&body.peer_id)?;
	let fingerprint = body.expected_fingerprint.trim();
	if fingerprint.len() > MAX_FINGERPRINT_BYTES || fingerprint.chars().any(char::is_control) {
		return Err(ServiceError::InvalidArgument("enter the full fingerprint".to_owned()).into());
	}
	let op = ServiceOp::ReplaceKey {
		peer,
		fingerprint: fingerprint.to_owned(),
		verified: body.verified,
		via: None,
	};
	service_op(service, mutation, body, op).await
}
