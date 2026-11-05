//! Cross-process behaviour: locks and detached children.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use relay_io::IoError;
use relay_io::lock::ProcessLock;

const HELPER: &str = env!("CARGO_BIN_EXE_io-helper");

#[test]
fn a_lock_excludes_other_processes_and_dies_with_its_holder() {
	let tmp = tempfile::tempdir().unwrap();
	let path = tmp.path().join("run/writer.lock");
	let mut child = Command::new(HELPER)
		.arg("lock")
		.arg(&path)
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.spawn()
		.unwrap();
	let mut line = String::new();
	BufReader::new(child.stdout.take().unwrap())
		.read_line(&mut line)
		.unwrap();
	assert_eq!(line.trim(), "locked");
	assert!(matches!(
		ProcessLock::acquire(&path),
		Err(IoError::Locked(_))
	));
	assert!(matches!(
		ProcessLock::acquire_beside(&path),
		Err(IoError::Locked(_))
	));
	// Killing the holder (no clean release) frees the lock: no stale lock.
	child.kill().unwrap();
	child.wait().unwrap();
	ProcessLock::acquire(&path).unwrap();
}

#[test]
fn a_lock_excludes_a_second_holder_in_the_same_process() {
	let tmp = tempfile::tempdir().unwrap();
	let path = tmp.path().join("l");
	let first = ProcessLock::acquire(&path).unwrap();
	assert!(matches!(
		ProcessLock::acquire(&path),
		Err(IoError::Locked(_))
	));
	drop(first);
	// A concurrent fork in another test thread can briefly share the descriptor
	// until its exec closes it, so allow a short grace period.
	let deadline = Instant::now() + Duration::from_secs(2);
	loop {
		match ProcessLock::acquire(&path) {
			Ok(_) => break,
			Err(IoError::Locked(_)) if Instant::now() < deadline => {
				std::thread::sleep(Duration::from_millis(10));
			}
			Err(e) => panic!("{e}"),
		}
	}
}

#[cfg(unix)]
fn alive(pid: &str) -> bool {
	Command::new("kill")
		.args(["-0", pid])
		.stderr(Stdio::null())
		.status()
		.unwrap()
		.success()
}

#[cfg(unix)]
#[test]
fn a_detached_child_outlives_the_process_that_launched_it() {
	let tmp = tempfile::tempdir().unwrap();
	let (log, pidfile) = (tmp.path().join("logs/d.log"), tmp.path().join("pid"));
	let status = Command::new(HELPER)
		.arg("launch")
		.arg(HELPER)
		.arg(&log)
		.arg(&pidfile)
		.status()
		.unwrap();
	assert!(status.success());
	let pid = std::fs::read_to_string(&pidfile).unwrap();
	// The launcher has exited; the child it detached is still running.
	std::thread::sleep(Duration::from_millis(200));
	assert!(alive(&pid), "child must survive its launcher");
	let _ = Command::new("kill").arg(&pid).status();
	assert!(log.exists(), "the log file was created");
}

#[test]
fn rotating_log_moves_an_oversized_file_aside_and_tail_reads_the_end() {
	use relay_io::proc::{open_rotating_log, read_tail};
	let tmp = tempfile::tempdir().unwrap();
	let log = tmp.path().join("d/relayd.log");
	std::fs::create_dir_all(log.parent().unwrap()).unwrap();
	std::fs::write(&log, "a\nb\nc\nd\n").unwrap();
	assert_eq!(read_tail(&log, 0, 4096, 2), "c\nd");
	assert_eq!(read_tail(&log, 4, 4096, 20), "c\nd");
	open_rotating_log(&log, 3).unwrap();
	assert!(Path::new(&log.with_extension("log.1")).exists());
	assert_eq!(std::fs::read(&log).unwrap().len(), 0);
	assert_eq!(read_tail(&tmp.path().join("none"), 0, 10, 10), "");
}
