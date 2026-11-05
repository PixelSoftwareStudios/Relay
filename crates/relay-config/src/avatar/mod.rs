//! Avatars: constrained decode, canonical PNG, content-addressed cache.

mod cache;
pub mod limits;
mod process;

pub use cache::AvatarCache;
pub use process::{Avatar, parse_hash, process_avatar, read_avatar_file, validate_canonical};
