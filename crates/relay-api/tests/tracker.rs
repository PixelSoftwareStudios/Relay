//! Revision bookkeeping, independent of any transport.

use relay_api::pb::state_envelope::Body;
use relay_api::pb::{ResyncRequired, Snapshot, StateEnvelope, StateEvent};
use relay_api::{ApiError, StateTracker, WatchItem};

fn envelope(inc: u8, revision: u64, body: Body) -> StateEnvelope {
	StateEnvelope {
		engine_incarnation: vec![inc; 16],
		revision,
		body: Some(body),
	}
}

fn event(inc: u8, revision: u64) -> StateEnvelope {
	envelope(inc, revision, Body::Event(StateEvent::default()))
}

fn snapshot(inc: u8, revision: u64) -> StateEnvelope {
	envelope(inc, revision, Body::Snapshot(Snapshot::default()))
}

#[test]
fn contiguous_revisions_pass() {
	let mut t = StateTracker::new(true);
	assert!(matches!(
		t.observe(snapshot(1, 5)),
		Ok(WatchItem::Snapshot(_))
	));
	assert!(t.observe(event(1, 6)).is_ok());
	assert!(t.observe(event(1, 7)).is_ok());
	assert_eq!(t.last_revision(), Some(7));
}

#[test]
fn gap_is_reported_when_strict() {
	let mut t = StateTracker::new(true);
	t.observe(snapshot(1, 5)).unwrap();
	assert!(matches!(
		t.observe(event(1, 7)),
		Err(ApiError::RevisionGap {
			expected: 6,
			received: 7
		})
	));
}

#[test]
fn filtered_streams_may_skip_but_never_repeat() {
	let mut t = StateTracker::new(false);
	t.observe(snapshot(1, 5)).unwrap();
	assert!(t.observe(event(1, 9)).is_ok());
	assert!(matches!(
		t.observe(event(1, 9)),
		Err(ApiError::StaleRevision { .. })
	));
}

#[test]
fn incarnation_change_is_fatal() {
	let mut t = StateTracker::resumed(true, vec![1; 16], 3);
	assert!(matches!(
		t.observe(event(2, 4)),
		Err(ApiError::IncarnationChanged)
	));
}

#[test]
fn resync_resets_the_stream() {
	let mut t = StateTracker::resumed(true, vec![1; 16], 3);
	let resync = envelope(1, 50, Body::Resync(ResyncRequired::default()));
	assert!(matches!(t.observe(resync), Ok(WatchItem::Resync(_))));
	assert_eq!(t.last_revision(), None);
	assert!(t.observe(snapshot(2, 50)).is_ok());
}

#[test]
fn empty_envelope_is_malformed() {
	let mut t = StateTracker::new(true);
	let bare = StateEnvelope::default();
	assert!(matches!(t.observe(bare), Err(ApiError::StreamClosed)));
}
