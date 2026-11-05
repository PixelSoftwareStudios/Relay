//! Rename that never replaces an existing destination.
//!
//! Strategy, strongest first:
//! 1. `renameat2(RENAME_NOREPLACE)` on Linux, `renamex_np(RENAME_EXCL)` on macOS:
//!    one atomic syscall, and an existing destination (including a symlink, which is
//!    never followed) yields `AlreadyExists`.
//! 2. Hard link `from` to `to` then unlink `from`: `link` fails if `to` exists, so
//!    it is race-free for creation. Between link and unlink both names exist, so a
//!    crash leaves a duplicate, never a lost or overwritten file.
//! 3. Where hard links are unsupported (FAT/exFAT volumes): exclusively create `to`
//!    as a placeholder, then rename over our own placeholder. Another process that
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
	matches!(e.kind(), io::ErrorKind::Unsupported)
		|| matches!(
			e.raw_os_error(),
			Some(code) if code == 22 /* EINVAL */ || code == 38 /* ENOSYS (linux) */ || code == 45 /* ENOTSUP (mac) */ || code == 95 /* EOPNOTSUPP (linux) */
		)
}

#[cfg(target_os = "linux")]
fn native(from: &Path, to: &Path) -> io::Result<()> {
	use std::ffi::CString;
	use std::os::unix::ffi::OsStrExt;
	const RENAME_NOREPLACE: libc::c_uint = 1;
	let cstr = |p: &Path| {
		CString::new(p.as_os_str().as_bytes())
			.map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"))
	};
	let (from, to) = (cstr(from)?, cstr(to)?);
	// SAFETY: both paths are valid NUL-terminated strings that outlive the call.
	let rc = unsafe {
		libc::syscall(
			libc::SYS_renameat2,
			libc::AT_FDCWD,
			from.as_ptr(),
			libc::AT_FDCWD,
			to.as_ptr(),
			RENAME_NOREPLACE,
		)
	};
	if rc == 0 {
		Ok(())
	} else {
		Err(io::Error::last_os_error())
	}
}

#[cfg(target_os = "macos")]
fn native(from: &Path, to: &Path) -> io::Result<()> {
	use std::ffi::CString;
	use std::os::unix::ffi::OsStrExt;
	const RENAME_EXCL: libc::c_uint = 0x0000_0004;
	unsafe extern "C" {
		fn renamex_np(
			from: *const libc::c_char,
			to: *const libc::c_char,
			flags: libc::c_uint,
		) -> libc::c_int;
	}
	let cstr = |p: &Path| {
		CString::new(p.as_os_str().as_bytes())
			.map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"))
	};
	let (from, to) = (cstr(from)?, cstr(to)?);
	// SAFETY: both paths are valid NUL-terminated strings that outlive the call, and
	// `renamex_np` has this signature in libSystem.
	let rc = unsafe { renamex_np(from.as_ptr(), to.as_ptr(), RENAME_EXCL) };
	if rc == 0 {
		Ok(())
	} else {
		Err(io::Error::last_os_error())
	}
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn native(_from: &Path, _to: &Path) -> io::Result<()> {
	Err(io::Error::from(io::ErrorKind::Unsupported))
}
