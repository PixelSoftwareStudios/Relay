//! Free space of the filesystem holding a directory.

use std::path::Path;

/// Bytes available to this user on the filesystem containing `dir`.
pub fn free_space(dir: &Path) -> std::io::Result<u64> {
	imp(dir)
}

#[cfg(unix)]
fn imp(dir: &Path) -> std::io::Result<u64> {
	use std::ffi::CString;
	use std::os::unix::ffi::OsStrExt;
	let path = CString::new(dir.as_os_str().as_bytes())
		.map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL in path"))?;
	let mut stat = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
	// SAFETY: `path` is a valid NUL-terminated string and `stat` points to writable
	// memory of the right type; `statvfs` only writes into it.
	let rc = unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) };
	if rc != 0 {
		return Err(std::io::Error::last_os_error());
	}
	// SAFETY: `statvfs` returned 0, so it fully initialised the struct.
	let stat = unsafe { stat.assume_init() };
	#[allow(clippy::unnecessary_cast)]
	Ok((stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64))
}

#[cfg(windows)]
fn imp(dir: &Path) -> std::io::Result<u64> {
	use std::os::windows::ffi::OsStrExt;
	use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
	let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
	let mut available = 0u64;
	// SAFETY: `wide` is NUL-terminated and outlives the call; the out pointer is valid
	// and the two unused out parameters may be null.
	let ok = unsafe {
		GetDiskFreeSpaceExW(
			wide.as_ptr(),
			&mut available,
			std::ptr::null_mut(),
			std::ptr::null_mut(),
		)
	};
	if ok == 0 {
		return Err(std::io::Error::last_os_error());
	}
	Ok(available)
}

#[cfg(not(any(unix, windows)))]
fn imp(_dir: &Path) -> std::io::Result<u64> {
	Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}
