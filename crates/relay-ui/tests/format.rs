use relay_ui::format::*;
use relay_ui::model::{Call, Mode, Transmit};

fn call(transmit: Transmit) -> Call {
	Call {
		call_id: "c".into(),
		peer_id: "p".into(),
		phase: relay_ui::model::Phase::Connected,
		transmit,
		user_muted: false,
		user_deafened: false,
		receive_gain: 1.0,
		profile: String::new(),
		problem: None,
		controlled_by_me: true,
		controller_generation: 1,
		connected_at_unix_ms: 0,
	}
}

fn tx(mode: Mode, consent: bool, effective: bool, reason: &str) -> Call {
	call(Transmit {
		mode,
		consent_granted: consent,
		effective_transmitting: effective,
		blocked_reason: reason.into(),
		releasing: false,
	})
}

#[test]
fn unknown_readings_render_empty_and_say_so() {
	assert_eq!(format_dbfs(None), "no signal data");
	assert_eq!(format_dbfs(Some(-20.0)), "-20 dBFS");
}

#[test]
fn avatar_precheck() {
	assert_eq!(avatar_problem(1000), None);
	assert_eq!(avatar_problem(MAX_AVATAR_BYTES), None);
	let refusal = "avatar must be 1 byte to 1 MiB (PNG, JPEG, WebP, GIF, BMP or TIFF)";
	assert_eq!(avatar_problem(0), Some(refusal));
	assert_eq!(avatar_problem(MAX_AVATAR_BYTES + 1), Some(refusal));
}

#[test]
fn listen_only_never_claims_a_live_microphone() {
	assert_eq!(
		describe_transmit(&tx(Mode::Muted, false, true, "")).tone,
		Tone::Idle
	);
}

#[test]
fn transmitting_is_reported_only_when_the_engine_says_so() {
	assert_eq!(
		describe_transmit(&tx(Mode::PushToTalk, true, true, "")).tone,
		Tone::Live
	);
	let armed = describe_transmit(&tx(Mode::PushToTalk, true, false, ""));
	assert_eq!(armed.tone, Tone::Idle);
	assert_eq!(armed.label, "Ready to talk");
	assert!(!armed.detail.contains("engine"));
}

#[test]
fn blocked_reason_from_the_engine_is_surfaced() {
	let s = describe_transmit(&tx(Mode::Unmuted, true, false, "device lost"));
	assert_eq!(s.tone, Tone::Warn);
	assert!(s.detail.contains("device lost"));
}

#[test]
fn releasing_is_reported_only_while_the_engine_says_the_gate_is_open() {
	let mut held = tx(Mode::PushToTalk, true, true, "");
	held.transmit.releasing = true;
	let status = relay_ui::format::describe_transmit(&held);
	assert_eq!(status.label, "Releasing");
	assert_eq!(status.tone, relay_ui::format::Tone::Live);
	// A releasing flag with a closed gate never claims a live microphone.
	let mut closed = tx(Mode::PushToTalk, true, false, "ptt idle");
	closed.transmit.releasing = true;
	assert_eq!(
		relay_ui::format::describe_transmit(&closed).label,
		"Ready to talk"
	);
}

#[test]
fn release_delay_label_names_the_off_state() {
	assert_eq!(
		relay_ui::format::release_delay_label(0),
		"Off (closes at once)"
	);
	assert_eq!(relay_ui::format::release_delay_label(200), "200 ms");
	assert_eq!(relay_ui::format::release_delay_label(2000), "2000 ms");
}
