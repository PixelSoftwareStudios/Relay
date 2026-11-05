//! `GetInfo` and `RegisterClient`.

use relay_api::pb::{
	ClientContext, ClientSession, EngineInfo, RegisterClientRequest, ResourceLimits,
};
use relay_core::{Command, Reject, Request as CoreRequest};
use tonic::{Response, Status};

use super::{API_MAJOR, EngineService};
use crate::service::clients::Capability;
use crate::service::error::ServiceError;

const MAX_NAME_CHARS: usize = 64;
const AVATAR_MAX_BYTES: u32 = 1024 * 1024;

pub(super) fn info(service: &EngineService) -> EngineInfo {
	let limits = &service.shared.limits;
	EngineInfo {
		engine_incarnation: service.incarnation().to_vec(),
		application_version: env!("CARGO_PKG_VERSION").to_owned(),
		api_major: API_MAJOR,
		api_minor: 0,
		capabilities: Capability::ALL
			.iter()
			.map(|c| c.name().to_owned())
			.collect(),
		limits: Some(ResourceLimits {
			active_calls: u32::try_from(limits.active_calls).unwrap_or(u32::MAX),
			clients: u32::try_from(limits.attached_clients).unwrap_or(u32::MAX),
			avatar_max_bytes: AVATAR_MAX_BYTES,
			meters_max_hz: 0,
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
		Capability::ALL.to_vec()
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
	}))
}
