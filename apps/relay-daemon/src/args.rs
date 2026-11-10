//! Command line of `relayd`.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use relay_config::Paths;
use relay_config::identity::{InsecureFileStore, MemoryStore};
use relay_config::prefs::KeyStorage;
use relay_config::vault::MemoryVault;
use relay_engine::daemon::{RunOptions, Transport};

/// Where the identity key is kept.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum IdentityStore {
	/// The operating system's credential store (the default behaviour).
	Os,
	/// A plaintext file in the home directory. Weaker; for headless machines.
	File,
	/// Nowhere: a new identity on every start, channel secrets in memory too. For tests.
	Memory,
}

/// The per-user Relay engine.
#[derive(Debug, Parser)]
#[command(name = "relayd", version, about)]
pub struct Args {
	/// Unix socket to serve (default: the per-user runtime directory).
	#[arg(long, conflicts_with_all = ["tcp", "tcp_profile"])]
	pub socket: Option<PathBuf>,
	/// Serve loopback TCP with a token profile in the runtime directory.
	#[arg(long)]
	pub tcp: bool,
	/// Serve loopback TCP and write the token profile to this path.
	#[arg(long, value_name = "PATH")]
	pub tcp_profile: Option<PathBuf>,
	/// Keep configuration and data under this directory instead of the
	/// per-user locations (tests, portable installs).
	#[arg(long, value_name = "DIR")]
	pub home: Option<PathBuf>,
	/// Log filter, for example `info` or `relay_engine=debug`. `RELAY_LOG` overrides it.
	#[arg(long, default_value = "info", value_name = "LEVEL")]
	pub log_level: String,
	/// TCP port for inbound peer links (default: the `network.listen_port` preference).
	#[arg(long, value_name = "PORT")]
	pub listen_port: Option<u16>,
	/// Stop gracefully after this many seconds with no client attached (default: never).
	/// Clients that start the daemon for you pass it, so an unused daemon does not linger.
	#[arg(long, value_name = "SECONDS")]
	pub idle_exit: Option<u64>,
	/// Do not advertise or browse on the LAN (manual addresses only).
	#[arg(long, hide = true)]
	pub no_discovery: bool,
	/// Where the identity key lives.
	#[arg(long, value_enum, hide = true)]
	pub identity_store: Option<IdentityStore>,
}

impl Args {
	/// The engine options these arguments describe.
	pub fn options(&self) -> Result<RunOptions> {
		let paths = match &self.home {
			Some(home) => Paths::under(home),
			None => Paths::platform().context("cannot find the per-user directories")?,
		};
		let mut options = RunOptions::new(paths);
		options.transport = if self.tcp || self.tcp_profile.is_some() {
			Transport::LoopbackTcp(self.tcp_profile.clone())
		} else {
			Transport::Unix(self.socket.clone())
		};
		if let Some(port) = self.listen_port {
			options.network.listen = Some(SocketAddr::from(([0, 0, 0, 0], port)));
		}
		options.idle_exit = self.idle_exit.map(std::time::Duration::from_secs);
		if self.no_discovery {
			options.network.discovery = None;
		}
		match self.identity_store {
			None | Some(IdentityStore::Os) => {}
			Some(IdentityStore::Memory) => {
				options.identity_store = Some(Box::new(MemoryStore::default()));
				// Channel secrets stay in memory too: nothing may reach the real keychain.
				options.channel_vault = Some(Arc::new(MemoryVault::default()));
			}
			Some(IdentityStore::File) => {
				let Some(home) = &self.home else {
					bail!("--identity-store file needs --home");
				};
				let store =
					InsecureFileStore::new(home.join("identity.key"), KeyStorage::InsecureFile)
						.context("cannot prepare the identity file")?;
				options.identity_store = Some(Box::new(store));
			}
		}
		Ok(options)
	}
}
