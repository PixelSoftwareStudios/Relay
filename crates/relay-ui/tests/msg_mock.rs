mod msgkit;

use std::sync::Arc;

use msgkit::*;
use relay_ui::messaging::actions::Intent;
use relay_ui::messaging::mock::{ADA_CALL, run};
use relay_ui::messaging::model::{Body, Delivery, OfferStatus};
use relay_ui::messaging::state::Feed;

fn ada() -> String {
	hex::encode([ADA_CALL; 16])
}

#[tokio::test]
async fn mock_waits_for_opt_in_then_seeds_every_kind() {
	let shell = Arc::new(FakeShell::default());
	let a = actions(shell.clone());
	let hub = a.shared.messages.clone();
	tokio::spawn(run(hub.clone(), shell));
	tokio::time::sleep(std::time::Duration::from_millis(30)).await;
	assert_eq!(hub.read(|s| s.feed.clone()), Feed::Off);
	hub.set_enabled(true);
	until(|| hub.read(|s| s.feed == Feed::Live)).await;
	hub.read(|s| {
		assert_eq!(
			s.live_sessions().count(),
			2,
			"two live calls for the destination rule"
		);
		assert!(s.sessions.iter().any(|x| !x.is_live()));
		let key = s.session_for_call(&ada()).unwrap().key.clone();
		let t = s.transcript(&key).unwrap();
		assert!(t.entries.iter().any(|e| matches!(e.body, Body::Image(_))));
		assert!(
			t.entries
				.iter()
				.any(|e| matches!(&e.body, Body::File(o) if o.status == OfferStatus::Offered))
		);
		assert!(t.entries.iter().any(|e| e.delivery == Delivery::Failed));
	});
	hub.set_enabled(false);
	assert!(hub.read(|s| s.sessions.is_empty()));
}

#[tokio::test]
async fn mock_send_goes_sending_then_delivered_and_fail_can_be_retried() {
	let shell = Arc::new(FakeShell::default());
	let a = actions(shell.clone());
	let hub = a.shared.messages.clone();
	tokio::spawn(run(hub.clone(), shell));
	hub.set_enabled(true);
	until(|| hub.read(|s| s.feed == Feed::Live) && hub.ops().is_some()).await;
	let key = hub.read(|s| s.session_for_call(&ada()).unwrap().key.clone());
	let last = |hub: &relay_ui::messaging::hub::MsgHub| {
		hub.read(|s| {
			s.transcript(&key)
				.and_then(|t| t.entries.last().map(|e| (e.delivery, e.seq)))
		})
	};
	a.dispatch(Intent::Send {
		session: key.clone(),
		text: "hello".into(),
	});
	until(|| last(&hub).is_some_and(|(d, _)| d == Delivery::Sending || d == Delivery::Delivered))
		.await;
	until(|| last(&hub).is_some_and(|(d, _)| d == Delivery::Delivered)).await;
	a.dispatch(Intent::Send {
		session: key.clone(),
		text: "/fail please".into(),
	});
	until(|| last(&hub).is_some_and(|(d, _)| d == Delivery::Failed)).await;
	let (_, seq) = last(&hub).unwrap();
	a.dispatch(Intent::Retry {
		session: key.clone(),
		seq,
	});
	until(|| last(&hub).is_some_and(|(d, _)| d == Delivery::Delivered)).await;
}

#[tokio::test]
async fn mock_download_progresses_to_completed() {
	let shell = Arc::new(FakeShell::default());
	*shell.path.lock().unwrap() = Some("/tmp/dl".into());
	let a = actions(shell.clone());
	let hub = a.shared.messages.clone();
	tokio::spawn(run(hub.clone(), shell));
	hub.set_enabled(true);
	until(|| hub.read(|s| s.feed == Feed::Live) && hub.ops().is_some()).await;
	let key = hub.read(|s| s.session_for_call(&ada()).unwrap().key.clone());
	a.dispatch(Intent::Download {
		session: key.clone(),
		offer: vec![1; 16],
	});
	until(|| hub.read(|s| s.transcript(&key).unwrap().entries.iter().any(|e| matches!(&e.body, Body::File(o) if o.offer_id == vec![1u8; 16] && o.status == OfferStatus::Completed)))).await;
}
