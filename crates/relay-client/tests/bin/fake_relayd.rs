//! A stand-in for `relayd` used by the launcher tests. It claims the endpoint the way the real
//! daemon would and answers `GetInfo` (and the start-up calls) like the real start-up switch.
//!
//! `--mode`: `ok` (serve, ready), `die` (print an error, exit 3), `slow` (bind after one
//! second), `hang` (never bind), `holdlock` (take the advisory lock named by `--lock PATH` and
//! sit on it), `starting` (serve at once but report `Starting` for `--starting-ms` milliseconds,
//! default 1500, as while a keychain prompt waits) or `failed` (serve `Failed` until the
//! `RetryKeychain` call). `--spawn-log PATH` gets one line per start, so tests can count
//! launches. It exits when its socket file disappears, so a test stops it by removing the file.

#[cfg(unix)]
mod fake {
	use std::convert::Infallible;
	use std::future::Ready;
	use std::sync::{Arc, Mutex};
	use std::task::{Context, Poll};
	use std::time::Duration;

	use relay_api::pb::{EngineInfo, Startup, StartupReason, StartupState};
	use relay_api::{ApiLimits, EngineSwitch, LateValidator, StartupControl, UdsListener};
	use tonic::Status;
	use tonic::body::Body;
	use tonic::codegen::http::{Request, Response};

	/// The real engine is never installed: this daemon only reports start-up.
	#[derive(Clone)]
	struct Never;

	impl tonic::server::NamedService for Never {
		const NAME: &'static str = "relay.v1.Engine";
	}

	impl tonic::codegen::Service<Request<Body>> for Never {
		type Response = Response<Body>;
		type Error = Infallible;
		type Future = Ready<Result<Response<Body>, Infallible>>;

		fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
			Poll::Ready(Ok(()))
		}

		fn call(&mut self, _: Request<Body>) -> Self::Future {
			std::future::ready(Ok(Status::unavailable("not an engine").into_http()))
		}
	}

	pub struct Control(pub Mutex<Startup>);

	impl Control {
		fn info(&self) -> EngineInfo {
			EngineInfo {
				application_version: relay_api::VERSION.into(),
				api_major: 1,
				build_revision: relay_api::BUILD_REVISION.into(),
				api_revision: relay_api::API_REVISION,
				startup: Some(self.0.lock().expect("state").clone()),
				..EngineInfo::default()
			}
		}
	}

	#[tonic::async_trait]
	impl StartupControl for Control {
		fn info(&self) -> EngineInfo {
			Control::info(self)
		}
		async fn retry(&self) -> Result<EngineInfo, Status> {
			*self.0.lock().expect("state") = Startup::default();
			Ok(Control::info(self))
		}
		async fn use_file_store(&self) -> Result<EngineInfo, Status> {
			self.retry().await
		}
		async fn reset_identity(&self) -> Result<EngineInfo, Status> {
			Err(Status::failed_precondition("not in the fake"))
		}
	}

	pub fn serve(path: &str, mode: &str, starting_ms: u64) {
		let runtime = tokio::runtime::Builder::new_multi_thread()
			.enable_all()
			.build()
			.expect("runtime");
		runtime.block_on(async {
			let state = match mode {
				"starting" => Startup {
					state: StartupState::Starting as i32,
					reason: StartupReason::WaitingForKeychain as i32,
					detail: String::new(),
				},
				"failed" => Startup {
					state: StartupState::Failed as i32,
					reason: StartupReason::KeychainUnavailable as i32,
					detail: "user denied".into(),
				},
				_ => Startup::default(),
			};
			let control = Arc::new(Control(Mutex::new(state)));
			let (switch, _slot) = EngineSwitch::<Never>::new(control.clone());
			let listener = UdsListener::bind(std::path::Path::new(path))
				.await
				.expect("bind");
			println!("fake relayd listening on {path}");
			if mode == "starting" {
				let later = control.clone();
				tokio::spawn(async move {
					tokio::time::sleep(Duration::from_millis(starting_ms)).await;
					*later.0.lock().expect("state") = Startup::default();
				});
			}
			let owned = path.to_owned();
			let gone = async move {
				while std::path::Path::new(&owned).exists() {
					tokio::time::sleep(Duration::from_millis(50)).await;
				}
			};
			let _ = relay_api::serve_unix_service(
				switch,
				listener,
				Arc::new(LateValidator::default()),
				ApiLimits::DEFAULT,
				gone,
			)
			.await;
		});
	}
}

#[cfg(unix)]
fn main() {
	use std::time::Duration;

	let mut args = std::env::args().skip(1);
	let mut socket = None;
	let mut mode = "ok".to_owned();
	let (mut spawn_log, mut lock, mut idle) = (None, None, "none".to_owned());
	let mut starting_ms = 1500u64;
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--socket" => socket = args.next(),
			"--mode" => mode = args.next().unwrap_or_default(),
			"--spawn-log" => spawn_log = args.next(),
			"--lock" => lock = args.next(),
			"--idle-exit" => idle = args.next().unwrap_or_default(),
			"--starting-ms" => {
				starting_ms = args.next().and_then(|v| v.parse().ok()).unwrap_or(1500)
			}
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
		writeln!(file, "started {} idle={idle}", std::process::id()).expect("write");
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
	fake::serve(&path, &mode, starting_ms);
}

#[cfg(not(unix))]
fn main() {}
