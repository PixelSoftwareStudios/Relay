//! `GetInfo` and `RegisterClient`.

use relay_api::pb::{
	ClientContext, ClientSession, EngineInfo, Operation, OperationState, RegisterClientRequest,
	ResourceLimits,
};
use relay_core::{Command, Reject, Request as CoreRequest};
use tonic::{Response, Status};

use super::{API_MAJOR, EngineService};
use crate::service::clients::Capability;
use crate::service::convert::Mutation;
use crate::service::error::ServiceError;

const MAX_NAME_CHARS: usize = 64;
pub(super) const AVATAR_MAX_BYTES: u32 = 1024 * 1024;

pub(super) fn info(service: &EngineService) -> EngineInfo {
	let limits = &service.shared.limits;
	EngineInfo {
		engine_incarnation: service.incarnation().to_vec(),
		application_version: env!("CARGO_PKG_VERSION").to_owned(),
		api_major: API_MAJOR,
		api_minor: 0,
		build_revision: relay_api::BUILD_REVISION.to_owned(),
		api_revision: relay_api::API_REVISION,
		capabilities: Capability::ALL
			.iter()
			.map(|c| c.name().to_owned())
			.collect(),
		limits: Some(ResourceLimits {
			active_calls: u32::try_from(limits.active_calls).unwrap_or(u32::MAX),
			clients: u32::try_from(limits.attached_clients).unwrap_or(u32::MAX),
			avatar_max_bytes: AVATAR_MAX_BYTES,
			meters_max_hz: super::telemetry::MAX_HZ,
			ptt_lease_max_ms: u32::try_from(limits.ptt_lease_max.as_millis()).unwrap_or(u32::MAX),
		}),
	}
}

pub(super) async fn register(
	service: &EngineService,
	body: RegisterClientRequest,
) -> Result<Response<ClientSession>, Status> {
	if body.api_major != API_MAJOR {
		return Err(Status::failed_precondition("API_MAJOR_UNSUPPORTED"));
	}
	if body.display_name.chars().count() > MAX_NAME_CHARS
		|| body.display_name.chars().any(char::is_control)
	{
		return Err(ServiceError::InvalidArgument("display name".to_owned()).into());
	}
	let granted: Vec<Capability> = if body.requested_capabilities.is_empty() {
		Capability::DEFAULT.to_vec()
	} else {
		let mut wanted: Vec<_> = body
			.requested_capabilities
			.iter()
			.filter_map(|name| Capability::parse(name))
			.collect();
		wanted.dedup();
		wanted
	};
	let shared = &service.shared;
	let raw = shared.registry.register()?;
	let client = relay_core::ClientId::from_u128(u128::from_be_bytes(raw));
	let attach = CoreRequest {
		client,
		operation: shared.ids.operation(),
		command: Command::Attach,
	};
	let outcome = shared.handle.submit(attach).await;
	let revision = match outcome {
		Ok(applied) if applied.result.is_ok() => applied.api_revision,
		Ok(applied) => {
			shared.registry.remove(&raw);
			return Err(match applied.result {
				Err(Reject::ClientLimit) => ServiceError::ClientLimit.into(),
				_ => Status::internal("client could not be attached"),
			});
		}
		Err(error) => {
			shared.registry.remove(&raw);
			return Err(error.into());
		}
	};
	shared.book.insert(client, granted.clone());
	Ok(Response::new(ClientSession {
		context: Some(ClientContext {
			client_id: raw.to_vec(),
			engine_incarnation: service.incarnation().to_vec(),
		}),
		granted_capabilities: granted.iter().map(|c| c.name().to_owned()).collect(),
		revision,
		daemon_build_revision: relay_api::BUILD_REVISION.to_owned(),
		daemon_api_revision: relay_api::API_REVISION,
	}))
}

/// Records the request to stop the engine. Repeating it changes nothing; the
/// daemon host notices the flag, answers nothing further and shuts down
/// gracefully (calls ended, microphone released, endpoint removed).
pub(super) fn shutdown(service: &EngineService, mutation: &Mutation) -> Operation {
	service.shared.shutdown.send_replace(true);
	Operation {
		operation_id: mutation.operation.to_bytes().to_vec(),
		state: OperationState::Applied as i32,
		revision: 0,
		call_id: Vec::new(),
		problem: None,
	}
}
