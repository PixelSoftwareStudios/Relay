use relay_desktop::format::*;
use relay_desktop::model::{Call, Mode, Transmit};

fn call(transmit: Transmit) -> Call {
	Call {
		call_id: "c".into(),
		peer_id: "p".into(),
		phase: relay_desktop::model::Phase::Connected,
		transmit,
		user_muted: false,
		user_deafened: false,
		receive_gain: 1.0,
		profile: String::new(),
		problem: None,
		controlled_by_me: true,
		controller_generation: 1,
	}
}

fn tx(mode: Mode, consent: bool, effective: bool, reason: &str) -> Call {
	call(Transmit {
		mode,
		consent_granted: consent,
		effective_transmitting: effective,
		blocked_reason: reason.into(),
	})
}

#[test]
fn unknown_readings_render_empty_and_say_so() {
	assert_eq!(format_dbfs(None), "no signal data");
	assert_eq!(format_dbfs(Some(-20.0)), "-20 dBFS");
}

#[test]
fn avatar_precheck() {
	assert_eq!(avatar_problem(1000, "image/png"), None);
	assert_eq!(avatar_problem(MAX_AVATAR_BYTES, "image/jpeg"), None);
	assert!(avatar_problem(10, "image/gif").is_some());
	assert!(avatar_problem(0, "image/png").is_some());
	assert!(avatar_problem(MAX_AVATAR_BYTES + 1, "image/png").is_some());
	assert_eq!(media_type_for("A.JPEG"), "image/jpeg");
	assert_eq!(media_type_for("a.bmp"), "application/octet-stream");
}

#[test]
fn listen_only_never_claims_a_live_microphone() {
	assert_eq!(
		describe_transmit(&tx(Mode::ReceiveOnly, false, true, "")).tone,
		Tone::Idle
	);
}

#[test]
fn consent_is_required_before_talking_about_transmitting() {
	let s = describe_transmit(&tx(Mode::OpenMic, false, true, ""));
	assert_eq!(s.tone, Tone::Warn);
	assert!(s.label.contains("not allowed"));
}

#[test]
fn transmitting_is_reported_only_when_the_engine_says_so() {
	assert_eq!(
		describe_transmit(&tx(Mode::PushToTalk, true, true, "")).tone,
		Tone::Live
	);
	let armed = describe_transmit(&tx(Mode::PushToTalk, true, false, ""));
	assert_eq!(armed.tone, Tone::Idle);
	assert!(armed.label.contains("armed"));
}

#[test]
fn blocked_reason_from_the_engine_is_surfaced() {
	let s = describe_transmit(&tx(Mode::OpenMic, true, false, "device lost"));
	assert_eq!(s.tone, Tone::Warn);
	assert!(s.detail.contains("device lost"));
}
