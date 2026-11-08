//! Filesystem primitives: crash-safe replacement, owner-only creation, bounded
//! reads, temporary files and no-overwrite rename.

mod atomic;
mod names;
mod noreplace;
mod private;
mod read;
mod remove;
mod space;
mod temp;

pub use atomic::{AtomicWrite, Staged, sync_dir};
pub use names::{backup_path, corrupt_path, with_suffix};
pub use noreplace::rename_noreplace;
#[cfg(unix)]
pub use private::current_uid;
pub use private::{
	check_private_file, create_private_file, create_private_new, ensure_owned_private_dir,
	ensure_private_dir, open_private_append, restrict_file, write_private, write_private_fresh,
};
pub use read::{read_if_exists, read_limited, read_string_limited};
pub use remove::remove_if_exists;
pub use space::free_space;
pub use temp::TempFile;
