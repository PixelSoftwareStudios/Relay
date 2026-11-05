//! Channel RPCs over the channel driver. Passwords and invites exist only in the
//! request bodies and in the invite answer; they are wiped after use and never
//! reach an event, a snapshot, an operation result or a log line.

use std::future::Future;

use prost::Message;
use relay_api::pb::{
	CreateChannelRequest, CreateInviteRequest, CreateInviteResponse, JoinChannelRequest,
	LeaveChannelRequest, ListChannelsResponse, Operation, OperationState,
	RevokeChannelMemberRequest,
};
use relay_core::channel::Policy;
use tonic::{Response, Status};
use zeroize::Zeroizing;

use super::EngineService;
use crate::channels::{ChannelHub, Failure, JoinRequest, Replay, Stored, StoredInvite};
use crate::service::convert::Mutation;
use crate::service::error::ServiceError;

fn hub(service: &EngineService) -> Result<&ChannelHub, Status> {
	service
		.shared
		.channels
		.as_ref()
		.ok_or_else(|| ServiceError::Unimplemented("channels").into())
}

/// Malformed requests and storage failures are API errors; the rest are results.
fn api_error(failure: &Failure) -> Status {
	match failure {
		Failure::Invalid(why) => ServiceError::InvalidArgument((*why).to_owned()).into(),
		_ => ServiceError::Precondition("channel storage is unavailable".to_owned()).into(),
	}
}

fn operation(hub: &ChannelHub, mutation: &Mutation, failure: Option<&Failure>) -> Operation {
	let problem = failure.and_then(Failure::problem);
	Operation {
		operation_id: mutation.operation.to_bytes().to_vec(),
		state: if problem.is_some() {
			OperationState::Failed
		} else {
			OperationState::Applied
		} as i32,
		revision: hub.revision(),
		call_id: Vec::new(),
		problem,
	}
}

/// Runs `work` once per (client, operation id): a retry with the same request gets
/// the stored outcome, a reused id with another request is refused.
async fn once<T, Fut>(
	service: &EngineService,
	mutation: &Mutation,
	request: &impl Message,
	work: impl FnOnce(ChannelHub) -> Fut,
	invite: impl FnOnce(T) -> Option<StoredInvite>,
) -> Result<Stored, Status>
where
	Fut: Future<Output = Result<T, Failure>>,
{
	let hub = hub(service)?;
	let encoded = Zeroizing::new(request.encode_to_vec());
	let (replay, print) = hub.replay(mutation.client, mutation.operation, &encoded);
	drop(encoded);
	match replay {
		Replay::Same(stored) => return Ok(*stored),
		Replay::Conflict => return Err(ServiceError::OperationReused.into()),
		Replay::Fresh => {}
	}
	let outcome = work(hub.clone()).await;
	let stored = match outcome {
		Ok(done) => Stored {
			operation: operation(hub, mutation, None),
			invite: invite(done),
		},
		Err(failure) if failure.problem().is_some() => Stored {
			operation: operation(hub, mutation, Some(&failure)),
			invite: None,
		},
		Err(failure) => return Err(api_error(&failure)),
	};
	hub.remember(mutation.client, mutation.operation, print, stored.clone());
	Ok(stored)
}

pub(super) async fn create(
	service: &EngineService,
	mutation: &Mutation,
	mut body: CreateChannelRequest,
) -> Result<Response<Operation>, Status> {
	let policy = body.policy.unwrap_or_default();
	let policy = Policy {
		listed: policy.listed,
		password: policy.password,
		invite_only: policy.invite_only,
	};
	let name = body.name.clone();
	let password = Zeroizing::new(std::mem::take(&mut body.password));
	let stored = once(
		service,
		mutation,
		&body,
		|hub| async move { hub.create(&name, policy, password).await },
		|_| None,
	)
	.await?;
	Ok(Response::new(stored.operation))
}

pub(super) async fn join(
	service: &EngineService,
	mutation: &Mutation,
	mut body: JoinChannelRequest,
) -> Result<Response<Operation>, Status> {
	let request = JoinRequest {
		channel_id: body.channel_id.clone(),
		name: body.name.clone(),
		password: Zeroizing::new(std::mem::take(&mut body.password)),
		invite: Zeroizing::new(std::mem::take(&mut body.invite)),
	};
	// The secrets are already out of `body`; what remains identifies the request.
	let stored = once(
		service,
		mutation,
		&body,
		|hub| async move { hub.join(request).await },
		|_| None,
	)
	.await?;
	Ok(Response::new(stored.operation))
}

pub(super) async fn leave(
	service: &EngineService,
	mutation: &Mutation,
	body: LeaveChannelRequest,
) -> Result<Response<Operation>, Status> {
	let id = body.channel_id.clone();
	let stored = once(
		service,
		mutation,
		&body,
		|hub| async move { hub.leave(&id).await },
		|_| None,
	)
	.await?;
	Ok(Response::new(stored.operation))
}

pub(super) async fn revoke(
	service: &EngineService,
	mutation: &Mutation,
	body: RevokeChannelMemberRequest,
) -> Result<Response<Operation>, Status> {
	let (id, fingerprint) = (body.channel_id.clone(), body.fingerprint.clone());
	let stored = once(
		service,
		mutation,
		&body,
		|hub| async move { hub.revoke(&id, &fingerprint).await },
		|_| None,
	)
	.await?;
	Ok(Response::new(stored.operation))
}

/// A failed invite is a FAILED operation inside the response (not a gRPC error), so
/// a non-owner learns nothing beyond the uniform `CHANNEL_DENIED`.
pub(super) async fn invite(
	service: &EngineService,
	mutation: &Mutation,
	body: CreateInviteRequest,
) -> Result<Response<CreateInviteResponse>, Status> {
	let (id, ttl, uses) = (body.channel_id.clone(), body.ttl_seconds, body.uses);
	let stored = once(
		service,
		mutation,
		&body,
		|hub| async move { hub.invite(&id, ttl, uses).await },
		|(token, expires)| Some(StoredInvite::new(token, expires)),
	)
	.await?;
	let mut response = stored
		.invite
		.as_ref()
		.map(StoredInvite::response)
		.unwrap_or_default();
	response.operation = Some(stored.operation);
	Ok(Response::new(response))
}

pub(super) fn list(service: &EngineService) -> Result<Response<ListChannelsResponse>, Status> {
	Ok(Response::new(ListChannelsResponse {
		channels: hub(service)?.list(),
	}))
}
