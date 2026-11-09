use relay_client::connection::{ConnectionStringProblem as P, check};
use relay_client::{ChannelOptions, Command, Secret};

const FP: &str = "ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12";

#[test]
fn accepts_well_formed_strings_with_surrounding_space() {
	assert_eq!(check(&format!(" relay://192.168.1.20:7000#{FP}\n")), Ok(()));
	assert_eq!(check(&format!("relay://[fd00::1]:7000#{FP}")), Ok(()));
}

#[test]
fn rejects_each_kind_of_malformed_input() {
	assert_eq!(check("http://1.2.3.4:5#x"), Err(P::Scheme));
	assert_eq!(check("relay://1.2.3.4:5"), Err(P::MissingFingerprint));
	assert_eq!(check("relay://1.2.3.4:5#abcd"), Err(P::BadFingerprint));
	assert_eq!(
		check(&format!("relay://host.local:5#{FP}")),
		Err(P::BadAddress)
	);
	assert_eq!(check(&format!("relay://1.2.3.4#{FP}")), Err(P::BadAddress));
	assert_eq!(
		check(&format!("relay://{}#{FP}", "1".repeat(300))),
		Err(P::Scheme)
	);
}

#[test]
fn secrets_never_appear_in_debug_output() {
	let command = Command::JoinChannel {
		channel_id: vec![1; 16],
		name: String::new(),
		password: Secret::new("hunter2"),
		invite: Secret::new("RLYI1-token"),
	};
	let text = format!("{command:?}");
	assert!(!text.contains("hunter2") && !text.contains("RLYI1"));
	let create = Command::CreateChannel {
		name: "dorm".into(),
		options: ChannelOptions::default(),
		password: Secret::new("pw-secret"),
	};
	assert!(!format!("{create:?}").contains("pw-secret"));
}
