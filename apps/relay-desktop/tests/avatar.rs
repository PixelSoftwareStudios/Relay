use relay_desktop::avatar::{Avatar, Lookup, TextureCache};

#[test]
fn resolve_maps_empty_to_the_placeholder() {
	assert_eq!(Avatar::resolve(""), Avatar::Placeholder);
	assert_eq!(Avatar::resolve("ab"), Avatar::Hash("ab".into()));
}

#[test]
fn a_key_is_fetched_once_and_then_served_from_the_cache() {
	let mut cache = TextureCache::new(4);
	assert_eq!(cache.get("a"), Lookup::<u32>::Unknown);
	assert!(cache.request("a"), "first request starts a fetch");
	assert!(!cache.request("a"), "a pending key is not fetched twice");
	assert_eq!(cache.get("a"), Lookup::Pending);
	cache.fulfill("a", Some(7u32));
	assert_eq!(cache.get("a"), Lookup::Ready(&7));
	assert!(!cache.request("a"), "decoded once, never refetched");
}

#[test]
fn failures_are_remembered_so_the_engine_is_not_hammered() {
	let mut cache = TextureCache::<u32>::new(4);
	cache.request("a");
	cache.fulfill("a", None);
	assert_eq!(cache.get("a"), Lookup::Missing);
	assert!(!cache.request("a"));
}

#[test]
fn the_cache_is_bounded_and_evicts_the_least_recently_used() {
	let mut cache = TextureCache::new(3);
	for key in ["a", "b", "c"] {
		cache.request(key);
		cache.fulfill(key, Some(1u32));
	}
	let _ = cache.get("a"); // refresh a; b is now the oldest
	cache.request("d");
	assert_eq!(cache.len(), 3);
	assert_eq!(cache.get("b"), Lookup::Unknown, "oldest entry evicted");
	assert!(matches!(cache.get("a"), Lookup::Ready(_)));
}

#[test]
fn pending_fetches_survive_eviction_pressure_and_late_results_are_ignored() {
	let mut cache = TextureCache::new(2);
	cache.request("slow");
	cache.request("x");
	cache.fulfill("x", Some(1u32));
	cache.request("y");
	assert_eq!(cache.get("slow"), Lookup::Pending);
	assert_eq!(cache.get("x"), Lookup::Unknown);
	cache.fulfill("x", Some(2)); // evicted meanwhile: ignored, not resurrected
	assert_eq!(cache.get("x"), Lookup::Unknown);
}

#[test]
fn a_flood_of_distinct_hashes_never_exceeds_the_capacity() {
	let mut cache = TextureCache::new(8);
	for i in 0..1000 {
		let key = format!("{i}");
		cache.request(&key);
		cache.fulfill(&key, Some(i));
		assert!(cache.len() <= 8);
	}
}
