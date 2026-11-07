//! `SetPeerAutoAnswer`: switch auto-answer for one contact.

use relay_api::pb::{Operation, SetPeerAutoAnswerRequest};
use tonic::{Response, Status};

use super::EngineService;
use super::ops::service_op;
use crate::service::convert::{Mutation, peer_id};
use crate::service::handle::ServiceOp;

pub(super) async fn set(
	service: &EngineService,
	mutation: &Mutation,
	body: &SetPeerAutoAnswerRequest,
) -> Result<Response<Operation>, Status> {
	let op = ServiceOp::SetAutoAnswer {
		peer: peer_id(&body.peer_id)?,
		enabled: body.enabled,
	};
	service_op(service, mutation, body, op).await
}
