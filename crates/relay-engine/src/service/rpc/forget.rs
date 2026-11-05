//! `ForgetPeer` and `DecidePeerTrust(UNKNOWN)`: removing a stored trust decision.

use relay_api::pb::{DecidePeerTrustRequest, ForgetPeerRequest, Operation};
use tonic::{Response, Status};

use super::EngineService;
use super::ops::service_op;
use crate::service::convert::{Mutation, peer_id};
use crate::service::error::ServiceError;
use crate::service::handle::ServiceOp;
use crate::service::ports::PeerSelector;

const MAX_QUERY_BYTES: usize = 256;

fn invalid(what: &str) -> ServiceError {
	ServiceError::InvalidArgument(what.to_owned())
}

pub(super) async fn forget(
	service: &EngineService,
	mutation: &Mutation,
	body: &ForgetPeerRequest,
) -> Result<Response<Operation>, Status> {
	let selector = if body.peer_id.is_empty() {
		let query = body.query.trim();
		if query.is_empty() || query.len() > MAX_QUERY_BYTES || query.chars().any(char::is_control)
		{
			return Err(invalid("name a peer by id, fingerprint or name").into());
		}
		PeerSelector::Query(query.to_owned())
	} else {
		PeerSelector::Id(peer_id(&body.peer_id)?)
	};
	service_op(service, mutation, body, ServiceOp::ForgetPeer(selector)).await
}

/// `requested_trust = UNKNOWN`: forget the peer. The key hash may be left out (nobody has it
/// before a first connection), but if it is given it must be the peer's own.
pub(super) async fn forget_by_decision(
	service: &EngineService,
	mutation: &Mutation,
	body: &DecidePeerTrustRequest,
) -> Result<Response<Operation>, Status> {
	let peer = peer_id(&body.peer_id)?;
	if !body.expected_public_key_sha256.is_empty()
		&& body.expected_public_key_sha256 != body.peer_id
	{
		return Err(invalid("key does not match the peer").into());
	}
	service_op(
		service,
		mutation,
		body,
		ServiceOp::ForgetPeer(PeerSelector::Id(peer)),
	)
	.await
}
