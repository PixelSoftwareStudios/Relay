use relay_core::channel::ChannelName;
use relay_core::{CallId, Revision};

#[test]
fn channel_name_parsing_uses_the_same_validation_as_construction() {
	let name: ChannelName = "  Floor 3  ".parse().unwrap();
	assert_eq!(name.as_str(), "Floor 3");
	assert_eq!(ChannelName::try_from("Floor 3").unwrap(), name);
	assert!("<invalid>".parse::<ChannelName>().is_err());
}

#[test]
fn numeric_identifiers_forward_formatting_flags() {
	let id = CallId::from_u128(42);
	assert_eq!(format!("{id:#06x}"), "0x002a");
	assert_eq!(format!("{id:X}"), "2A");
	assert_eq!(format!("{id:o}"), "52");
	assert_eq!(format!("{id:b}"), "101010");
	assert_eq!(format!("{:04x}", Revision::new(42)), "002a");
}
