use relay_net::proto::EpochSwitch;
use relay_net::session::LinkEpoch;
use relay_net::session::switch::{SwitchError, announce, validate};

#[test]
fn announce_then_validate_accepts_the_current_epoch_only() {
	let (current, next) = (LinkEpoch::random(), LinkEpoch::random());
	let message = announce(&current, &next, &[[1; 16], [2; 16]]);
	let request = validate(&message, &current).unwrap();
	assert_eq!((request.previous, request.next), (current, next));
	assert_eq!(request.active_call_ids.len(), 2);

	assert_eq!(
		validate(&message, &LinkEpoch::random()),
		Err(SwitchError::NotCurrent)
	);
}

#[test]
fn malformed_switches_are_rejected() {
	let (current, next) = (LinkEpoch::random(), LinkEpoch::random());
	assert_eq!(
		validate(&announce(&current, &current, &[]), &current),
		Err(SwitchError::NoChange)
	);
	assert_eq!(
		validate(&announce(&current, &next, &[[1; 16]; 2]), &current),
		Err(SwitchError::BadCalls),
		"duplicate call"
	);
	assert_eq!(
		validate(&announce(&current, &next, &[[0; 16]; 9]), &current),
		Err(SwitchError::BadCalls),
		"too many"
	);

	let mut short = announce(&current, &next, &[]);
	short.next_epoch.pop();
	assert_eq!(validate(&short, &current), Err(SwitchError::BadEpoch));
	let mut bad_call = EpochSwitch {
		active_call_ids: vec![vec![1; 3]],
		..announce(&current, &next, &[])
	};
	assert_eq!(validate(&bad_call, &current), Err(SwitchError::BadCalls));
	bad_call.active_call_ids.clear();
	assert!(validate(&bad_call, &current).is_ok());
}
