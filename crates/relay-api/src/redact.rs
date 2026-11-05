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

impl fmt::Debug for crate::pb::SendSessionMessageRequest {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("SendSessionMessageRequest")
			.field("context", &self.context)
			.field("session_id", &self.session_id)
			.field(
				"text",
				&format_args!("<{} bytes redacted>", self.text.len()),
			)
			.finish()
	}
}

impl fmt::Debug for crate::pb::SendSessionImageRequest {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("SendSessionImageRequest")
			.field("context", &self.context)
			.field("session_id", &self.session_id)
			.field("image", &format_args!("<{} bytes>", self.image.len()))
			.finish()
	}
}

impl fmt::Debug for crate::pb::OfferFileRequest {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("OfferFileRequest")
			.field("context", &self.context)
			.field("session_id", &self.session_id)
			.field("path", &REDACTED)
			.finish()
	}
}

impl fmt::Debug for crate::pb::RespondOfferRequest {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("RespondOfferRequest")
			.field("context", &self.context)
			.field("session_id", &self.session_id)
			.field("offer_id", &self.offer_id)
			.field("accept", &self.accept)
			.field("destination_dir", &REDACTED)
			.field("file_name", &REDACTED)
			.finish()
	}
}

impl fmt::Debug for crate::pb::ExportSessionRequest {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("ExportSessionRequest")
			.field("context", &self.context)
			.field("session_id", &self.session_id)
			.field("embed_images", &self.embed_images)
			.field("destination_path", &REDACTED)
			.finish()
	}
}

impl fmt::Debug for crate::pb::ExportSessionResponse {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("ExportSessionResponse")
			.field("operation", &self.operation)
			.field("json", &format_args!("<{} bytes>", self.json.len()))
			.field("written_path", &REDACTED)
			.finish()
	}
}

impl fmt::Debug for crate::pb::SearchSessionRequest {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("SearchSessionRequest")
			.field("client", &self.client)
			.field("session_id", &self.session_id)
			.field("query", &REDACTED)
			.finish()
	}
}

impl fmt::Debug for crate::pb::ImageBody {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("ImageBody")
			.field("media_type", &self.media_type)
			.field("width", &self.width)
			.field("height", &self.height)
			.field("data", &format_args!("<{} bytes>", self.data.len()))
			.finish()
	}
}

impl fmt::Debug for crate::pb::FileOfferBody {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("FileOfferBody")
			.field("offer_id", &self.offer_id)
			.field("name", &REDACTED)
			.field("size", &self.size)
			.field("state", &self.state)
			.field("transferred_bytes", &self.transferred_bytes)
			.finish()
	}
}

impl fmt::Debug for crate::pb::TranscriptEntry {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("TranscriptEntry")
			.field("ordinal", &self.ordinal)
			.field("outgoing", &self.outgoing)
			.field("seq", &self.seq)
			.field("delivery", &self.delivery)
			.field("pinned", &self.pinned)
			.field(
				"body",
				&self.body.as_ref().map(|b| match b {
					crate::pb::transcript_entry::Body::Text(_) => "text",
					crate::pb::transcript_entry::Body::Image(_) => "image",
					crate::pb::transcript_entry::Body::File(_) => "file",
				}),
			)
			.finish()
	}
}
