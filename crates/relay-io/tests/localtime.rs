//! Local time rendering with injected zones: no test depends on the host zone.

use relay_io::localtime::{LocalZone, NO_TIME, utc_clock};

/// 2026-01-15 12:34:56.789 UTC (winter) and 2026-07-15 12:34:56 UTC (summer).
const WINTER_MS: u64 = 1_768_480_496_789;
const SUMMER_MS: u64 = 1_784_118_896_000;

#[test]
fn utc_zone_matches_the_old_utc_rendering() {
	assert_eq!(LocalZone::utc().clock(WINTER_MS), "12:34:56");
	assert_eq!(utc_clock(WINTER_MS), "12:34:56");
	assert_eq!(
		LocalZone::utc().full(WINTER_MS),
		"2026-01-15 12:34:56 +00:00"
	);
}

#[test]
fn a_fixed_offset_shifts_the_clock_and_the_date() {
	let plus = LocalZone::fixed(5 * 3600 + 30 * 60).unwrap();
	assert_eq!(plus.clock(WINTER_MS), "18:04:56");
	let minus = LocalZone::fixed(-13 * 3600).unwrap();
	assert_eq!(minus.full(WINTER_MS), "2026-01-14 23:34:56 -13:00");
	assert!(LocalZone::fixed(10_000_000).is_err());
}

#[test]
fn hover_keeps_the_utc_value() {
	let zone = LocalZone::fixed(3600).unwrap();
	assert_eq!(
		zone.hover(WINTER_MS),
		"2026-01-15 13:34:56 +01:00 (12:34:56 UTC)"
	);
}

#[test]
fn daylight_saving_is_applied_per_instant() {
	let berlin = LocalZone::named("Europe/Berlin").unwrap();
	assert_eq!(berlin.full(WINTER_MS), "2026-01-15 13:34:56 +01:00");
	assert_eq!(berlin.full(SUMMER_MS), "2026-07-15 14:34:56 +02:00");
}

#[test]
fn unknown_zone_names_and_unrepresentable_times_are_honest() {
	assert!(LocalZone::named("Not/AZone").is_err());
	assert_eq!(LocalZone::utc().clock(u64::MAX), NO_TIME);
	assert_eq!(LocalZone::utc().full(u64::MAX), NO_TIME);
}

#[test]
fn the_system_zone_always_renders_something_well_formed() {
	let text = LocalZone::system().clock(WINTER_MS);
	assert_eq!(text.len(), 8);
	assert_eq!(text.matches(':').count(), 2);
}
