//! Message row times: local clock for the label, UTC kept in the hover text.

use relay_io::localtime::LocalZone;
use relay_ui::messaging::rules::row_times;

/// 2026-01-15 12:34:56 UTC.
const RECEIVED: u64 = 1_768_480_496_000;
/// The sender's clock is two minutes behind.
const SENT: u64 = RECEIVED - 120_000;

#[test]
fn label_is_local_and_hover_keeps_utc() {
	let zone = LocalZone::fixed(2 * 3600).unwrap();
	let times = row_times(&zone, RECEIVED, SENT);
	assert_eq!(times.label, "14:34:56");
	assert!(
		times
			.hover
			.starts_with("2026-01-15 14:34:56 +02:00 (12:34:56 UTC), when this device")
	);
	assert!(
		times
			.hover
			.contains("claims 2026-01-15 14:32:56 +02:00 (12:32:56 UTC).")
	);
}

#[test]
fn utc_zone_gives_the_same_clock_as_before() {
	let times = row_times(&LocalZone::utc(), RECEIVED, SENT);
	assert_eq!(times.label, "12:34:56");
}

#[test]
fn the_local_date_can_differ_from_the_utc_date() {
	let zone = LocalZone::fixed(-5 * 3600).unwrap();
	let times = row_times(&zone, RECEIVED - 10 * 3600 * 1000, SENT);
	assert_eq!(times.label, "21:34:56");
	assert!(
		times
			.hover
			.starts_with("2026-01-14 21:34:56 -05:00 (02:34:56 UTC)")
	);
}
