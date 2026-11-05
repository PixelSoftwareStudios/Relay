//! `relay avatar set|clear`.

use relay_config::avatar::process_avatar;

use super::{Out, report};
use crate::cli::{AvatarCommand, CliError};
use crate::client::Manage;

/// The engine's limit for an uploaded avatar.
const MAX_AVATAR_BYTES: u64 = 1024 * 1024;

/// The engine's own text for an empty or oversized body.
const SIZE_REFUSAL: &str = "avatar must be 1 byte to 1 MiB (PNG, JPEG, WebP, GIF, BMP or TIFF)";

/// The format is sniffed from the bytes (extension and media type are ignored, as in the
/// engine), so this mirrors the engine's checks: six formats, 1 MiB, 2048x2048. The engine
/// stays the authority; this only saves a round trip and gives the same wording.
fn precheck(bytes: &[u8]) -> Result<(), CliError> {
	if bytes.is_empty() || bytes.len() as u64 > MAX_AVATAR_BYTES {
		return Err(CliError::Usage(SIZE_REFUSAL.into()));
	}
	process_avatar(bytes)
		.map(drop)
		// `ConfigError` words it as `avatar rejected: <reason>`, as the engine does.
		.map_err(|e| CliError::Usage(e.to_string()))
}

pub async fn run<C: Manage>(
	client: &mut C,
	cmd: AvatarCommand,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	match cmd {
		AvatarCommand::Set { file } => {
			let size = std::fs::metadata(&file)?.len();
			if size == 0 || size > MAX_AVATAR_BYTES {
				return Err(CliError::Usage(SIZE_REFUSAL.into()));
			}
			let bytes = std::fs::read(&file)?;
			precheck(&bytes)?;
			let op = client
				.upload_avatar(bytes, "application/octet-stream")
				.await?;
			report(&op, out)
		}
		AvatarCommand::Clear => {
			let op = client.clear_avatar().await?;
			report(&op, out)
		}
	}
}
