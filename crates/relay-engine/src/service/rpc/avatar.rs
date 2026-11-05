//! `GetAvatar`: the canonical lossless WebP for a cached avatar hash.

use relay_api::pb::{AvatarImage, GetAvatarRequest};
use tonic::{Response, Status};

use super::EngineService;
use super::session::AVATAR_MAX_BYTES;
use crate::service::error::ServiceError;

/// Width and height from the header of a canonical (single `VP8L` chunk) WebP.
fn dimensions(webp: &[u8]) -> Option<(u32, u32)> {
	if webp.len() < 25 || &webp[..4] != b"RIFF" || &webp[8..16] != b"WEBPVP8L" || webp[20] != 0x2f {
		return None;
	}
	let bits = u32::from_le_bytes([webp[21], webp[22], webp[23], webp[24]]);
	Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
}

pub(super) async fn get(
	service: &EngineService,
	body: GetAvatarRequest,
) -> Result<Response<AvatarImage>, Status> {
	let hash = <[u8; 32]>::try_from(body.avatar_sha256.as_slice())
		.map_err(|_| ServiceError::InvalidArgument("malformed avatar hash".to_owned()))?;
	let settings = service.shared.settings.clone();
	let found = tokio::task::spawn_blocking(move || settings.avatar(&hash))
		.await
		.map_err(|_| ServiceError::Stopped)?
		.map_err(ServiceError::from)?;
	let image = found.ok_or_else(|| Status::not_found("AVATAR_UNKNOWN"))?;
	let sized = usize::try_from(AVATAR_MAX_BYTES).is_ok_and(|max| image.len() <= max);
	let (width, height) = dimensions(&image)
		.filter(|_| sized)
		.ok_or_else(|| Status::internal("cached avatar is not a canonical WebP"))?;
	Ok(Response::new(AvatarImage {
		avatar_sha256: hash.to_vec(),
		image,
		width,
		height,
		media_type: "image/webp".to_owned(),
	}))
}
