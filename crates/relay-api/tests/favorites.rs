#![cfg(unix)]
//! `RelayClient::set_favorites` against the contract fake.

mod common;

use common::Harness;
use relay_api::pb::{OperationState, Peer};

#[tokio::test]
async fn the_ordered_list_is_sent_as_given_and_a_refusal_changes_nothing() {
	let h = Harness::start().await;
	let mut c = h.client().await;
	let order = vec![vec![3; 32], vec![1; 32], vec![2; 32]];
	let ok = c.set_favorites(&order).await.unwrap();
	assert_eq!(ok.state, OperationState::Applied as i32);
	assert_eq!(ok.operation_id.len(), 16);
	assert_eq!(*h.engine.settings.favorites.lock().unwrap(), order);

	let seven: Vec<Vec<u8>> = (1..=7u8).map(|n| vec![n; 32]).collect();
	let full = c.set_favorites(&seven).await.unwrap();
	assert_eq!(full.state, OperationState::Failed as i32);
	let problem = full.problem.unwrap();
	assert_eq!(problem.code, "INVALID_REQUEST");
	assert!(problem.message.contains("up to 6 favorites"));
	assert_eq!(*h.engine.settings.favorites.lock().unwrap(), order);

	let twice = c.set_favorites(&[vec![1; 32], vec![1; 32]]).await.unwrap();
	assert_eq!(twice.state, OperationState::Failed as i32);
}

#[test]
fn a_peer_carries_its_favorite_place() {
	use prost::Message;
	let peer = Peer {
		favorite: true,
		favorite_position: 4,
		..Peer::default()
	};
	let back = Peer::decode(peer.encode_to_vec().as_slice()).unwrap();
	assert!(back.favorite);
	assert_eq!(back.favorite_position, 4);
	assert!(!Peer::default().favorite);
}

#[tokio::test]
async fn auto_answer_is_sent_per_contact() {
	let h = Harness::start().await;
	let mut c = h.client().await;
	let ok = c.set_peer_auto_answer(&[7; 32], true).await.unwrap();
	assert_eq!(ok.state, OperationState::Applied as i32);
	let bad = c.set_peer_auto_answer(&[7; 3], true).await.unwrap();
	assert_eq!(bad.state, OperationState::Failed as i32);
	assert_eq!(
		*h.engine.settings.auto_answer.lock().unwrap(),
		vec![(vec![7; 32], true)]
	);
}

#[test]
fn the_auto_answer_fields_round_trip() {
	use prost::Message;
	use relay_api::pb::{AutoAnswered, Call, Settings};
	let peer = Peer {
		auto_answer: true,
		..Peer::default()
	};
	assert!(
		Peer::decode(peer.encode_to_vec().as_slice())
			.unwrap()
			.auto_answer
	);
	let call = Call {
		auto_answered: true,
		..Call::default()
	};
	assert!(
		Call::decode(call.encode_to_vec().as_slice())
			.unwrap()
			.auto_answered
	);
	let settings = Settings {
		background_available: true,
		call_alert: "silent".into(),
		auto_answer_enabled: true,
		..Settings::default()
	};
	let back = Settings::decode(settings.encode_to_vec().as_slice()).unwrap();
	assert_eq!(back.call_alert, "silent");
	assert!(back.background_available && back.auto_answer_enabled);
	let note = AutoAnswered {
		call_id: vec![1; 16],
		peer_id: vec![2; 32],
		peer_name: "A".into(),
	};
	assert_eq!(
		AutoAnswered::decode(note.encode_to_vec().as_slice()).unwrap(),
		note
	);
}
