use std::fs;
use std::path::Path;
use std::process::Command;

use relay_io::IoError;
use relay_io::fs::{
	AtomicWrite, TempFile, backup_path, check_private_file, create_private_new,
	ensure_owned_private_dir, ensure_private_dir, free_space, read_if_exists, read_limited,
	read_string_limited, remove_if_exists, rename_noreplace, restrict_file, with_suffix,
	write_private, write_private_fresh,
};

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
	use std::os::unix::fs::PermissionsExt;
	fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn suffix_helpers_append_to_the_file_name() {
	assert_eq!(
		with_suffix(Path::new("/a/b.db"), "-wal"),
		Path::new("/a/b.db-wal")
	);
	assert_eq!(backup_path(Path::new("x")), Path::new("x.bak"));
}

#[cfg(unix)]
#[test]
fn private_dir_is_0700_and_loose_dirs_are_tightened() {
	use std::os::unix::fs::PermissionsExt;
	let tmp = tempfile::tempdir().unwrap();
	let dir = tmp.path().join("a/b");
	ensure_private_dir(&dir).unwrap();
	assert_eq!(mode(&dir), 0o700);
	fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
	ensure_private_dir(&dir).unwrap();
	assert_eq!(mode(&dir), 0o700);
}

#[cfg(unix)]
#[test]
fn owned_dir_refuses_symlinks_and_files_and_tightens_loose_modes() {
	use std::os::unix::fs::PermissionsExt;
	let tmp = tempfile::tempdir().unwrap();
	let real = tmp.path().join("real");
	fs::create_dir(&real).unwrap();
	let link = tmp.path().join("link");
	std::os::unix::fs::symlink(&real, &link).unwrap();
	assert!(matches!(
		ensure_owned_private_dir(&link),
		Err(IoError::Insecure { .. })
	));
	let file = tmp.path().join("file");
	fs::write(&file, b"x").unwrap();
	assert!(matches!(
		ensure_owned_private_dir(&file),
		Err(IoError::Insecure { .. })
	));
	fs::set_permissions(&real, fs::Permissions::from_mode(0o777)).unwrap();
	ensure_owned_private_dir(&real).unwrap();
	assert_eq!(mode(&real), 0o700);
	let fresh = tmp.path().join("fresh");
	ensure_owned_private_dir(&fresh).unwrap();
	assert_eq!(mode(&fresh), 0o700);
}

#[cfg(unix)]
#[test]
fn uid_matches_the_owner_of_a_file_we_create() {
	use std::os::unix::fs::MetadataExt;
	let tmp = tempfile::tempdir().unwrap();
	let file = tmp.path().join("f");
	fs::write(&file, b"").unwrap();
	assert_eq!(
		relay_io::fs::current_uid(),
		fs::metadata(&file).unwrap().uid()
	);
}

#[cfg(unix)]
#[test]
fn private_files_are_0600_and_restrict_tightens() {
	use std::os::unix::fs::PermissionsExt;
	let tmp = tempfile::tempdir().unwrap();
	let a = tmp.path().join("a");
	write_private(&a, b"x").unwrap();
	assert_eq!(mode(&a), 0o600);
	fs::set_permissions(&a, fs::Permissions::from_mode(0o644)).unwrap();
	assert!(matches!(
		check_private_file(&a),
		Err(IoError::Insecure { .. })
	));
	restrict_file(&a).unwrap();
	check_private_file(&a).unwrap();
	assert_eq!(mode(&a), 0o600);
	let b = tmp.path().join("b");
	write_private_fresh(&b, b"y").unwrap();
	write_private_fresh(&b, b"z").unwrap();
	assert_eq!(fs::read(&b).unwrap(), b"z");
	assert_eq!(mode(&b), 0o600);
}

#[cfg(unix)]
#[test]
fn exclusive_creation_never_follows_a_symlink() {
	let tmp = tempfile::tempdir().unwrap();
	let target = tmp.path().join("victim");
	fs::write(&target, b"keep").unwrap();
	let link = tmp.path().join("link");
	std::os::unix::fs::symlink(&target, &link).unwrap();
	assert!(create_private_new(&link).is_err());
	assert_eq!(fs::read(&target).unwrap(), b"keep");
	// A planted `.tmp` symlink is replaced, not written through.
	let file = tmp.path().join("cfg");
	std::os::unix::fs::symlink(&target, with_suffix(&file, ".tmp")).unwrap();
	AtomicWrite::new(&file)
		.private(true)
		.write(b"data")
		.unwrap();
	assert_eq!(fs::read(&target).unwrap(), b"keep");
	assert_eq!(fs::read(&file).unwrap(), b"data");
	assert_eq!(mode(&file), 0o600);
}

#[test]
fn atomic_write_replaces_and_keeps_a_backup_on_request() {
	let tmp = tempfile::tempdir().unwrap();
	let file = tmp.path().join("sub/cfg");
	AtomicWrite::new(&file).write(b"one").unwrap();
	assert!(!backup_path(&file).exists());
	AtomicWrite::new(&file).backup(true).write(b"two").unwrap();
	assert_eq!(fs::read(&file).unwrap(), b"two");
	assert_eq!(fs::read(backup_path(&file)).unwrap(), b"one");
	assert!(!with_suffix(&file, ".tmp").exists());
}

