//! Filesystem primitives: atomic replace with backup, and the single-writer lock.

mod atomic;
mod lock;

pub use atomic::{backup_path, corrupt_path, read_if_exists, write_atomic};
pub use lock::WriterLock;
