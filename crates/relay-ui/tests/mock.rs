use std::sync::Arc;
use std::time::Duration;

use relay_ui::controller::{Controller, SessionSlot};
use relay_ui::hub::Hub;
use relay_ui::mock;
use relay_ui::model::{Mode, Phase};

async fn rig() -> (Controller, Arc<Hub>) {
	let hub = Arc::new(Hub::default());
	let slot = Arc::new(SessionSlot::default());
	tokio::spawn(mock::run(hub.clone(), slot.clone()));
	tokio::time::sleep(Duration::from_millis(10)).await;
	(Controller::new(hub.clone(), slot), hub)
}

#[tokio::test(start_paused = true)]
async fn runs_a_call_through_accept_and_end_without_a_daemon() {
	let (ctl, hub) = rig().await;
	let id = hub.state().calls[0].call_id.clone();
	ctl.accept_call(&id).await.unwrap();
	assert_eq!(hub.state().calls[0].phase, Phase::Connected);
	ctl.end_call(&id).await.unwrap();
	assert_eq!(hub.state().calls[0].phase, Phase::Ended);
}

#[tokio::test(start_paused = true)]
async fn mock_commands_share_the_real_validation() {
	let (ctl, hub) = rig().await;
	let id = hub.state().calls[0].call_id.clone();
	assert!(ctl.set_transmit("zz", Mode::Unmuted).await.is_err());
	ctl.set_transmit(&id, Mode::PushToTalk).await.unwrap();
	let call = hub.state().calls[0].clone();
	assert!(call.transmit.consent_granted);
	assert!(
		!call.transmit.effective_transmitting,
		"push-to-talk is armed, not live"
	);
}

#[tokio::test(start_paused = true)]
async fn meters_run_only_while_visible() {
	let (_ctl, hub) = rig().await;
	hub.set_visible(false);
	hub.clear_meters();
	let _ = hub.take_meters();
	tokio::time::sleep(Duration::from_millis(500)).await;
	assert!(hub.take_meters().is_none());
}

#[tokio::test(start_paused = true)]
async fn mock_channels_invites_and_privacy_work_end_to_end() {
	use relay_client::ChannelOptions;
	use relay_ui::model::{Advert, Inbound};
	let (ctl, hub) = rig().await;
	let options = ChannelOptions {
		listed: true,
		password: false,
		invite_only: true,
	};
	ctl.create_channel("lab", options, "").await.unwrap();
	let channel = hub.state().channels[0].clone();
	assert!(channel.can_admin() && channel.invite_only);
	let token = ctl
		.create_invite(&channel.channel_id, 3600, 1)
		.await
		.unwrap();
	assert!(!format!("{token:?}").contains("MOCK"), "redacted");
	ctl.set_privacy(Some(Inbound::SharedChannelOnly), Some(Advert::Hidden))
		.await
		.unwrap();
	let settings = hub.state().settings.unwrap();
	assert_eq!(
		(settings.inbound, settings.advert),
		(Inbound::SharedChannelOnly, Advert::Hidden)
	);
	ctl.leave_channel(&channel.channel_id).await.unwrap();
	assert!(hub.state().channels.is_empty());
	assert!(
		ctl.connection_info()
			.await
			.unwrap()
			.connection_string
			.starts_with("relay://")
	);
}

#[tokio::test(start_paused = true)]
async fn mock_release_delay_slider_value_round_trips() {
	let (ctl, hub) = rig().await;
	assert_eq!(hub.state().settings.unwrap().ptt_release_delay_ms, 200);
	ctl.set_ptt_release_delay(750).await.unwrap();
	assert_eq!(hub.state().settings.unwrap().ptt_release_delay_ms, 750);
	assert!(ctl.set_ptt_release_delay(5000).await.is_err());
	assert_eq!(hub.state().settings.unwrap().ptt_release_delay_ms, 750);
}

#[tokio::test(start_paused = true)]
async fn mock_ring_settings_round_trip() {
	let (ctl, hub) = rig().await;
	let s = hub.state().settings.unwrap();
	assert!(s.ringtone_enabled && s.ringback_enabled && s.call_sounds_enabled);
	assert!((s.ringtone_volume - 0.4).abs() < 1e-6);
	ctl.set_ring_preferences(Some(false), Some(0.25), None, Some(false))
		.await
		.unwrap();
	let s = hub.state().settings.unwrap();
	assert!(!s.ringtone_enabled && s.ringback_enabled && !s.call_sounds_enabled);
	assert!((s.ringtone_volume - 0.25).abs() < 1e-6);
	ctl.preview_tone().await.unwrap();
}
