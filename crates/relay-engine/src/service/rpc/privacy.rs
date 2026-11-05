//! `SetPrivacy`: who may ring this device and whether discovery shows it.

use relay_api::pb::{
	AdvertMode as PbAdvert, InboundPolicy as PbInbound, Operation, SetPrivacyRequest,
};
use tonic::{Response, Status};

use super::EngineService;
use super::ops::service_op;
use crate::service::convert::Mutation;
use crate::service::error::ServiceError;
use crate::service::handle::ServiceOp;
use crate::service::ports::{AdvertMode, InboundPolicy};

fn invalid(what: &str) -> ServiceError {
	ServiceError::InvalidArgument(what.to_owned())
}

pub(super) async fn set(
	service: &EngineService,
	mutation: &Mutation,
	body: &SetPrivacyRequest,
) -> Result<Response<Operation>, Status> {
	let inbound = body
		.inbound_policy
		.map(|raw| match PbInbound::try_from(raw) {
			Ok(PbInbound::Anyone) => Ok(InboundPolicy::Anyone),
			Ok(PbInbound::SharedChannelOnly) => Ok(InboundPolicy::SharedChannelOnly),
			_ => Err(invalid("inbound policy")),
		})
		.transpose()?;
	let advert = body
		.advert_mode
		.map(|raw| match PbAdvert::try_from(raw) {
			Ok(PbAdvert::Visible) => Ok(AdvertMode::Visible),
			Ok(PbAdvert::Hidden) => Ok(AdvertMode::Hidden),
			_ => Err(invalid("advert mode")),
		})
		.transpose()?;
	if inbound.is_none() && advert.is_none() {
		return Err(invalid("nothing to change").into());
	}
	service_op(
		service,
		mutation,
		body,
		ServiceOp::Privacy { inbound, advert },
	)
	.await
}
