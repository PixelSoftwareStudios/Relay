//! Rename that never replaces an existing destination.
//!
//! Strategy, strongest first:
//! 1. Linux/Android: `renameat2(RENAME_NOREPLACE)` (through `rustix`): one atomic
//!    syscall; an existing destination, including a symlink (never followed), yields
//!    `AlreadyExists`. Kernels or filesystems without support fall through.
//! 2. Everywhere else: hard link `from` to `to`, then unlink `from`. `link` fails if
//!    `to` exists, so creation is race-free. Between link and unlink both names exist,
//!    so a crash leaves a duplicate, never a lost or overwritten file. macOS takes
//!    this path (its `renamex_np` has no safe wrapper).
//! 3. Where hard links are unsupported (FAT/exFAT volumes): exclusively create `to` as
//!    a placeholder, then rename over our own placeholder. Another process that
//!    deletes the placeholder inside that window could be overwritten; this is the
//!    only non-atomic path and only used when 1 and 2 are impossible.

use std::fs::{self, OpenOptions};
use std::io;
use std::path::Path;

/// Renames `from` to `to`, failing with [`io::ErrorKind::AlreadyExists`] if `to` exists.
pub fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
	match native(from, to) {
		Err(e) if is_unsupported(&e) => {}
		other => return other,
	}
	match fs::hard_link(from, to) {
		Ok(()) => return fs::remove_file(from),
		Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Err(e),
		Err(_) => {}
	}
	OpenOptions::new().write(true).create_new(true).open(to)?;
	fs::rename(from, to).inspect_err(|_| {
		let _ = fs::remove_file(to);
	})
}

fn is_unsupported(e: &io::Error) -> bool {
	e.kind() == io::ErrorKind::Unsupported
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn native(from: &Path, to: &Path) -> io::Result<()> {
	use rustix::fs::{CWD, RenameFlags, renameat_with};
	use rustix::io::Errno;
	match renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE) {
		Ok(()) => Ok(()),
		Err(Errno::INVAL | Errno::NOSYS | Errno::OPNOTSUPP) => {
			Err(io::Error::from(io::ErrorKind::Unsupported))
		}
		Err(e) => Err(e.into()),
	}
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn native(_from: &Path, _to: &Path) -> io::Result<()> {
	Err(io::Error::from(io::ErrorKind::Unsupported))
}
