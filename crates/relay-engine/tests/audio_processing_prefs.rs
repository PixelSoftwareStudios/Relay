//! The saved microphone-processing preferences become the processor the media path installs.

use relay_audio::dsp::NoiseLevel;
use relay_config::prefs::{Preferences, flat};
use relay_engine::daemon::processing_of;

#[test]
fn the_defaults_are_moderate_noise_suppression_without_gain_or_echo_control() {
	let config = processing_of(&Preferences::default().audio);
	assert!(config.noise_suppression && !config.auto_gain && !config.echo_cancel);
	assert_eq!(config.noise_level, NoiseLevel::Moderate);
	assert!(!config.is_bypass());
}

#[test]
fn each_preference_reaches_the_processor() {
	let mut prefs = Preferences::default();
	for (key, value) in [
		("audio.noise_suppression_level", "high"),
		("audio.auto_gain", "true"),
	] {
		prefs = flat::set(&prefs, key, value).unwrap();
	}
	let config = processing_of(&prefs.audio);
	assert!(config.noise_suppression && config.auto_gain);
	assert_eq!(config.noise_level, NoiseLevel::High);
	// Off keeps the strength but nothing runs, which is the bit-exact copy.
	let off = flat::set(&prefs, "audio.noise_suppression", "false").unwrap();
	let off = flat::set(&off, "audio.auto_gain", "false").unwrap();
	let config = processing_of(&off.audio);
	assert!(config.is_bypass());
	assert_eq!(config.noise_level, NoiseLevel::High);
}
