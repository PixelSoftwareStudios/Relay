//! `ConnectAndTrust`: a scanned QR code is an out-of-band exchange of the fingerprint.

use relay_api::pb::{ConnectAndTrustRequest, Operation, OperationState, Problem};
use tonic::{Response, Status};

use super::EngineService;
use super::ops::service_op;
use crate::service::ConnectFailure;
use crate::service::convert::Mutation;
use crate::service::convert::service_problem;
use crate::service::error::ServiceError;
use crate::service::handle::ServiceOp;
use crate::service::ports::{PeerTrust, PortError, TrustRequest};

/// Dials the string with its key pinned; on a match records the peer as verified through the
/// same path as `DecidePeerTrust(VERIFIED, out_of_band_verified)`. Any failure is a FAILED
/// operation with a stable code and stores nothing.
pub(super) async fn connect(
	service: &EngineService,
	mutation: &Mutation,
	body: &ConnectAndTrustRequest,
) -> Result<Response<Operation>, Status> {
	let peers = service.shared.peers.clone();
	let text = body.connection_string.clone();
	// A scanned code of a key-changed contact is the out-of-band check of its new key: the
	// contact is repaired, not trusted anew. Name or address alone never get here: only the
	// scanned key equal to the key the device was refused with does.
	let repair = |peers: std::sync::Arc<dyn crate::service::ports::PeerControl>, text: String| {
		tokio::task::spawn_blocking(move || peers.key_change_for(&text))
	};
	let mut changed = repair(peers.clone(), text.clone())
		.await
		.map_err(|_| ServiceError::Stopped)?;
	if changed.is_none() {
		let (dial_peers, dial_text) = (peers.clone(), text.clone());
		let dialled = tokio::task::spawn_blocking(move || dial_peers.connect_pinned(&dial_text))
			.await
			.map_err(|_| ServiceError::Stopped)?;
		match dialled {
			Ok(peer) => {
				let request = TrustRequest {
					peer,
					expected_key: peer,
					requested: PeerTrust::Verified,
					out_of_band_verified: true,
				};
				return service_op(service, mutation, body, ServiceOp::Trust(request)).await;
			}
			Err(failure) => {
				// The dial itself may have shown the key change (a contact whose new key was
				// not seen yet): then the same repair applies.
				changed = repair(peers, text.clone())
					.await
					.map_err(|_| ServiceError::Stopped)?;
				if changed.is_none() {
					return Ok(Response::new(failed(mutation, &failure)));
				}
			}
		}
	}
	let Some(peer) = changed else {
		return Err(ServiceError::Stopped.into());
	};
	let op = ServiceOp::ReplaceKey {
		peer,
		fingerprint: hex::encode(peer.as_bytes()),
		verified: true,
		via: Some(text),
	};
	service_op(service, mutation, body, op).await
}

fn failed(mutation: &Mutation, failure: &ConnectFailure) -> Operation {
	Operation {
		operation_id: mutation.operation.to_bytes().to_vec(),
		state: OperationState::Failed as i32,
		revision: 0,
		call_id: Vec::new(),
		problem: Some(Problem {
			code: failure.code().to_owned(),
			message: failure.to_string(),
			retryable: failure.retryable(),
			recovery_action: String::new(),
		}),
	}
}

/// A FAILED operation for an adapter refusal, with its stable code.
pub(super) fn failed_port(mutation: &Mutation, error: &PortError) -> Operation {
	Operation {
		operation_id: mutation.operation.to_bytes().to_vec(),
		state: OperationState::Failed as i32,
		revision: 0,
		call_id: Vec::new(),
		problem: Some(service_problem(error)),
	}
}
