//! Nearby channels in the view state: snapshot, event, link loss and joining by name.

mod common;

use common::*;
use relay_api::pb::{self, state_event::Event};
use relay_ui::model::{AppState, NearbyChannel};

fn nearby(name: &str, devices: u32, ambiguous: bool) -> pb::NearbyChannel {
	pb::NearbyChannel {
		name: name.into(),
		devices,
		ambiguous,
		password: false,
	}
}

fn view(name: &str, devices: u32, ambiguous: bool) -> NearbyChannel {
	NearbyChannel {
		name: name.into(),
		devices,
		ambiguous,
		password: false,
	}
}

#[test]
fn snapshot_and_events_replace_the_nearby_list() {
	let mut s = AppState::default();
	let mut snap = snapshot(1, vec![]);
	snap.nearby_channels = vec![nearby("Garage", 2, false)];
	s.load_snapshot(&snap);
	assert_eq!(s.nearby_channels, vec![view("Garage", 2, false)]);

	s.apply_event(
		2,
		&pb::StateEvent {
			event: Some(Event::NearbyChannels(pb::NearbyChannels {
				channels: vec![nearby("Club", 1, false), nearby("Lobby", 3, true)],
			})),
		},
	);
	assert_eq!(
		s.nearby_channels,
		vec![view("Club", 1, false), view("Lobby", 3, true)]
	);
	assert_eq!(s.revision, 2);

	s.apply_event(
		3,
		&pb::StateEvent {
			event: Some(Event::NearbyChannels(pb::NearbyChannels::default())),
		},
	);
	assert!(s.nearby_channels.is_empty(), "an empty list clears it");

	s.nearby_channels = vec![view("Club", 1, false)];
	s.set_link_down(relay_ui::model::Link::Offline, None);
	assert!(s.nearby_channels.is_empty(), "a lost link forgets them");
}

#[tokio::test]
async fn join_by_name_validates_and_sends_the_name_alone() {
	let (ctl, _hub, ops) = controller_with(snapshot(1, vec![]), 0);
	assert!(ctl.join_by_name("  ", "").await.is_err());
	assert!(ctl.join_by_name(&"n".repeat(33), "").await.is_err());
	// A name that looks like an invite or an id is still a name here.
	ctl.join_by_name("RLYI1-lookalike", "").await.unwrap();
	ctl.join_by_name(&"ab".repeat(16), "").await.unwrap();
	let sent = ops.recorded();
	assert_eq!(sent.len(), 2);
	for command in &sent {
		let relay_ui::engine::Command::JoinChannel {
			channel_id, invite, ..
		} = command
		else {
			panic!("wrong command");
		};
		assert!(channel_id.is_empty() && invite.expose().is_empty());
	}
}
