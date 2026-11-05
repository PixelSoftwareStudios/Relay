use relay_client::Secret;

#[test]
fn equality_compares_contents() {
	assert_eq!(Secret::new("pw"), Secret::new("pw"));
	assert_ne!(Secret::new("pw"), Secret::new("pw2"));
	assert_ne!(Secret::new("pw"), Secret::new("px"));
	assert_eq!(Secret::new(""), Secret::new(""));
}

#[test]
fn debug_is_redacted() {
	assert_eq!(
		format!("{:?}", Secret::new("hunter2")),
		"Secret(<redacted>)"
	);
}

#[test]
fn emptiness_is_reported() {
	assert!(Secret::new("").is_empty());
	assert!(!Secret::new("x").is_empty());
}
