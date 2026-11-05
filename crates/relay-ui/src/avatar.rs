//! Avatars: which image a profile shows, and a bounded lazy cache for the
//! decoded textures. The default image is a local asset, never sent anywhere.

use std::collections::HashMap;

/// PNG bytes of the default avatar (256x256).
pub const DEFAULT_PNG: &[u8] = include_bytes!("../../../assets/icon/default-avatar.png");

/// Textures kept at once; older ones are re-fetched on demand.
pub const CACHE_CAPACITY: usize = 64;

/// What to draw for a profile's picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Avatar {
	/// No avatar hash is set: draw the default image.
	Placeholder,
	/// Fetch (once) and draw the image with this hex SHA-256.
	Hash(String),
}

impl Avatar {
	/// Resolve from the hex hash a profile advertises (empty when none).
	pub fn resolve(hash: &str) -> Self {
		if hash.is_empty() {
			Self::Placeholder
		} else {
			Self::Hash(hash.to_owned())
		}
	}
}

/// State of one cache key.
#[derive(Debug, PartialEq, Eq)]
pub enum Lookup<'a, T> {
	/// Never asked for: call [`TextureCache::request`].
	Unknown,
	/// A fetch is in flight.
	Pending,
	/// The image could not be fetched or decoded; the default is drawn.
	Missing,
	Ready(&'a T),
}

enum Slot<T> {
	Pending,
	Missing,
	Ready(T),
}

struct Entry<T> {
	slot: Slot<T>,
	used: u64,
}

/// Bounded, lazily filled map from avatar hash to a decoded texture. It holds
/// no I/O: the owner starts a fetch when [`Self::request`] returns `true` and
/// reports the result with [`Self::fulfill`]. Generic so it tests without egui.
pub struct TextureCache<T> {
	capacity: usize,
	clock: u64,
	entries: HashMap<String, Entry<T>>,
}

impl<T> TextureCache<T> {
	pub fn new(capacity: usize) -> Self {
		Self {
			capacity: capacity.max(1),
			clock: 0,
			entries: HashMap::new(),
		}
	}

	pub fn len(&self) -> usize {
		self.entries.len()
	}

	pub fn is_empty(&self) -> bool {
		self.entries.is_empty()
	}

	/// Look a key up, marking it recently used.
	pub fn get(&mut self, key: &str) -> Lookup<'_, T> {
		self.clock += 1;
		let clock = self.clock;
		match self.entries.get_mut(key) {
			None => Lookup::Unknown,
			Some(entry) => {
				entry.used = clock;
				match &entry.slot {
					Slot::Pending => Lookup::Pending,
					Slot::Missing => Lookup::Missing,
					Slot::Ready(t) => Lookup::Ready(t),
				}
			}
		}
	}

	/// Mark a key as being fetched. Returns `true` when the caller should
	/// start the fetch (the key was unknown).
	pub fn request(&mut self, key: &str) -> bool {
		if self.entries.contains_key(key) {
			return false;
		}
		self.clock += 1;
		self.entries.insert(
			key.to_owned(),
			Entry {
				slot: Slot::Pending,
				used: self.clock,
			},
		);
		self.evict();
		true
	}

	/// Store a fetch result. Ignored if the key was evicted meanwhile.
	pub fn fulfill(&mut self, key: &str, value: Option<T>) {
		if let Some(entry) = self.entries.get_mut(key) {
			entry.slot = value.map_or(Slot::Missing, Slot::Ready);
		}
	}

	/// Drop the least recently used entries beyond the capacity. Pending keys
	/// go last so a slow fetch is not duplicated.
	fn evict(&mut self) {
		while self.entries.len() > self.capacity {
			let victim = self
				.entries
				.iter()
				.min_by_key(|(_, e)| (matches!(e.slot, Slot::Pending), e.used))
				.map(|(k, _)| k.clone());
			match victim {
				Some(key) => self.entries.remove(&key),
				None => break,
			};
		}
	}
}
