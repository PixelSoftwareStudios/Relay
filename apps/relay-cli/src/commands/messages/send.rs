//! Sending: text, inline images and file offers.

use std::path::Path;

use relay_messages::limits::MAX_TEXT_BYTES;

use super::ack::Ack;
use super::target::{absolutize, accepted, path_text, resolve};
use crate::cli::CliError;
use crate::client::Messaging;
use crate::commands::Out;

/// Largest inline image: a control record is 16 KiB.
pub const MAX_INLINE_IMAGE_BYTES: u64 = 15 * 1024;

/// `relay messages send <text|->`
pub async fn text<M: Messaging>(
	client: &mut M,
	call: Option<&str>,
	text: &str,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let body = if text == "-" {
		let raw = out.read_input(MAX_TEXT_BYTES)?;
		raw.strip_suffix('\n')
			.map(|t| t.strip_suffix('\r').unwrap_or(t))
			.unwrap_or(&raw)
			.to_owned()
	} else {
		text.to_owned()
	};
	if body.trim().is_empty() {
		return Err(CliError::Usage("there is no text to send".into()));
	}
	if body.len() > MAX_TEXT_BYTES {
		return Err(CliError::Usage(format!(
			"the text is {} bytes; a message holds at most {MAX_TEXT_BYTES}",
			body.len()
		)));
	}
	let session = resolve(client, call).await?;
	tracing::debug!(bytes = body.len(), "sending a session message");
	let result = accepted(client.send_text(&session.session_id, &body).await?)?;
	out.emit(&Ack::new("sent", &result))
}

/// `relay messages image send <file>`
pub async fn image<M: Messaging>(
	client: &mut M,
	call: Option<&str>,
	file: &Path,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let meta = std::fs::metadata(file)?;
	if !meta.is_file() {
		return Err(CliError::Usage(format!(
			"{} is not a regular file",
			file.display()
		)));
	}
	if meta.len() > MAX_INLINE_IMAGE_BYTES {
		return Err(CliError::Usage(format!(
			"the image is {} bytes; inline images are at most {MAX_INLINE_IMAGE_BYTES} (15 KiB): offer it as a file with `relay messages offer {}`",
			meta.len(),
			file.display()
		)));
	}
	let bytes = std::fs::read(file)?;
	let session = resolve(client, call).await?;
	tracing::debug!(bytes = bytes.len(), "sending an inline image");
	let result = accepted(client.send_image(&session.session_id, bytes).await?)?;
	out.emit(&Ack::new("sent image", &result))
}

/// `relay messages offer <path>`: the daemon opens the file; nothing moves until the peer accepts.
pub async fn offer<M: Messaging>(
	client: &mut M,
	call: Option<&str>,
	path: &Path,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let absolute = absolutize(path)?;
	let session = resolve(client, call).await?;
	let result = accepted(
		client
			.offer_file(&session.session_id, path_text(&absolute)?)
			.await?,
	)?;
	out.emit(&Ack::new("offered", &result))
}
