//! A stand-in for `relayd` used by the launcher tests. It speaks no API: it
//! only claims the endpoint the way the real daemon would.
//!
//! `--mode`: `ok` (bind and serve), `die` (print an error, exit 3),
//! `slow` (bind after one second), `hang` (never bind) or `holdlock` (take the
//! advisory lock named by `--lock PATH` and sit on it). `--spawn-log PATH`
//! gets one line per start, so tests can count launches. It exits when its
//! socket file disappears, so a test stops it by removing the file.

#[cfg(unix)]
fn main() {
	use std::os::unix::net::UnixListener;
	use std::time::Duration;

	let mut args = std::env::args().skip(1);
	let mut socket = None;
	let mut mode = "ok".to_owned();
	let (mut spawn_log, mut lock) = (None, None);
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--socket" => socket = args.next(),
			"--mode" => mode = args.next().unwrap_or_default(),
			"--spawn-log" => spawn_log = args.next(),
			"--lock" => lock = args.next(),
			_ => {}
		}
	}
	if let Some(path) = spawn_log {
		use std::io::Write;
		let mut file = std::fs::OpenOptions::new()
			.create(true)
			.append(true)
			.open(path)
			.expect("spawn log");
		writeln!(file, "started {}", std::process::id()).expect("write");
	}
	match mode.as_str() {
		"die" => {
			eprintln!("boom: cannot bind the endpoint");
			std::process::exit(3);
		}
		"hang" => std::thread::sleep(Duration::from_secs(60)),
		"holdlock" => {
			let file = std::fs::OpenOptions::new()
				.create(true)
				.truncate(false)
				.write(true)
				.open(lock.expect("--lock is required"))
				.expect("lock file");
			file.lock().expect("lock");
			println!("holding the lock");
			std::thread::sleep(Duration::from_secs(60));
			return;
		}
		"slow" => std::thread::sleep(Duration::from_secs(1)),
		_ => {}
	}
	let path = socket.expect("--socket is required");
	let listener = UnixListener::bind(&path).expect("bind");
	listener.set_nonblocking(true).expect("nonblocking");
	println!("fake relayd listening on {path}");
	let mut held = Vec::new();
	for _ in 0..600 {
		if let Ok((stream, _)) = listener.accept() {
			held.push(stream);
		}
		if !std::path::Path::new(&path).exists() {
			return;
		}
		std::thread::sleep(Duration::from_millis(50));
	}
}

#[cfg(not(unix))]
fn main() {}
