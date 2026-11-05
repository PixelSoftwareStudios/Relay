//! `relay address`: the connection string others use to call this device.

use std::fmt::{self, Display, Formatter};

use relay_api::pb::ConnectionInfo;
use serde::Serialize;

use super::{Out, names};
use crate::cli::{AddressArgs, CliError};
use crate::client::Manage;

/// Connection details of this device.
#[derive(Debug, Serialize)]
pub struct AddressView {
	pub connection_string: String,
	pub local_addresses: Vec<String>,
	pub listen_port: u32,
	pub fingerprint: String,
	pub copied: bool,
}

impl From<&ConnectionInfo> for AddressView {
	fn from(info: &ConnectionInfo) -> Self {
		Self {
			connection_string: info.connection_string.clone(),
			local_addresses: info.local_addresses.clone(),
			listen_port: info.listen_port,
			fingerprint: names::hex_id(&info.fingerprint),
			copied: false,
		}
	}
}

/// The text form is only the connection string, so it pipes cleanly.
impl Display for AddressView {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.write_str(&self.connection_string)
	}
}

pub async fn run<C: Manage>(
	client: &mut C,
	args: &AddressArgs,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let info = client.connection_info().await?;
	let mut view = AddressView::from(&info);
	if view.connection_string.is_empty() {
		return Err(CliError::Rejected {
			code: "no_address".into(),
			message: "the engine has no reachable LAN address to share".into(),
			retryable: true,
		});
	}
	let copy_result = args.copy.then(|| out.copy(&view.connection_string));
	view.copied = matches!(copy_result, Some(Ok(())));
	out.emit(&view)?;
	match copy_result {
		Some(Err(reason)) => Err(CliError::Other(anyhow::anyhow!(
			"the connection string was printed but could not be copied: {reason}"
		))),
		_ => Ok(()),
	}
}
