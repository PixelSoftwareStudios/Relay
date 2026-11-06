//! Relative times for the call history ("5 min ago", "Yesterday, 14:03").
//!
//! Pure functions of an explicit "now" and a [`LocalZone`], so tests choose both: the shell
//! supplies `now` (`Shell::now_unix_ms`) and the zone (`Shell::time_zone`).

use relay_io::localtime::LocalZone;

use crate::tr;

const MINUTE_MS: u64 = 60_000;
const HOUR_MS: u64 = 60 * MINUTE_MS;
/// Beyond this many calendar days the date itself is shown.
const MAX_DAYS_AGO: i64 = 6;

/// How long ago something was, in the units the history shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relative {
	/// Less than a minute ago, or in the future (a clock that moved back).
	JustNow,
	MinutesAgo(u64),
	/// Earlier today, a whole number of hours ago.
	HoursAgo(u64),
	/// The previous calendar day, at this local clock time.
	Yesterday(String),
	DaysAgo(u64),
	/// `YYYY-MM-DD` in the zone.
	Date(String),
}

impl Relative {
	/// The words, in the active language.
	pub fn text(&self) -> String {
		match self {
			Self::JustNow => tr!("history-time-now").to_owned(),
			Self::MinutesAgo(n) => tr!("history-time-minutes", count = *n),
			Self::HoursAgo(n) => tr!("history-time-hours", count = *n),
			Self::Yesterday(clock) => tr!("history-time-yesterday", time = clock.as_str()),
			Self::DaysAgo(n) => tr!("history-time-days", count = *n),
			Self::Date(date) => date.clone(),
		}
	}

	/// Whether the text changes within a day, so the screen should refresh it.
	pub fn is_recent(&self) -> bool {
		matches!(
			self,
			Self::JustNow | Self::MinutesAgo(_) | Self::HoursAgo(_)
		)
	}
}

/// Days since 1970-01-01 of a `YYYY-MM-DD` date (proleptic Gregorian).
fn day_number(date: &str) -> Option<i64> {
	let mut parts = date.splitn(3, '-');
	let year: i64 = parts.next()?.parse().ok()?;
	let month: i64 = parts.next()?.parse().ok()?;
	let day: i64 = parts.next()?.parse().ok()?;
	let y = if month <= 2 { year - 1 } else { year };
	let era = y.div_euclid(400);
	let year_of_era = y - era * 400;
	let shifted = (month + 9) % 12;
	let day_of_year = (153 * shifted + 2) / 5 + day - 1;
	let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
	Some(era * 146_097 + day_of_era - 719_468)
}

/// The local `(YYYY-MM-DD, HH:MM)` of a time.
fn civil(zone: &LocalZone, unix_ms: u64) -> (String, String) {
	let full = zone.full(unix_ms);
	let date = full.get(..10).unwrap_or_default().to_owned();
	let clock = full.get(11..16).unwrap_or_default().to_owned();
	(date, clock)
}

/// How long before `now_ms` the time `then_ms` was, as the history words it.
pub fn relative(now_ms: u64, then_ms: u64, zone: &LocalZone) -> Relative {
	if then_ms >= now_ms || now_ms - then_ms < MINUTE_MS {
		return Relative::JustNow;
	}
	let elapsed = now_ms - then_ms;
	if elapsed < HOUR_MS {
		return Relative::MinutesAgo(elapsed / MINUTE_MS);
	}
	let (today, _) = civil(zone, now_ms);
	let (day, clock) = civil(zone, then_ms);
	let (Some(today_n), Some(day_n)) = (day_number(&today), day_number(&day)) else {
		return Relative::Date(day);
	};
	match today_n - day_n {
		0 => Relative::HoursAgo(elapsed / HOUR_MS),
		1 => Relative::Yesterday(clock),
		n @ 2..=MAX_DAYS_AGO => Relative::DaysAgo(n.unsigned_abs()),
		_ => Relative::Date(day),
	}
}

/// A connected time as `m:ss`, or `h:mm:ss` from one hour.
pub fn duration(ms: u64) -> String {
	crate::format::call_duration(ms / 1000)
}
