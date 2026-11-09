//! Which build of the API a process carries, so a client can tell a stale daemon from its own.

/// Bumped when the API changes in a way clients depend on (a new RPC or field the UI needs, a
/// changed meaning). A client that sees a different number from its own build knows the
/// running daemon is out of date.
pub const API_REVISION: u32 = 1;

/// The release version of this binary (the workspace version). After a self-update the app is
/// the new release while a daemon started earlier still runs the old one; comparing this tells
/// them apart even when `API_REVISION` did not change.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The build this binary came from: the git commit (with `-dirty` for uncommitted changes)
/// embedded at build time, or the crate version with `-dev` when git was not available.
pub const BUILD_REVISION: &str = env!("RELAY_BUILD_REVISION");

/// The build and API revision of one side of a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
	/// Git commit, `-dirty` or `<version>-dev` (see [`BUILD_REVISION`]).
	pub build: String,
	/// API revision (see [`API_REVISION`]).
	pub api: u32,
	/// Release version (see [`VERSION`]); empty for a daemon that does not report one.
	pub version: String,
}

impl Revision {
	/// The revision of this binary.
	pub fn local() -> Self {
		Self {
			build: BUILD_REVISION.to_owned(),
			api: API_REVISION,
			version: VERSION.to_owned(),
		}
	}

	/// Whether a daemon with this revision is the one this binary belongs with: same API
	/// revision and same release version. The build text is for display only, because two
	/// binaries built from one tree at different moments can carry different commit or `-dirty`
	/// text. In a development tree both sides share the workspace version, so only an installed
	/// release that was replaced by an update shows a difference.
	pub fn is_current(&self) -> bool {
		self.api == API_REVISION && self.version == VERSION
	}
}

impl std::fmt::Display for Revision {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		if self.build.is_empty() {
			write!(f, "unknown (api {})", self.api)
		} else {
			write!(f, "{} (api {})", self.build, self.api)
		}
	}
}
