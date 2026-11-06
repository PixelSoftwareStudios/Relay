//! The `limits.*` defaults are the defaults of the code that enforces them.

use relay_config::prefs::Limits;
use relay_engine::messaging::MessagingLimits;
use relay_messages::limits::{
	GLOBAL_RETAINED_BYTES, MAX_CONCURRENT_TRANSFERS, SESSION_BUDGET_BYTES,
};

#[test]
fn the_messaging_defaults_match_the_library_constants() {
	let d = Limits::default();
	assert_eq!(d.session_bytes(), SESSION_BUDGET_BYTES);
	assert_eq!(d.retained_bytes(), GLOBAL_RETAINED_BYTES);
	assert_eq!(d.transfer_concurrency as usize, MAX_CONCURRENT_TRANSFERS);
	let live = MessagingLimits::default();
	assert_eq!(live.session_bytes, SESSION_BUDGET_BYTES);
	assert_eq!(live.retained_bytes, GLOBAL_RETAINED_BYTES);
	assert_eq!(live.transfers, MAX_CONCURRENT_TRANSFERS);
}

#[test]
fn a_full_default_set_is_valid() {
	assert_eq!(Limits::default().check(), None);
}
