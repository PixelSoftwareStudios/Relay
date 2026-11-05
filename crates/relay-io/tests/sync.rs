use std::sync::{Arc, Mutex, RwLock};

use relay_io::entropy::{random_bytes, random_hex};
use relay_io::{LockExt, RwLockExt};

#[test]
fn mutex_recovers_from_poison() {
	let m = Arc::new(Mutex::new(1));
	let m2 = Arc::clone(&m);
	let _ = std::thread::spawn(move || {
		let _g = m2.lock().unwrap();
		panic!("poison");
	})
	.join();
	assert!(m.is_poisoned());
	*m.lock_unpoisoned() += 1;
	assert_eq!(*m.lock_unpoisoned(), 2);
}

#[test]
fn rwlock_recovers_from_poison() {
	let l = Arc::new(RwLock::new(1));
	let l2 = Arc::clone(&l);
	let _ = std::thread::spawn(move || {
		let _g = l2.write().unwrap();
		panic!("poison");
	})
	.join();
	assert!(l.is_poisoned());
	*l.write_unpoisoned() += 1;
	assert_eq!(*l.read_unpoisoned(), 2);
}

#[test]
fn random_hex_is_fixed_width_and_fresh() {
	let a = random_hex::<32>();
	assert_eq!(a.len(), 64);
	assert!(a.bytes().all(|b| b.is_ascii_hexdigit()));
	assert_ne!(a, random_hex::<32>());
	assert_ne!(random_bytes::<16>(), random_bytes::<16>());
}
