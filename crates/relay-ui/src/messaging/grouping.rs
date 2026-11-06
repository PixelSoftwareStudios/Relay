//! Which consecutive messages form one visual group in the transcript: the
//! same sender, the same local day and no long pause. A group shows the
//! sender once at its start and the time once at its end. Window-less.

use relay_io::localtime::LocalZone;

use super::model::EntryView;
use crate::timefmt;

/// Messages further apart than this start a new group.
pub const GROUP_GAP_MS: u64 = 5 * 60 * 1000;

/// Where a message sits in its group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
	/// A date separator comes before it (first message, or a new local day).
	pub new_day: bool,
	/// First of its group: the sender name goes above it.
	pub first: bool,
	/// Last of its group: the time goes below it.
	pub last: bool,
}

fn joined(zone: &LocalZone, a: &EntryView, b: &EntryView) -> bool {
	a.outgoing == b.outgoing
		&& b.received_ms.saturating_sub(a.received_ms) <= GROUP_GAP_MS
		&& timefmt::same_day(zone, a.received_ms, b.received_ms)
}

/// The place of `entry` between its visible neighbours.
pub fn place(
	zone: &LocalZone,
	prev: Option<&EntryView>,
	entry: &EntryView,
	next: Option<&EntryView>,
) -> Place {
	Place {
		new_day: prev.is_none_or(|p| !timefmt::same_day(zone, p.received_ms, entry.received_ms)),
		first: prev.is_none_or(|p| !joined(zone, p, entry)),
		last: next.is_none_or(|n| !joined(zone, entry, n)),
	}
}
