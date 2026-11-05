//! Launching a long-lived child that outlives this process, and its log file.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Child, Command, Stdio};

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

/// Opens `path` for appending (creating its directory), first moving it to
/// `path.with_extension("log.1")` when it exceeds `rotate_bytes`.
pub fn open_rotating_log(path: &Path, rotate_bytes: u64) -> std::io::Result<File> {
	if let Some(dir) = path.parent() {
		fs::create_dir_all(dir)?;
	}
	if fs::metadata(path).is_ok_and(|meta| meta.len() > rotate_bytes) {
		let _ = fs::rename(path, path.with_extension("log.1"));
	}
	OpenOptions::new().create(true).append(true).open(path)
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
