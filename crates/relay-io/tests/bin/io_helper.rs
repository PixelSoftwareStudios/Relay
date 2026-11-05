//! Child process used by the relay-io integration tests.
//!
//! `lock PATH`: take the process lock, print `locked`, wait for stdin to close.
//! `stage PATH`: stage an atomic write of `new` over PATH and abort before the rename.
//! `launch BIN LOG PIDFILE`: spawn `BIN` detached (a copy of this helper in `sleep`
//! mode), write its pid to PIDFILE and exit at once.
//! `sleep`: sleep for a minute.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;

fn main() {
	let args: Vec<String> = std::env::args().skip(1).collect();
	match args.first().map(String::as_str) {
		Some("lock") => {
			let _lock = relay_io::lock::ProcessLock::acquire(Path::new(&args[1])).expect("lock");
			println!("locked");
			std::io::stdout().flush().expect("flush");
			let mut sink = Vec::new();
			let _ = std::io::stdin().read_to_end(&mut sink);
		}
		Some("stage") => {
			let staged = relay_io::fs::AtomicWrite::new(&args[1])
				.private(true)
				.stage(b"new")
				.expect("stage");
			std::mem::forget(staged);
			std::process::abort();
		}
		Some("launch") => {
			let log = relay_io::proc::open_rotating_log(Path::new(&args[2]), 1 << 20).expect("log");
			let child = relay_io::proc::detached_command(
				Path::new(&args[1]),
				&[OsString::from("sleep")],
				&log,
			)
			.expect("command")
			.spawn()
			.expect("spawn");
			std::fs::write(&args[3], child.id().to_string()).expect("pidfile");
			// Deliberately not waited for: the child must outlive this process.
			std::mem::forget(child);
		}
		Some("sleep") => std::thread::sleep(std::time::Duration::from_secs(60)),
		_ => std::process::exit(2),
	}
}
