//! `relay privacy set`.

use relay_api::pb::{AdvertMode, InboundPolicy};

use super::{Out, report};
use crate::cli::{AdvertArg, CliError, InboundArg, PrivacyCommand};
use crate::client::Manage;

pub async fn run<C: Manage>(
	client: &mut C,
	cmd: PrivacyCommand,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let PrivacyCommand::Set { inbound, advert } = cmd;
	if inbound.is_none() && advert.is_none() {
		return Err(CliError::Usage("pass --inbound and/or --advert".into()));
	}
	let inbound = inbound.map(|p| match p {
		InboundArg::Anyone => InboundPolicy::Anyone,
		InboundArg::SharedChannel => InboundPolicy::SharedChannelOnly,
		InboundArg::Nobody => InboundPolicy::Nobody,
	});
	let advert = advert.map(|a| match a {
		AdvertArg::Visible => AdvertMode::Visible,
		AdvertArg::Hidden => AdvertMode::Hidden,
	});
	let op = client.set_privacy(inbound, advert).await?;
	report(&op, out)
}
