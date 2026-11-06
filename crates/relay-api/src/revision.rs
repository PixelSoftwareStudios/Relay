//! Which build of the API a process carries, so a client can tell a stale daemon from its own.

/// Bumped when the API changes in a way clients depend on (a new RPC or field the UI needs, a
/// changed meaning). A client that sees a different number from its own build knows the
/// running daemon is out of date.
pub const API_REVISION: u32 = 2;

/// The build this binary came from: the git commit (with `-dirty` for uncommitted changes)
/// embedded at build time, or the crate version with `-dev` when git was not available.
pub const BUILD_REVISION: &str = env!("RELAY_BUILD_REVISION");

/// The build and API revision of one side of a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
	/// Git commit, `-dirty` or `<version>-dev` (see [`BUILD_REVISION`]). Empty when the
	/// daemon is too old to report one.
	pub build: String,
	/// API revision (see [`API_REVISION`]); `0` when the daemon is too old to report one.
	pub api: u32,
}

impl Revision {
	/// The revision of this binary.
	pub fn local() -> Self {
		Self {
			build: BUILD_REVISION.to_owned(),
			api: API_REVISION,
		}
	}

	/// Whether this is the same build as the running binary, API revision included.
	pub fn is_current(&self) -> bool {
		*self == Self::local()
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
