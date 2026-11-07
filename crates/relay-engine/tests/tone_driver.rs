//! Which tone plays when: the pure decision behind ring, ringback and blips.

use std::time::Duration;

use relay_audio::tones::Sound;
use relay_core::{CallId, CallState, Direction};
use relay_engine::service::{RingPrefs, ToneCommand, ToneDriver, ToneInput};
use tokio::time::Instant;

const CALL: CallId = CallId::from_u128(1);

type Calls = [(CallId, Direction, CallState); 1];

fn input(
	calls: &[(CallId, Direction, CallState)],
	attached: bool,
	deafened: bool,
	prefs: RingPrefs,
) -> ToneInput<'_> {
	ToneInput {
		calls,
		attached,
		deafened,
		prefs,
		now: Instant::now(),
	}
}

fn ring(volume: f32) -> ToneCommand {
	ToneCommand::Start {
		sound: Sound::Ring,
		looping: true,
		volume,
	}
}

fn incoming(state: CallState) -> Calls {
	[(CALL, Direction::Incoming, state)]
}

#[test]
fn ring_needs_a_ringing_invite_a_client_and_the_preference() {
	let prefs = RingPrefs::default();
	let ringing = incoming(CallState::IncomingRinging);
	let mut d = ToneDriver::default();
	assert!(d.sync(&input(&ringing, false, false, prefs)).is_empty());
	assert_eq!(d.sync(&input(&ringing, true, false, prefs)), [ring(0.4)]);
	assert!(d.sync(&input(&ringing, true, false, prefs)).is_empty());
	let accepted = incoming(CallState::Connecting);
	assert_eq!(
		d.sync(&input(&accepted, true, false, prefs)),
		[ToneCommand::Stop]
	);
	let mut off = ToneDriver::default();
	let quiet = RingPrefs {
		ringtone_enabled: false,
		..prefs
	};
	assert!(off.sync(&input(&ringing, true, false, quiet)).is_empty());
}

#[test]
fn the_owner_leaving_declining_or_disabling_stops_the_ring() {
	let prefs = RingPrefs::default();
	let ringing = incoming(CallState::IncomingRinging);
	let quiet = RingPrefs {
		ringtone_enabled: false,
		..prefs
	};
	for (calls, attached, now) in [
		(incoming(CallState::Declined), true, prefs),
		(incoming(CallState::IncomingRinging), false, prefs),
		(incoming(CallState::IncomingRinging), true, quiet),
	] {
		let mut d = ToneDriver::default();
		d.sync(&input(&ringing, true, false, prefs));
		let out = d.sync(&input(&calls, attached, false, now));
		assert_eq!(out, [ToneCommand::Stop]);
	}
}

#[test]
fn volume_changes_apply_to_the_playing_ring() {
	let prefs = RingPrefs::default();
	let ringing = incoming(CallState::IncomingRinging);
	let mut d = ToneDriver::default();
	d.sync(&input(&ringing, true, false, prefs));
	let softer = RingPrefs {
		ringtone_volume: 0.3,
		..prefs
	};
	assert_eq!(
		d.sync(&input(&ringing, true, false, softer)),
		[ToneCommand::Volume(0.3)]
	);
}

#[test]
fn deafen_mutes_and_stops_and_undeafen_unmutes() {
	let prefs = RingPrefs::default();
	let ringing = incoming(CallState::IncomingRinging);
	let mut d = ToneDriver::default();
	d.sync(&input(&ringing, true, false, prefs));
	assert_eq!(
		d.sync(&input(&ringing, true, true, prefs)),
		[ToneCommand::Muted(true), ToneCommand::Stop]
	);
	assert_eq!(
		d.sync(&input(&ringing, true, false, prefs)),
		[ToneCommand::Muted(false), ring(0.4)]
	);
}

#[test]
fn ringback_plays_while_an_outgoing_call_rings() {
	let prefs = RingPrefs::default();
	let mut d = ToneDriver::default();
	let ringing = [(CALL, Direction::Outgoing, CallState::OutgoingRinging)];
	let back = ToneCommand::Start {
		sound: Sound::Ringback,
		looping: true,
		volume: 0.5,
	};
	assert_eq!(d.sync(&input(&ringing, false, false, prefs)), [back]);
	let connecting = [(CALL, Direction::Outgoing, CallState::Connecting)];
	assert_eq!(
		d.sync(&input(&connecting, false, false, prefs)),
		[ToneCommand::Stop]
	);
	let mut off = ToneDriver::default();
	let quiet = RingPrefs {
		ringback_enabled: false,
		..prefs
	};
	assert!(off.sync(&input(&ringing, false, false, quiet)).is_empty());
}

#[test]
fn blips_follow_connect_and_hang_up_when_enabled() {
	let prefs = RingPrefs::default();
	let blip = |sound| ToneCommand::Start {
		sound,
		looping: false,
		volume: 0.4,
	};
	let mut d = ToneDriver::default();
	let active = incoming(CallState::Active);
	assert_eq!(
		d.sync(&input(&active, true, false, prefs)),
		[blip(Sound::Connected)]
	);
	assert!(d.sync(&input(&active, true, false, prefs)).is_empty());
	let ended = incoming(CallState::Ended);
	assert_eq!(
		d.sync(&input(&ended, true, false, prefs)),
		[blip(Sound::CallEnd)]
	);
	let mut off = ToneDriver::default();
	let quiet = RingPrefs {
		call_sounds_enabled: false,
		..prefs
	};
	assert!(off.sync(&input(&active, true, false, quiet)).is_empty());
	assert!(off.sync(&input(&ended, true, false, quiet)).is_empty());
}

#[test]
fn the_test_ring_lasts_two_seconds() {
	let prefs = RingPrefs::default();
	let mut d = ToneDriver::default();
	let start = Instant::now();
	d.preview(start);
	let at = |now| ToneInput {
		calls: &[],
		attached: false,
		deafened: false,
		prefs,
		now,
	};
	assert_eq!(d.sync(&at(start)), [ring(0.4)]);
	assert_eq!(
		d.sync(&at(start + Duration::from_millis(2_100))),
		[ToneCommand::Stop]
	);
	assert!(d.deadline().is_none());
}

#[test]
fn an_outgoing_call_warms_the_output_once_before_its_ringback() {
	let prefs = RingPrefs::default();
	let mut d = ToneDriver::default();
	let preparing = [(CALL, Direction::Outgoing, CallState::Preparing)];
	assert_eq!(
		d.sync(&input(&preparing, true, false, prefs)),
		[ToneCommand::Prewarm]
	);
	assert!(d.sync(&input(&preparing, true, false, prefs)).is_empty());
	let ringing = [(CALL, Direction::Outgoing, CallState::OutgoingRinging)];
	let started = d.sync(&input(&ringing, true, false, prefs));
	assert!(matches!(
		started.as_slice(),
		[ToneCommand::Start {
			sound: Sound::Ringback,
			..
		}]
	));
	// No warm-up when the user switched ringback off or deafened.
	let quiet = RingPrefs {
		ringback_enabled: false,
		..prefs
	};
	assert!(
		ToneDriver::default()
			.sync(&input(&preparing, true, false, quiet))
			.is_empty()
	);
}
