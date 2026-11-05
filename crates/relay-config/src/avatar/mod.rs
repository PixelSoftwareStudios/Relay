//! Avatars: constrained decode, canonical lossless WebP, disposable LRU cache.

mod cache;
pub mod limits;
mod process;
mod store;
mod webp;

pub use cache::AvatarCache;
pub use process::{Avatar, parse_hash, process_avatar, read_avatar_file, validate_canonical};
