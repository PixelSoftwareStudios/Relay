//! `ListCallHistory` and `ClearCallHistory`: the local list of past calls.

use relay_api::pb::{
	CallDirection, CallHistoryEntry, CallHistoryPage, CallOutcome as PbOutcome,
	ListCallHistoryRequest, Operation, OperationState,
};
use relay_config::history::{CallOutcome, CallRecord, DEFAULT_LIST, Direction, MAX_LIST};
use tonic::{Response, Status};

use super::EngineService;
use crate::service::convert::Mutation;
use crate::service::error::ServiceError;

fn entry(record: CallRecord) -> CallHistoryEntry {
	CallHistoryEntry {
		id: record.id,
		call_id: record.call_id.to_vec(),
		peer_name: record.peer_name,
		peer_id: hex::decode(&record.peer_fingerprint).unwrap_or_default(),
		peer_fingerprint_short: record.peer_fingerprint_short,
		direction: match record.direction {
			Direction::Incoming => CallDirection::Incoming,
			Direction::Outgoing => CallDirection::Outgoing,
		} as i32,
		outcome: match record.outcome {
			CallOutcome::Completed => PbOutcome::Completed,
			CallOutcome::Missed => PbOutcome::Missed,
			CallOutcome::Declined => PbOutcome::Declined,
			CallOutcome::Failed => PbOutcome::Failed,
			CallOutcome::Cancelled => PbOutcome::Cancelled,
		} as i32,
		started_at_unix_ms: record.started_at_ms,
		duration_ms: record.duration_ms,
	}
}

pub(super) async fn list(
	service: &EngineService,
	body: ListCallHistoryRequest,
) -> Result<Response<CallHistoryPage>, Status> {
	let limit = match body.limit {
		0 => DEFAULT_LIST,
		n => usize::try_from(n).unwrap_or(MAX_LIST).min(MAX_LIST),
	};
	let before = body.before_id;
	let settings = service.shared.settings.clone();
	// One extra row tells whether older entries exist.
	let mut rows = tokio::task::spawn_blocking(move || settings.call_history(limit + 1, before))
		.await
		.map_err(|_| ServiceError::Stopped)?
		.map_err(ServiceError::from)?;
	let more = rows.len() > limit;
	rows.truncate(limit);
	let revision = service
		.shared
		.handle
		.snapshot()
		.await?
		.settings
		.map_or(0, |s| s.history_revision);
	Ok(Response::new(CallHistoryPage {
		entries: rows.into_iter().map(entry).collect(),
		more,
		revision,
	}))
}

pub(super) async fn clear(
	service: &EngineService,
	mutation: &Mutation,
) -> Result<Response<Operation>, Status> {
	service.shared.handle.clear_call_history().await?;
	Ok(Response::new(Operation {
		operation_id: mutation.operation.to_bytes().to_vec(),
		state: OperationState::Applied as i32,
		revision: 0,
		call_id: Vec::new(),
		problem: None,
	}))
}
