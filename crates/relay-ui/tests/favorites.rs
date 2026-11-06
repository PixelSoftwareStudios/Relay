//! Favorites in the model: conversion, the view, and the controller's list edits.

mod common;

use common::*;
use relay_api::pb;
use relay_ui::controller::ControlError;
use relay_ui::engine::Command;
use relay_ui::viewmodel::favorites;

fn peer(n: u8, favorite: Option<u32>) -> pb::Peer {
	pb::Peer {
		peer_id: vec![n; 32],
		display_name: format!("Friend {n}"),
		trust: pb::TrustState::Verified as i32,
		reachable: true,
		favorite: favorite.is_some(),
		favorite_position: favorite.unwrap_or(0),
		..Default::default()
	}
}

fn world(favorites: &[u8], others: &[u8]) -> pb::Snapshot {
	let mut snap = snapshot(1, vec![]);
	snap.peers = favorites
		.iter()
		.enumerate()
		.map(|(i, n)| peer(*n, Some(i as u32)))
		.chain(others.iter().map(|n| peer(*n, None)))
		.collect();
	snap
}

fn sent(ops: &FakeOps) -> Vec<Vec<u8>> {
	match ops.recorded().last() {
		Some(Command::SetFavorites { peer_ids }) => {
			peer_ids.iter().map(|id| id[0..1].to_vec()).collect()
		}
		other => panic!("{other:?}"),
	}
}

fn hex_id(n: u8) -> String {
	hex::encode([n; 32])
}

#[test]
fn the_wire_flag_reaches_the_view_in_grid_order() {
	let (_ctl, hub, _ops) = controller_with(
		{
			let mut snap = world(&[3, 1], &[2]);
			// The wire order is not the grid order.
			snap.peers.swap(0, 1);
			snap
		},
		0,
	);
	let state = hub.state();
	let rows = favorites(&state);
	let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
	assert_eq!(names, ["Friend 3", "Friend 1"]);
	assert!(rows.iter().all(|r| r.favorite));
	assert!(
		!state
			.peers
			.iter()
			.find(|p| p.display_name == "Friend 2")
			.unwrap()
			.favorite
	);
}

#[tokio::test]
async fn adding_appends_and_removing_keeps_the_rest_in_order() {
	let (ctl, _hub, ops) = controller_with(world(&[1, 2], &[3]), 0);
	ctl.set_favorite(&hex_id(3), true).await.unwrap();
	assert_eq!(sent(&ops), [[1], [2], [3]]);
	ctl.set_favorite(&hex_id(1), false).await.unwrap();
	assert_eq!(sent(&ops), [[2]]);
	// Adding one that is already there moves it to the end, never duplicates it.
	ctl.set_favorite(&hex_id(1), true).await.unwrap();
	assert_eq!(sent(&ops), [[2], [1]]);
	assert!(matches!(
		ctl.set_favorite(&hex_id(9), true).await,
		Err(ControlError::Invalid(_))
	));
}

#[tokio::test]
async fn a_seventh_favorite_is_refused_before_anything_is_sent() {
	let (ctl, _hub, ops) = controller_with(world(&[1, 2, 3, 4, 5, 6], &[7]), 0);
	let Err(ControlError::Invalid(reason)) = ctl.set_favorite(&hex_id(7), true).await else {
		panic!("must be refused");
	};
	assert_eq!(reason, "You can have up to 6 favorites");
	assert!(ops.recorded().is_empty());
}

#[tokio::test]
async fn moving_reorders_and_clamps() {
	let (ctl, _hub, ops) = controller_with(world(&[1, 2, 3], &[4]), 0);
	ctl.move_favorite(&hex_id(3), 0).await.unwrap();
	assert_eq!(sent(&ops), [[3], [1], [2]]);
	ctl.move_favorite(&hex_id(1), 99).await.unwrap();
	assert_eq!(sent(&ops), [[2], [3], [1]]);
	assert!(
		ctl.move_favorite(&hex_id(4), 0).await.is_err(),
		"not a favorite"
	);
}
