//! Authenticated loopback-TCP fallback for platforms without Unix sockets.
//!
//! The engine binds `127.0.0.1` on an ephemeral port, generates a random
//! per-launch token and writes both to a private profile file. Every RPC must
//! carry `authorization: Bearer <token>`. Loopback is not authentication; a
//! same-user hostile process can read the profile and is out of scope.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use tokio::net::TcpListener;
use uuid::Uuid;

use crate::ApiError;

/// Address and bearer token a client needs to reach a loopback engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpProfile {
	/// Loopback socket address of the engine.
	pub addr: SocketAddr,
	/// Random bearer token (64 hex characters).
	pub token: String,
}

impl TcpProfile {
	/// Serialize as two `key=value` lines.
	fn render(&self) -> String {
		format!("addr={}\ntoken={}\n", self.addr, self.token)
	}

	/// Parse a profile, rejecting non-loopback addresses and short tokens.
	pub fn parse(text: &str) -> Result<Self, ApiError> {
		let mut addr = None;
		let mut token = None;
		for line in text.lines() {
			match line.split_once('=') {
				Some(("addr", v)) => addr = v.parse::<SocketAddr>().ok(),
				Some(("token", v)) => token = Some(v.to_owned()),
				_ => {}
			}
		}
		let addr = addr.ok_or(ApiError::BadProfile("missing addr"))?;
		let token = token.ok_or(ApiError::BadProfile("missing token"))?;
		if !addr.ip().is_loopback() {
			return Err(ApiError::BadProfile("address is not loopback"));
		}
		if token.len() < 32 {
			return Err(ApiError::BadProfile("token too short"));
		}
		Ok(Self { addr, token })
	}

	/// Read a profile file, refusing group/world-accessible files on Unix.
	pub fn load(path: &Path) -> Result<Self, ApiError> {
		#[cfg(unix)]
		{
			use std::os::unix::fs::PermissionsExt;
			if fs::metadata(path)?.permissions().mode() & 0o077 != 0 {
				return Err(ApiError::InsecurePath {
					path: path.to_path_buf(),
					reason: "profile file is group/world accessible",
				});
			}
		}
		Self::parse(&fs::read_to_string(path)?)
	}

	fn write(&self, path: &Path) -> Result<(), ApiError> {
		match fs::remove_file(path) {
			Ok(()) => {}
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
			Err(e) => return Err(e.into()),
		}
		let mut options = OpenOptions::new();
		options.write(true).create_new(true);
		#[cfg(unix)]
		{
			use std::os::unix::fs::OpenOptionsExt;
			options.mode(0o600);
		}
		options.open(path)?.write_all(self.render().as_bytes())?;
		Ok(())
	}
}

/// Bind loopback on an ephemeral port and publish the profile at `profile_path`
/// (created 0600 on Unix; Windows ACL hardening is an unmet gate).
pub async fn bind_loopback(profile_path: &Path) -> Result<(TcpListener, TcpProfile), ApiError> {
	let listener = TcpListener::bind((IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).await?;
	let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
	let profile = TcpProfile {
		addr: listener.local_addr()?,
		token,
	};
	profile.write(profile_path)?;
	Ok((listener, profile))
}

/// Conventional profile location inside a private runtime directory.
pub fn profile_path(runtime_dir: &Path) -> PathBuf {
	runtime_dir.join("engine.tcp")
}
