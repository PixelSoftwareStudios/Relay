//! The "ended chats kept" row of Settings, Storage and limits.

use relay_config::prefs::limits::spec;
use relay_ui::limits_settings::{DEFS, LimitValues};

#[test]
fn the_row_counts_calls_and_says_chats_stay_in_memory() {
	let key = "limits.messages_retained_calls";
	let def = DEFS.iter().find(|d| d.key == key).expect("row");
	let spec = spec(key).expect("spec");
	assert_eq!(
		(def.min, def.max, def.default),
		(spec.min, spec.max, spec.default)
	);
	assert_eq!(spec.unit, "calls");
	assert_eq!(def.default, 3);
	assert!(!def.unit.text().is_empty());
	assert!(def.help().contains("until you quit"), "{}", def.help());
	assert_ne!(def.label(), "limits-calls-label");
	let mut values = LimitValues::default();
	values.0[DEFS.iter().position(|d| d.key == key).unwrap()] = 21;
	assert!(values.check().is_err(), "21 calls is out of range");
}
