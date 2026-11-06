//! Message row times: the app-wide format for the label, UTC kept in the hover text.

use relay_io::localtime::LocalZone;
use relay_ui::messaging::rules::row_times;

/// 2026-01-15 12:34:56 UTC.
const RECEIVED: u64 = 1_768_480_496_000;
/// The sender's clock is two minutes behind.
const SENT: u64 = RECEIVED - 120_000;
const HOUR: u64 = 3_600_000;

#[test]
fn label_is_local_and_hover_keeps_utc() {
	let zone = LocalZone::fixed(2 * 3600).unwrap();
	let times = row_times(&zone, RECEIVED + HOUR, RECEIVED, SENT);
	assert_eq!(times.label, "2:34 PM");
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
fn older_and_yesterday_messages_say_so() {
	let utc = LocalZone::utc();
	let day = 24 * HOUR;
	assert_eq!(
		row_times(&utc, RECEIVED + HOUR, RECEIVED, SENT).label,
		"12:34 PM"
	);
	assert_eq!(
		row_times(&utc, RECEIVED + day, RECEIVED, SENT).label,
		"Yesterday at 12:34 PM"
	);
	assert_eq!(
		row_times(&utc, RECEIVED + 3 * day, RECEIVED, SENT).label,
		"01/15/26, 12:34 PM"
	);
}

#[test]
fn the_local_date_can_differ_from_the_utc_date() {
	let zone = LocalZone::fixed(-5 * 3600).unwrap();
	let received = RECEIVED - 10 * HOUR;
	let times = row_times(&zone, received + HOUR, received, SENT);
	assert_eq!(times.label, "9:34 PM");
	assert!(
		times
			.hover
			.starts_with("2026-01-14 21:34:56 -05:00 (02:34:56 UTC)")
	);
}