#[test]
fn staged_content_is_invisible_until_commit() {
	let tmp = tempfile::tempdir().unwrap();
	let file = tmp.path().join("cfg");
	AtomicWrite::new(&file).write(b"old").unwrap();
	let staged = AtomicWrite::new(&file).stage(b"new").unwrap();
	assert_eq!(fs::read(&file).unwrap(), b"old");
	staged.commit().unwrap();
	assert_eq!(fs::read(&file).unwrap(), b"new");
}

#[test]
fn a_process_killed_before_the_rename_leaves_the_old_file_whole() {
	let tmp = tempfile::tempdir().unwrap();
	let file = tmp.path().join("cfg");
	AtomicWrite::new(&file).write(b"old").unwrap();
	let status = Command::new(env!("CARGO_BIN_EXE_io-helper"))
		.arg("stage")
		.arg(&file)
		.status()
		.unwrap();
	assert!(!status.success(), "the helper aborts");
	assert_eq!(fs::read(&file).unwrap(), b"old");
	// The leftover temp is complete (never partial) and the next write recovers.
	assert_eq!(fs::read(with_suffix(&file, ".tmp")).unwrap(), b"new");
	AtomicWrite::new(&file).write(b"newer").unwrap();
	assert_eq!(fs::read(&file).unwrap(), b"newer");
}

#[test]
fn bounded_reads_refuse_oversized_files() {
	let tmp = tempfile::tempdir().unwrap();
	let file = tmp.path().join("f");
	fs::write(&file, b"12345").unwrap();
	assert_eq!(read_limited(&file, 5).unwrap(), b"12345");
	assert!(matches!(
		read_limited(&file, 4),
		Err(IoError::TooLarge { limit: 4, .. })
	));
	assert_eq!(read_string_limited(&file, 10).unwrap(), "12345");
	fs::write(&file, [0xff, 0xfe]).unwrap();
	assert!(read_string_limited(&file, 10).is_err());
	assert!(read_if_exists(&tmp.path().join("none")).unwrap().is_none());
	assert!(read_if_exists(&file).unwrap().is_some());
}

#[test]
fn remove_if_exists_is_idempotent() {
	let tmp = tempfile::tempdir().unwrap();
	let file = tmp.path().join("f");
	fs::write(&file, b"x").unwrap();
	remove_if_exists(&file).unwrap();
	remove_if_exists(&file).unwrap();
}

#[test]
fn free_space_reports_something_for_an_existing_directory() {
	let tmp = tempfile::tempdir().unwrap();
	assert!(free_space(tmp.path()).unwrap() > 0);
	assert!(free_space(&tmp.path().join("missing")).is_err());
}

#[test]
fn rename_noreplace_never_overwrites() {
	let tmp = tempfile::tempdir().unwrap();
	let (a, b, c) = (
		tmp.path().join("a"),
		tmp.path().join("b"),
		tmp.path().join("c"),
	);
	fs::write(&a, b"A").unwrap();
	fs::write(&b, b"B").unwrap();
	let err = rename_noreplace(&a, &b).unwrap_err();
	assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
	assert_eq!(fs::read(&b).unwrap(), b"B");
	assert!(a.exists());
	rename_noreplace(&a, &c).unwrap();
	assert!(!a.exists());
	assert_eq!(fs::read(&c).unwrap(), b"A");
}

#[cfg(unix)]
#[test]
fn rename_noreplace_does_not_follow_a_destination_symlink() {
	let tmp = tempfile::tempdir().unwrap();
	let (a, victim, link) = (
		tmp.path().join("a"),
		tmp.path().join("victim"),
		tmp.path().join("link"),
	);
	fs::write(&a, b"A").unwrap();
	fs::write(&victim, b"V").unwrap();
	std::os::unix::fs::symlink(&victim, &link).unwrap();
	assert!(rename_noreplace(&a, &link).is_err());
	assert_eq!(fs::read(&victim).unwrap(), b"V");
}

#[cfg(unix)]
#[test]
fn temp_files_are_private_unique_and_removed_on_drop() {
	let tmp = tempfile::tempdir().unwrap();
	let mut one = TempFile::create_in(tmp.path(), "t-").unwrap();
	let two = TempFile::create_in(tmp.path(), "t-").unwrap();
	assert_ne!(one.path(), two.path());
	assert_eq!(mode(one.path()), 0o600);
	let (p1, p2) = (one.path().to_path_buf(), two.path().to_path_buf());
	one.keep();
	drop(one);
	drop(two);
	assert!(p1.exists());
	assert!(!p2.exists());
}

#[cfg(unix)]
#[test]
fn a_private_atomic_write_tightens_a_loose_parent_directory() {
	use std::os::unix::fs::PermissionsExt;
	let tmp = tempfile::tempdir().unwrap();
	let dir = tmp.path().join("data");
	fs::create_dir(&dir).unwrap();
	fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
	AtomicWrite::new(dir.join("f"))
		.private(true)
		.write(b"x")
		.unwrap();
	assert_eq!(mode(&dir), 0o700);
	assert_eq!(mode(&dir.join("f")), 0o600);
}
