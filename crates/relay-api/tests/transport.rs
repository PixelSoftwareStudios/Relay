#![cfg(unix)]
//! Endpoint hardening: private paths, stale sockets, peer checks, TCP fallback.

mod common;

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use common::{FakeEngine, Harness};
use relay_api::{
	ApiError, ApiLimits, ClientRegistry, Endpoint, RelayClient, TcpProfile, UdsListener,
	bind_loopback, current_uid, default_socket_path, prepare_runtime_dir, profile_path, serve_tcp,
};

fn mode(path: &Path) -> u32 {
	fs::metadata(path).expect("metadata").permissions().mode() & 0o777
}

#[tokio::test]
async fn socket_and_directory_are_owner_only() {
	let h = Harness::start().await;
	assert_eq!(mode(&h.socket), 0o600);
	assert_eq!(mode(h.socket.parent().unwrap()), 0o700);
}

#[tokio::test]
async fn loose_directory_is_tightened() {
	let dir = tempfile::tempdir().unwrap();
	let run = dir.path().join("run");
	fs::create_dir(&run).unwrap();
	fs::set_permissions(&run, fs::Permissions::from_mode(0o755)).unwrap();
	prepare_runtime_dir(&run).expect("owner may tighten");
	assert_eq!(mode(&run), 0o700);
}

#[tokio::test]
async fn symlinked_directory_is_refused() {
	let dir = tempfile::tempdir().unwrap();
	let real = dir.path().join("real");
	fs::create_dir(&real).unwrap();
	let link = dir.path().join("link");
	std::os::unix::fs::symlink(&real, &link).unwrap();
	assert!(matches!(
		prepare_runtime_dir(&link),
		Err(ApiError::InsecurePath { .. })
	));
}

#[tokio::test]
async fn stale_socket_is_reclaimed() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("run").join("engine.sock");
	prepare_runtime_dir(path.parent().unwrap()).unwrap();
	// Simulate a crash: the file stays behind but nobody listens.
	drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
	assert!(path.exists());
	UdsListener::bind(&path)
		.await
		.expect("stale socket reclaimed");
}

#[tokio::test]
async fn live_socket_is_not_stolen() {
	let h = Harness::start().await;
	let err = UdsListener::bind(&h.socket).await.expect_err("live");
	assert!(matches!(err, ApiError::AlreadyRunning(_)));
	// The original engine still answers.
	RelayClient::connect(&h.endpoint())
		.await
		.expect("still serving");
}

#[tokio::test]
async fn non_socket_at_endpoint_is_refused() {
	let dir = tempfile::tempdir().unwrap();
	let run = dir.path().join("run");
	fs::create_dir(&run).unwrap();
	let path = run.join("engine.sock");
	fs::write(&path, b"precious").unwrap();
	assert!(matches!(
		UdsListener::bind(&path).await,
		Err(ApiError::NotASocket(_))
	));
	assert_eq!(fs::read(&path).unwrap(), b"precious");
}

#[tokio::test]
async fn socket_file_removed_on_drop() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("run").join("engine.sock");
	drop(UdsListener::bind(&path).await.unwrap());
	assert!(!path.exists());
}

#[tokio::test]
async fn same_user_peer_credentials_match() {
	let uid = current_uid().expect("uid");
	let (a, _b) = tokio::net::UnixStream::pair().unwrap();
	assert!(relay_api::check_peer(&a, uid).is_ok());
	assert!(matches!(
		relay_api::check_peer(&a, uid.wrapping_add(1)),
		Err(ApiError::PeerRejected { .. })
	));
}

#[test]
fn socket_path_prefers_xdg_runtime_dir() {
	let tmp = Path::new("/tmp");
	let xdg = Some(OsString::from("/run/user/1000"));
	assert_eq!(
		default_socket_path(xdg, tmp, 1000),
		Path::new("/run/user/1000/relay/engine.sock")
	);
	assert_eq!(
		default_socket_path(Some(OsString::from("relative")), tmp, 501),
		Path::new("/tmp/relay-501/engine.sock")
	);
	assert_eq!(
		default_socket_path(None, tmp, 501),
		Path::new("/tmp/relay-501/engine.sock")
	);
}

#[tokio::test]
async fn tcp_fallback_requires_the_token() {
	let dir = tempfile::tempdir().unwrap();
	let path = profile_path(dir.path());
	let (listener, profile) = bind_loopback(&path).await.expect("bind");
	assert_eq!(mode(&path), 0o600);
	assert!(profile.addr.ip().is_loopback());
	assert_eq!(profile.token.len(), 64);
	assert!(profile.token.bytes().all(|b| b.is_ascii_hexdigit()));
	let (_second, other) = bind_loopback(&dir.path().join("other.tcp"))
		.await
		.expect("bind");
	assert_ne!(profile.token, other.token);

	let registry = Arc::new(ClientRegistry::new(8));
	let engine = FakeEngine::new(Arc::clone(&registry), ApiLimits::DEFAULT);
	let served = profile.clone();
	tokio::spawn(async move {
		let never = std::future::pending::<()>();
		let _ = serve_tcp(
			engine,
			listener,
			&served,
			registry,
			ApiLimits::DEFAULT,
			never,
		)
		.await;
	});

	let mut client = RelayClient::connect(&Endpoint::LoopbackTcp {
		profile_path: path.clone(),
	})
	.await
	.expect("authorized connect");
	client.register("tcp", "0", &[]).await.expect("register");
	client.snapshot().await.expect("snapshot");

	// A client holding the wrong token is refused on every RPC.
	let bad = TcpProfile {
		token: "0".repeat(64),
		..profile
	};
	fs::remove_file(&path).unwrap();
	fs::write(&path, format!("addr={}\ntoken={}\n", bad.addr, bad.token)).unwrap();
	fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
	let err = RelayClient::connect(&Endpoint::LoopbackTcp { profile_path: path })
		.await
		.err()
		.expect("bad token");
	assert!(matches!(err, ApiError::Status(s) if s.code() == tonic::Code::Unauthenticated));
}

#[test]
fn tcp_profile_rejects_unsafe_input() {
	assert!(TcpProfile::parse("addr=10.0.0.5:80\ntoken=".to_owned().as_str()).is_err());
	let long = "a".repeat(64);
	assert!(TcpProfile::parse(&format!("addr=8.8.8.8:1\ntoken={long}")).is_err());
	assert!(TcpProfile::parse("addr=127.0.0.1:1\ntoken=short").is_err());
	assert!(TcpProfile::parse(&format!("addr=127.0.0.1:1\ntoken={long}")).is_ok());
}

#[tokio::test]
async fn world_readable_profile_is_refused() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("p");
	fs::write(&path, format!("addr=127.0.0.1:1\ntoken={}", "a".repeat(64))).unwrap();
	fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
	assert!(matches!(
		TcpProfile::load(&path),
		Err(ApiError::InsecurePath { .. })
	));
}
