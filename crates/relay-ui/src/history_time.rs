//! The time of a history row: "Just now" for under a minute, then the
//! app-wide formats of [`timefmt`](crate::timefmt) (`1:54 AM`,
//! `Yesterday at 6:51 PM`, `11/04/26, 3:33 PM`).
//!
//! Pure functions of an explicit "now" and a [`LocalZone`], so tests choose both: the shell
//! supplies `now` (`Shell::now_unix_ms`) and the zone (`Shell::time_zone`).

use relay_io::localtime::LocalZone;

use crate::timefmt;
use crate::tr;

const MINUTE_MS: u64 = 60_000;

/// When a call happened, in the words the history shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relative {
	/// Less than a minute ago, or in the future (a clock that moved back).
	JustNow,
	/// Anything older, already formatted in the zone and language.
	At(String),
}

impl Relative {
	/// The words, in the active language.
	pub fn text(&self) -> String {
		match self {
			Self::JustNow => tr!("history-time-now").to_owned(),
			Self::At(text) => text.clone(),
		}
	}

	/// Whether the text changes soon, so the screen should refresh it.
	pub fn is_recent(&self) -> bool {
		matches!(self, Self::JustNow)
	}
}

/// How `then_ms` is worded at `now_ms`.
pub fn relative(now_ms: u64, then_ms: u64, zone: &LocalZone) -> Relative {
	if then_ms >= now_ms || now_ms - then_ms < MINUTE_MS {
		return Relative::JustNow;
	}
	Relative::At(timefmt::format(zone, now_ms, then_ms))
}

/// A connected time as `m:ss`, or `h:mm:ss` from one hour.
pub fn duration(ms: u64) -> String {
	crate::format::call_duration(ms / 1000)
}
