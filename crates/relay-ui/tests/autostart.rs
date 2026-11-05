use std::time::{Duration, Instant};

use relay_ui::autostart::{AutoStart, Decision, env_switch_off};

fn ok() -> Result<String, String> {
	Ok("started".to_owned())
}

#[test]
fn it_starts_at_launch_by_default() {
	let policy = AutoStart::new(false);
	assert_eq!(policy.decide(Instant::now()), Decision::Start);
}

#[test]
fn the_setting_and_the_environment_switch_it_off() {
	let off = AutoStart::new(false);
	off.set_setting(false);
	assert_eq!(off.decide(Instant::now()), Decision::Idle);
	let env = AutoStart::new(true);
	assert_eq!(env.decide(Instant::now()), Decision::Idle);
	assert!(env_switch_off(Some("1")));
	assert!(!env_switch_off(Some("0")));
	assert!(!env_switch_off(None));
}

#[test]
fn a_user_quit_is_never_followed_by_a_restart_until_the_user_starts_it() {
	let policy = AutoStart::new(false);
	policy.user_quit();
	assert_eq!(policy.decide(Instant::now()), Decision::Idle);
	assert!(policy.paused());
	policy.user_start();
	assert_eq!(policy.decide(Instant::now()), Decision::Start);
}

#[test]
fn restarts_back_off_and_stop_after_three_in_a_minute() {
	let policy = AutoStart::new(false);
	let t0 = Instant::now();
	policy.record_start(t0, &ok());
	assert_eq!(policy.decide(t0), Decision::Wait(Duration::from_secs(1)));
	let t1 = t0 + Duration::from_secs(1);
	assert_eq!(policy.decide(t1), Decision::Start);
	policy.record_start(t1, &ok());
	assert_eq!(policy.decide(t1), Decision::Wait(Duration::from_secs(2)));
	let t2 = t1 + Duration::from_secs(2);
	policy.record_start(t2, &Err("no binary".to_owned()));
	assert_eq!(policy.decide(t2 + Duration::from_secs(5)), Decision::GiveUp);
	assert_eq!(policy.last_failure().as_deref(), Some("no binary"));
	assert_eq!(policy.decide(t2 + Duration::from_secs(90)), Decision::Idle);
	policy.user_start();
	assert_eq!(policy.decide(t2 + Duration::from_secs(90)), Decision::Start);
}

#[test]
fn old_starts_age_out_of_the_window() {
	let policy = AutoStart::new(false);
	let t0 = Instant::now();
	for i in 0..2 {
		policy.record_start(t0 + Duration::from_secs(i), &ok());
	}
	assert_eq!(
		policy.decide(t0 + Duration::from_secs(120)),
		Decision::Start
	);
}
