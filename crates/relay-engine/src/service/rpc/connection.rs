//! `GetConnectionInfo`: what another person needs to reach this device.

use relay_api::pb::ConnectionInfo;
use tonic::{Response, Status};

use super::EngineService;
use crate::service::error::ServiceError;

pub(super) async fn get(service: &EngineService) -> Result<Response<ConnectionInfo>, Status> {
	let peers = service.shared.peers.clone();
	// Listing interfaces is a system call, so it stays off the async threads.
	let details = tokio::task::spawn_blocking(move || peers.connection_info())
		.await
		.map_err(|_| ServiceError::Stopped)?
		.map_err(ServiceError::from)?;
	Ok(Response::new(ConnectionInfo {
		local_addresses: details.addresses.iter().map(ToString::to_string).collect(),
		listen_port: u32::from(details.listen_port),
		fingerprint: details.fingerprint.to_vec(),
		connection_string: details.connection_string.unwrap_or_default(),
	}))
}
