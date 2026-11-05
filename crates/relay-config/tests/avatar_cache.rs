use std::io::Cursor;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use relay_config::avatar::{Avatar, AvatarCache, process_avatar};

fn encode(img: DynamicImage) -> Vec<u8> {
	let mut out = Vec::new();
	img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
		.unwrap();
	out
}

/// Distinct, incompressible-ish avatars of similar size.
fn mk(shade: u8) -> Avatar {
	let img = RgbaImage::from_fn(64, 64, |x, y| {
		Rgba([shade, (x * 4) as u8 ^ shade, (y * 4) as u8, 255])
	});
	process_avatar(&encode(DynamicImage::ImageRgba8(img))).unwrap()
}

fn avatars_db(dir: &std::path::Path) -> std::path::PathBuf {
	dir.join("avatars.db")
}

fn flat(shade: u8) -> Avatar {
	let img = RgbaImage::from_pixel(50, 50, Rgba([shade, 2, 3, 255]));
	process_avatar(&encode(DynamicImage::ImageRgba8(img))).unwrap()
}

fn open_in(dir: &std::path::Path, bound: u64) -> AvatarCache {
	AvatarCache::with_limit(dir, bound).unwrap()
}

#[test]
fn cache_stores_verifies_and_rejects_untrusted() {
	let dir = tempfile::tempdir().unwrap();
	let cache = AvatarCache::open(dir.path()).unwrap();
	let a = flat(1);
	cache.put(&a).unwrap();
	assert_eq!(
		cache.get(&a.hash).unwrap().as_deref(),
		Some(a.png.as_slice())
	);
	assert_eq!(cache.get(&[0; 32]).unwrap(), None);
	assert!(cache.put_untrusted(b"junk").is_err());
	assert_eq!(cache.put_untrusted(&a.png).unwrap(), a.hash);
}

#[test]
fn bytes_are_not_in_the_settings_database() {
	let dir = tempfile::tempdir().unwrap();
	let cache = AvatarCache::open(dir.path()).unwrap();
	let a = flat(1);
	cache.put(&a).unwrap();
	cache.set_local(&a.hash).unwrap();
	let settings = rusqlite::Connection::open(dir.path().join("relay.db")).unwrap();
	let tables: i64 = settings
		.query_row(
			"SELECT COUNT(*) FROM sqlite_master WHERE name = 'avatars'",
			[],
			|r| r.get(0),
		)
		.unwrap();
	assert_eq!(tables, 0);
	assert!(avatars_db(dir.path()).exists());
}

#[test]
fn cache_detects_tampering() {
	let dir = tempfile::tempdir().unwrap();
	let cache = AvatarCache::open(dir.path()).unwrap();
	let a = flat(1);
	cache.put(&a).unwrap();
	let db = rusqlite::Connection::open(avatars_db(dir.path())).unwrap();
	db.execute("UPDATE avatars SET data = x'6576696c'", [])
		.unwrap();
	drop(db);
	assert_eq!(cache.get(&a.hash).unwrap(), None);
	assert_eq!(cache.size().unwrap(), 0);
}

#[test]
fn garbage_cache_file_is_wiped_and_recreated() {
	let dir = tempfile::tempdir().unwrap();
	std::fs::write(
		avatars_db(dir.path()),
		b"this is not a database at all, ".repeat(200),
	)
	.unwrap();
	let cache = AvatarCache::open(dir.path()).unwrap();
	assert_eq!(cache.size().unwrap(), 0);
	let a = flat(1);
	cache.put(&a).unwrap();
	assert!(cache.get(&a.hash).unwrap().is_some());
}

#[test]
fn corrupted_pages_are_wiped_silently() {
	let dir = tempfile::tempdir().unwrap();
	let (a, b) = (mk(1), mk(2));
	{
		let cache = AvatarCache::open(dir.path()).unwrap();
		cache.put(&a).unwrap();
		cache.put(&b).unwrap();
	}
	let path = avatars_db(dir.path());
	let mut raw = std::fs::read(&path).unwrap();
	for byte in raw.iter_mut().skip(100) {
		*byte = 0xa5;
	}
	std::fs::write(&path, raw).unwrap();
	let cache = AvatarCache::open(dir.path()).unwrap();
	assert_eq!(cache.get(&a.hash).unwrap(), None);
	cache.put(&b).unwrap();
	assert_eq!(
		cache.get(&b.hash).unwrap().as_deref(),
		Some(b.png.as_slice())
	);
}

#[cfg(unix)]
#[test]
fn cache_file_and_directory_are_private() {
	use std::os::unix::fs::PermissionsExt;
	let dir = tempfile::tempdir().unwrap();
	let sub = dir.path().join("cache");
	let cache = AvatarCache::open(&sub).unwrap();
	cache.put(&flat(1)).unwrap();
	let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
	assert_eq!(mode(&sub), 0o700);
	assert_eq!(mode(&avatars_db(&sub)), 0o600);
}

#[test]
fn eviction_follows_access_order_and_never_exceeds_the_bound() {
	let dir = tempfile::tempdir().unwrap();
	let items: Vec<Avatar> = (1..=6).map(mk).collect();
	let each = items.iter().map(|a| a.png.len()).max().unwrap() as u64;
	let bound = each * 3;
	let cache = open_in(dir.path(), bound);
	for a in &items[..3] {
		cache.put(a).unwrap();
	}
	// Touch the oldest so the second becomes least recently used.
	assert!(cache.get(&items[0].hash).unwrap().is_some());
	for a in &items[3..] {
		cache.put(a).unwrap();
		assert!(cache.size().unwrap() <= bound);
	}
	assert!(cache.get(&items[1].hash).unwrap().is_none());
	assert!(cache.get(&items[2].hash).unwrap().is_none());
	assert!(cache.get(&items[5].hash).unwrap().is_some());
}

#[test]
fn local_avatar_is_pinned_and_persists() {
	let dir = tempfile::tempdir().unwrap();
	let items: Vec<Avatar> = (1..=5).map(mk).collect();
	let each = items.iter().map(|a| a.png.len()).max().unwrap() as u64;
	let bound = each * 2;
	{
		let cache = open_in(dir.path(), bound);
		assert_eq!(cache.local().unwrap(), None);
		cache.put(&items[0]).unwrap();
		cache.set_local(&items[0].hash).unwrap();
		for a in &items[1..] {
			cache.put(a).unwrap();
			assert!(cache.size().unwrap() <= bound);
		}
		assert!(cache.get(&items[1].hash).unwrap().is_none());
	}
	let cache = open_in(dir.path(), bound);
	assert_eq!(cache.local().unwrap(), Some(items[0].hash));
	assert!(cache.get(&items[0].hash).unwrap().is_some());
}

#[test]
fn local_reads_absent_after_the_cache_is_wiped() {
	let dir = tempfile::tempdir().unwrap();
	{
		let cache = open_in(dir.path(), 1 << 20);
		let a = flat(1);
		cache.put(&a).unwrap();
		cache.set_local(&a.hash).unwrap();
	}
	std::fs::remove_file(avatars_db(dir.path())).unwrap();
	assert_eq!(open_in(dir.path(), 1 << 20).local().unwrap(), None);
}

#[test]
fn oversized_entries_are_skipped_not_stored() {
	let dir = tempfile::tempdir().unwrap();
	let cache = open_in(dir.path(), 10);
	cache.put(&flat(1)).unwrap();
	assert_eq!(cache.size().unwrap(), 0);
}

#[test]
fn default_bound_is_128_mib() {
	assert_eq!(
		relay_config::avatar::limits::CACHE_MAX_BYTES,
		128 * 1024 * 1024
	);
}
