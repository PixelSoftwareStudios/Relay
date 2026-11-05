//! Avatars: constrained decode, canonical compressed PNG, disposable LRU cache.

mod cache;
pub mod limits;
mod png;
mod process;
mod store;

pub use cache::AvatarCache;
pub use process::{Avatar, parse_hash, process_avatar, read_avatar_file, validate_canonical};
