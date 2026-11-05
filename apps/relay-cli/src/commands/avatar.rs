//! `relay avatar set|clear`.

use std::path::Path;

use super::{Out, report};
use crate::cli::{AvatarCommand, CliError};
use crate::client::Manage;

/// The engine's limit for an uploaded avatar.
const MAX_AVATAR_BYTES: u64 = 1024 * 1024;

/// Media type from the file extension; the engine decodes and validates anyway.
fn media_type(path: &Path) -> Result<&'static str, CliError> {
	match path
		.extension()
		.and_then(|e| e.to_str())
		.map(str::to_ascii_lowercase)
		.as_deref()
	{
		Some("png") => Ok("image/png"),
		Some("jpg" | "jpeg") => Ok("image/jpeg"),
		_ => Err(CliError::Usage(
			"the avatar must be a .png or .jpg file".into(),
		)),
	}
}

pub async fn run<C: Manage>(
	client: &mut C,
	cmd: AvatarCommand,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	match cmd {
		AvatarCommand::Set { file } => {
			let media = media_type(&file)?;
			let size = std::fs::metadata(&file)?.len();
			if size == 0 || size > MAX_AVATAR_BYTES {
				return Err(CliError::Usage(
					"the avatar file must be between 1 byte and 1 MiB".into(),
				));
			}
			let bytes = std::fs::read(&file)?;
			let op = client.upload_avatar(bytes, media).await?;
			report(&op, out)
		}
		AvatarCommand::Clear => Err(CliError::Unsupported(
			"this engine API has no way to remove an avatar yet; set a different image instead"
				.into(),
		)),
	}
}
