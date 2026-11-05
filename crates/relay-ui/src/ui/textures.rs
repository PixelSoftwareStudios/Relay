//! Avatar textures: the default image plus a bounded lazily filled cache.

use egui::{self, ColorImage, TextureHandle, TextureOptions};

use crate::actions::Actions;
use crate::avatar::{Avatar, CACHE_CAPACITY, DEFAULT_PNG, Lookup, TextureCache};
use crate::imaging::{Decoded, decode_for_texture};
use crate::inbox::Inbox;

pub fn upload(ctx: &egui::Context, name: &str, image: &Decoded) -> TextureHandle {
	let pixels = ColorImage::from_rgba_unmultiplied(image.size, &image.rgba);
	ctx.load_texture(name, pixels, TextureOptions::LINEAR)
}

pub struct Textures {
	cache: TextureCache<TextureHandle>,
	default: TextureHandle,
}

impl Textures {
	pub fn new(ctx: &egui::Context) -> Self {
		let default = decode_for_texture(DEFAULT_PNG).unwrap_or(Decoded {
			size: [1, 1],
			rgba: vec![128; 4],
		});
		Self {
			cache: TextureCache::new(CACHE_CAPACITY),
			default: upload(ctx, "default-avatar", &default),
		}
	}

	/// The texture to draw now. An unknown hash starts one fetch and shows the
	/// default image until the picture arrives (or fails).
	pub fn texture(&mut self, avatar: &Avatar, actions: &Actions) -> TextureHandle {
		let Avatar::Hash(hash) = avatar else {
			return self.default.clone();
		};
		match self.cache.get(hash) {
			Lookup::Ready(texture) => return texture.clone(),
			Lookup::Pending | Lookup::Missing => {}
			Lookup::Unknown => {
				if self.cache.request(hash) {
					actions.fetch_avatar(hash.clone());
				}
			}
		}
		self.default.clone()
	}

	/// Turn finished downloads into textures.
	pub fn drain(&mut self, ctx: &egui::Context, inbox: &Inbox) {
		for (hash, image) in inbox.take_avatars() {
			let texture =
				image.map(|d| upload(ctx, &format!("avatar-{}", &hash[..hash.len().min(8)]), &d));
			self.cache.fulfill(&hash, texture);
		}
	}
}
