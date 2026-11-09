//! Git-derived build identity and the inputs that invalidate it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> Option<String> {
	let out = Command::new("git")
		.current_dir(dir)
		.env("GIT_OPTIONAL_LOCKS", "0")
		.args(args)
		.output()
		.ok()?;
	out.status
		.success()
		.then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Embeds the revision and registers every tracked worktree and Git-state input.
pub fn emit(dir: &Path, version: &str) {
	let revision = match git(dir, &["rev-parse", "--short=12", "HEAD"]) {
		Some(commit) if !commit.is_empty() => {
			let dirty = git(dir, &["status", "--porcelain", "--untracked-files=no"])
				.is_some_and(|status| !status.is_empty());
			format!("{commit}{}", if dirty { "-dirty" } else { "" })
		}
		_ => format!("{version}-dev"),
	};
	println!("cargo:rustc-env=RELAY_BUILD_REVISION={revision}");
	let Some(root) = git(dir, &["rev-parse", "--show-toplevel"]) else {
		return;
	};
	let root = PathBuf::from(root);
	let mut paths = BTreeSet::new();
	if let Some(files) = git(dir, &["ls-files", "-z"]) {
		paths.extend(
			files
				.split('\0')
				.filter(|s| !s.is_empty())
				.map(|p| root.join(p)),
		);
	}
	// Watch directories as well as HEAD: loose refs may be replaced by packed refs,
	// and worktree HEAD/index can live outside the repository's common Git directory.
	for name in ["HEAD", "index", "refs", "packed-refs"] {
		if let Some(path) = git(dir, &["rev-parse", "--git-path", name]) {
			let path = dir.join(path);
			paths.insert(if path.exists() {
				path
			} else {
				path.parent().unwrap_or(dir).to_path_buf()
			});
		}
	}
	for path in paths {
		println!("cargo:rerun-if-changed={}", path.display());
	}
}
