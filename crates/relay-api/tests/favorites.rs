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
