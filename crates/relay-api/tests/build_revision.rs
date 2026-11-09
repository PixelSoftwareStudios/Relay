//! Exercise actual Cargo invalidation against an isolated Git repository.

use std::path::Path;
use std::process::Command;

fn run(dir: &Path, program: &str, args: &[&str]) -> String {
	let out = Command::new(program)
		.current_dir(dir)
		.args(args)
		.env("GIT_AUTHOR_NAME", "Relay test")
		.env("GIT_AUTHOR_EMAIL", "test@example.invalid")
		.env("GIT_COMMITTER_NAME", "Relay test")
		.env("GIT_COMMITTER_EMAIL", "test@example.invalid")
		.output()
		.unwrap();
	assert!(
		out.status.success(),
		"{program} {args:?}: {}",
		String::from_utf8_lossy(&out.stderr)
	);
	String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

#[test]
fn cargo_revision_follows_commits_checkouts_packed_refs_and_dirty_sources() {
	let temp = tempfile::tempdir().unwrap();
	let dir = temp.path();
	std::fs::create_dir(dir.join("src")).unwrap();
	std::fs::write(
		dir.join("Cargo.toml"),
		"[package]\nname='revision-fixture'\nversion='0.1.0'\nedition='2024'\n",
	)
	.unwrap();
	let helper = Path::new(env!("CARGO_MANIFEST_DIR")).join("build/revision.rs");
	std::fs::write(dir.join("build.rs"), format!("#[path = {helper:?}] mod revision;\nfn main() {{ revision::emit(std::path::Path::new(\".\"), \"0.1.0\"); }}")).unwrap();
	std::fs::write(
		dir.join("src/main.rs"),
		"fn main() { println!(\"{}\", env!(\"RELAY_BUILD_REVISION\")); }",
	)
	.unwrap();
	std::fs::write(dir.join("input.txt"), "initial").unwrap();
	std::fs::write(dir.join(".gitignore"), "target/\n").unwrap();
	run(dir, "cargo", &["generate-lockfile", "--offline"]);
	run(dir, "git", &["init", "-q"]);
	run(dir, "git", &["add", "."]);
	run(
		dir,
		"git",
		&["-c", "commit.gpgsign=false", "commit", "-qm", "initial"],
	);
	let build = || run(dir, "cargo", &["run", "--offline", "--quiet"]);
	let head = || run(dir, "git", &["rev-parse", "--short=12", "HEAD"]);
	assert_eq!(build(), head());
	run(
		dir,
		"git",
		&[
			"-c",
			"commit.gpgsign=false",
			"commit",
			"--allow-empty",
			"-qm",
			"advance",
		],
	);
	assert_eq!(build(), head());
	run(dir, "git", &["pack-refs", "--all", "--prune"]);
	assert_eq!(build(), head());
	run(dir, "git", &["checkout", "-q", "--detach", "HEAD~1"]);
	assert_eq!(build(), head());
	std::fs::write(dir.join("input.txt"), "changed").unwrap();
	assert_eq!(build(), format!("{}-dirty", head()));
	run(dir, "git", &["checkout", "--", "input.txt"]);
	assert_eq!(build(), head());
	// A source archive without Git still has a stable version-based identity.
	let archive = tempfile::tempdir().unwrap();
	std::fs::create_dir(archive.path().join("src")).unwrap();
	for name in ["Cargo.toml", "build.rs", "Cargo.lock", "src/main.rs"] {
		std::fs::copy(dir.join(name), archive.path().join(name)).unwrap();
	}
	assert_eq!(
		run(archive.path(), "cargo", &["run", "--offline", "--quiet"]),
		"0.1.0-dev"
	);
}
