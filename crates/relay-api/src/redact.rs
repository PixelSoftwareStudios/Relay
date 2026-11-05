//! Debug impls for secret-bearing messages. Prost's derived `Debug` is disabled
//! for these types in `build.rs` so passwords and invites cannot reach logs.

use std::fmt;

use crate::pb::{CreateChannelRequest, CreateInviteResponse, JoinChannelRequest};

const REDACTED: &str = "<redacted>";

impl fmt::Debug for CreateChannelRequest {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("CreateChannelRequest")
			.field("context", &self.context)
			.field("name", &self.name)
			.field("policy", &self.policy)
			.field("password", &REDACTED)
			.finish()
	}
}

impl fmt::Debug for JoinChannelRequest {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("JoinChannelRequest")
			.field("context", &self.context)
			.field("channel_id", &self.channel_id)
			.field("name", &self.name)
			.field("password", &REDACTED)
			.field("invite", &REDACTED)
			.finish()
	}
}

impl fmt::Debug for CreateInviteResponse {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("CreateInviteResponse")
			.field("invite", &REDACTED)
			.field("expires_unix", &self.expires_unix)
			.field("operation", &self.operation)
			.finish()
	}
}
