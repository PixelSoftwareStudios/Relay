//! Filesystem primitives: atomic replace with backup, owner-only permissions and the
//! single-writer lock.

mod atomic;
mod lock;
mod private;

pub use atomic::{backup_path, corrupt_path, read_if_exists, sync_dir, with_suffix, write_atomic};
pub use lock::WriterLock;
pub use private::{ensure_private_dir, restrict_file};
