//! Launching a long-lived child that outlives this process, and its log file.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Child, Command, Stdio};

use crate::error::IoError;
use crate::fs::{ensure_owned_private_dir, open_private_append, restrict_file};

/// The first argument that makes a client executable run the daemon instead of the client:
/// `relay`, `relay-desktop` and `relayd` all honour it (see the `relay-daemon` library), so a
/// client starts the daemon from its own file and never searches for another one.
pub const RUN_DAEMON_ARG: &str = "--run-daemon";

/// A command for a child that survives this process: its own process group (Unix)
/// or a detached process in a new group (Windows), no terminal, no inherited
/// standard input, output appended to `log`.
pub fn detached_command(binary: &Path, args: &[OsString], log: &File) -> std::io::Result<Command> {
	let mut command = Command::new(binary);
	command
		.args(args)
		.stdin(Stdio::null())
		.stdout(log.try_clone()?)
		.stderr(log.try_clone()?);
	#[cfg(unix)]
	{
		use std::os::unix::process::CommandExt;
		command.process_group(0);
	}
	#[cfg(windows)]
	{
		use std::os::windows::process::CommandExt;
		const DETACHED_PROCESS: u32 = 0x0000_0008;
		const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
		command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
	}
	Ok(command)
}

/// Waits for `child` on a background thread so a long-lived parent leaves no
/// zombie when the child eventually exits.
pub fn reap_in_background(mut child: Child) {
	std::thread::spawn(move || {
		let _ = child.wait();
	});
}

/// Opens `path` for appending, first moving it to `path.with_extension("log.1")`
/// when it exceeds `rotate_bytes`. The directory is created 0700 and the file 0600;
/// looser modes on existing ones are tightened, symlinks and files or directories
/// of another user are refused.
pub fn open_rotating_log(path: &Path, rotate_bytes: u64) -> std::io::Result<File> {
	let into_io = |error: IoError| match error {
		IoError::Io { source, .. } => source,
		other => std::io::Error::new(std::io::ErrorKind::PermissionDenied, other.to_string()),
	};
	if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
		ensure_owned_private_dir(dir).map_err(into_io)?;
	}
	if fs::symlink_metadata(path).is_ok_and(|meta| meta.len() > rotate_bytes) {
		let aside = path.with_extension("log.1");
		if fs::rename(path, &aside).is_ok() {
			let _ = restrict_file(&aside);
		}
	}
	open_private_append(path).map_err(into_io)
}

/// The tail of a log file: at most `max_lines` lines from the last `max_bytes`,
/// starting no earlier than byte `from`. Empty if the file cannot be read.
pub fn read_tail(path: &Path, from: u64, max_bytes: u64, max_lines: usize) -> String {
	let Ok(mut file) = File::open(path) else {
		return String::new();
	};
	let len = file.metadata().map_or(0, |meta| meta.len());
	let start = len.saturating_sub(max_bytes).max(from.min(len));
	let mut bytes = Vec::new();
	if file.seek(SeekFrom::Start(start)).is_err() || file.read_to_end(&mut bytes).is_err() {
		return String::new();
	}
	let text = String::from_utf8_lossy(&bytes);
	let lines: Vec<&str> = text.lines().collect();
	lines[lines.len().saturating_sub(max_lines)..].join("\n")
}
