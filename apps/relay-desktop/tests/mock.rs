use std::sync::Arc;
use std::time::Duration;

use relay_desktop::controller::{Controller, SessionSlot};
use relay_desktop::hub::Hub;
use relay_desktop::mock;
use relay_desktop::model::{Mode, Phase};

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
	assert!(ctl.set_transmit(&id, Mode::OpenMic, false).await.is_err());
	ctl.set_transmit(&id, Mode::PushToTalk, true).await.unwrap();
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
