//! Unix-domain-socket endpoint: private directory, stale-socket recovery and
//! peer-credential enforcement.

use std::ffi::OsString;
use std::fs::{self, DirBuilder, Permissions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use tokio::net::{UnixListener, UnixStream};
use tokio_stream::wrappers::ReceiverStream;

use crate::ApiError;

const SOCKET_NAME: &str = "engine.sock";

/// The effective uid of this process, read from a socketpair's peer
/// credentials. Needs a tokio runtime context for IO registration.
pub fn current_uid() -> io::Result<u32> {
	let (a, _b) = UnixStream::pair()?;
	Ok(a.peer_cred()?.uid())
}

/// Resolve the per-user socket path: `$XDG_RUNTIME_DIR/relay/engine.sock`, or
/// `<tmp>/relay-<uid>/engine.sock` where no runtime dir exists (macOS).
pub fn default_socket_path(xdg_runtime_dir: Option<OsString>, tmp: &Path, uid: u32) -> PathBuf {
	match xdg_runtime_dir.map(PathBuf::from) {
		Some(dir) if dir.is_absolute() => dir.join("relay").join(SOCKET_NAME),
		_ => tmp.join(format!("relay-{uid}")).join(SOCKET_NAME),
	}
}

/// Create (0700) or verify the directory holding the endpoint. Refuses
/// symlinks, foreign owners and, unless fixable by the owner, loose modes.
pub fn prepare_runtime_dir(dir: &Path) -> Result<(), ApiError> {
	let uid = current_uid()?;
	match fs::symlink_metadata(dir) {
		Err(e) if e.kind() == io::ErrorKind::NotFound => {
			DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
		}
		Err(e) => return Err(e.into()),
		Ok(_) => {}
	}
	let meta = fs::symlink_metadata(dir)?;
	let insecure = |reason| ApiError::InsecurePath {
		path: dir.to_path_buf(),
		reason,
	};
	if !meta.is_dir() {
		return Err(insecure("not a directory (symlinks are refused)"));
	}
	if meta.uid() != uid {
		return Err(insecure("owned by another user"));
	}
	if meta.mode() & 0o077 != 0 {
		fs::set_permissions(dir, Permissions::from_mode(0o700))?;
	}
	Ok(())
}

/// A bound, owner-only listener that removes its socket file on drop.
#[derive(Debug)]
pub struct UdsListener {
	listener: UnixListener,
	path: PathBuf,
	uid: u32,
}

impl UdsListener {
	/// Bind `path`, reclaiming it only if it is a stale socket we own.
	pub async fn bind(path: &Path) -> Result<Self, ApiError> {
		let uid = current_uid()?;
		let dir = path
			.parent()
			.ok_or(ApiError::NotASocket(path.to_path_buf()))?;
		prepare_runtime_dir(dir)?;
		reclaim_if_stale(path, uid).await?;
		let listener = UnixListener::bind(path)?;
		fs::set_permissions(path, Permissions::from_mode(0o600))?;
		Ok(Self {
			listener,
			path: path.to_path_buf(),
			uid,
		})
	}

	/// Socket path being served.
	pub fn path(&self) -> &Path {
		&self.path
	}

	/// Accept loop yielding only connections from this OS user. Rejected
	/// peers are dropped and logged without terminating the stream.
	pub fn incoming(self) -> ReceiverStream<io::Result<UnixStream>> {
		let (tx, rx) = tokio::sync::mpsc::channel(16);
		tokio::spawn(async move {
			loop {
				let stream = match self.listener.accept().await {
					Ok((stream, _)) => stream,
					Err(e) => {
						if tx.send(Err(e)).await.is_err() {
							break;
						}
						continue;
					}
				};
				match check_peer(&stream, self.uid) {
					Ok(()) => {
						if tx.send(Ok(stream)).await.is_err() {
							break;
						}
					}
					// A probe that connects and hangs up (a readiness check) is not an attack.
					Err(ApiError::Io(e)) if e.kind() == io::ErrorKind::NotConnected => {
						tracing::debug!("local api probe closed before it was checked");
					}
					Err(e) => tracing::warn!(error = %e, "rejected local api peer"),
				}
			}
		});
		ReceiverStream::new(rx)
	}
}

impl Drop for UdsListener {
	fn drop(&mut self) {
		let _ = fs::remove_file(&self.path);
	}
}

/// Reject a connection whose peer uid differs from `expected`.
pub fn check_peer(stream: &UnixStream, expected: u32) -> Result<(), ApiError> {
	let peer = stream.peer_cred()?.uid();
	if peer == expected {
		Ok(())
	} else {
		Err(ApiError::PeerRejected { peer, expected })
	}
}

async fn reclaim_if_stale(path: &Path, uid: u32) -> Result<(), ApiError> {
	let meta = match fs::symlink_metadata(path) {
		Ok(meta) => meta,
		Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
		Err(e) => return Err(e.into()),
	};
	if !meta.file_type().is_socket() {
		return Err(ApiError::NotASocket(path.to_path_buf()));
	}
	if meta.uid() != uid {
		return Err(ApiError::InsecurePath {
			path: path.to_path_buf(),
			reason: "socket owned by another user",
		});
	}
	match UnixStream::connect(path).await {
		Ok(_) => Err(ApiError::AlreadyRunning(path.to_path_buf())),
		Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => {
			fs::remove_file(path)?;
			Ok(())
		}
		Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
		Err(e) => Err(e.into()),
	}
}
