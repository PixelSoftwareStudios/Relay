#![cfg(unix)]
//! Channels listed on the network: what a device sees nearby, when it expires, and how a
//! clashing name is handled.

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_media;

use std::time::Duration;

use relay_api::pb::NearbyChannel;
use relay_engine::channels::{ChannelHub, Clock, LISTED_TTL};
use relay_engine::peers::ChannelEvent;
use service_support::assert_applied;
use support_channels::*;

fn listed_advert(
	template: &Side,
	instance: u8,
	channel: [u8; 16],
	name: &str,
) -> relay_net::discovery::DiscoveredPeer {
	use relay_net::channel::{ChannelAdvert, ChannelId, ChannelName, Policy};
	let policy = Policy {
		listed: true,
		..Policy::default()
	};
	let (id, name) = (
		ChannelId::from_bytes(channel),
		ChannelName::new(name).unwrap(),
	);
	let advert = ChannelAdvert::build(
		[(&id, &name, &policy)],
		relay_net::channel::tag::epoch_of(0),
		&[instance; 16],
		&relay_net::identity::Fingerprint::from_bytes([instance; 32]),
	);
	let mut found = template.discovered();
	found.instance_id = [instance; 16];
	found.txt = advert.to_txt().into_iter().collect();
	found
}

async fn hear(hub: &ChannelHub, found: relay_net::discovery::DiscoveredPeer) {
	hub.on_event(ChannelEvent::Advert(Box::new(found))).await;
}

fn row(name: &str, devices: u32, ambiguous: bool) -> NearbyChannel {
	NearbyChannel {
		name: name.to_owned(),
		devices,
		ambiguous,
	}
}

fn step(clock: &Clock, secs: u64) {
	clock.advance(Duration::from_secs(secs));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_listed_channel_of_another_device_shows_nearby_and_expires_in_silence() {
	let clock = Clock::starting_at(1_000_000);
	let mut a = side_with(440.0, Some(clock.clone())).await;
	let mut b = side_with(1_700.0, Some(clock.clone())).await;
	assert_applied(
		&b.cli
			.create_channel("Garage", policy(true, false, false), "")
			.await
			.expect("create"),
	);
	assert!(a.cli.snapshot().await.unwrap().nearby_channels.is_empty());

	let hub = a.daemon.channels().expect("hub");
	hear(&hub, b.discovered()).await;
	let nearby = a.cli.snapshot().await.unwrap().nearby_channels;
	assert_eq!(nearby, vec![row("Garage", 1, false)]);

	// Still advertised a minute later: the same record refreshes it.
	step(&clock, 60);
	hear(&hub, b.discovered()).await;
	step(&clock, 100);
	hub.tick().await;
	assert_eq!(a.cli.snapshot().await.unwrap().nearby_channels.len(), 1);

	// Then it goes quiet.
	step(&clock, LISTED_TTL.as_secs() + 1);
	hub.tick().await;
	assert!(a.cli.snapshot().await.unwrap().nearby_channels.is_empty());
	assert_eq!(
		hub.listed_id("Garage"),
		None,
		"an expired name cannot be joined"
	);
	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_channel_this_device_is_in_is_not_offered() {
	let mut a = side(440.0).await;
	let mut b = side(1_700.0).await;
	pair(&mut a, &mut b, 1).await;
	assert_applied(
		&b.cli
			.create_channel("Club", policy(true, false, false), "")
			.await
			.expect("create"),
	);
	// Our own advertisement, heard back, is never a nearby channel.
	assert_applied(
		&a.cli
			.create_channel("Mine", policy(true, false, false), "")
			.await
			.expect("create"),
	);
	let hub = a.daemon.channels().expect("hub");
	hear(&hub, a.discovered()).await;
	hear(&hub, b.discovered()).await;
	assert_eq!(
		a.cli.snapshot().await.unwrap().nearby_channels,
		vec![row("Club", 1, false)]
	);

	// Joining it removes it from the list.
	let id = b
		.cli
		.list_channels()
		.await
		.unwrap()
		.into_iter()
		.find(|c| c.name == "Club")
		.unwrap()
		.channel_id;
	assert_applied(&a.cli.join_channel(&id, "", "").await.expect("join"));
	eventually!(
		a.cli
			.snapshot()
			.await
			.unwrap()
			.nearby_channels
			.is_empty()
			.then_some(())
	);
	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn devices_are_counted_once_each_and_age_out_one_by_one() {
	let clock = Clock::starting_at(1_000_000);
	let a = side_with(440.0, Some(clock.clone())).await;
	let c = side_with(3_100.0, Some(clock.clone())).await;
	let hub = a.daemon.channels().expect("hub");
	let id = [7u8; 16];

	hear(&hub, listed_advert(&c, 10, id, "Lobby")).await;
	hear(&hub, listed_advert(&c, 10, id, "Lobby")).await;
	assert_eq!(hub.view().nearby, vec![row("Lobby", 1, false)]);
	step(&clock, 100);
	hear(&hub, listed_advert(&c, 11, id, "Lobby")).await;
	hear(&hub, listed_advert(&c, 12, id, "Lobby")).await;
	assert_eq!(hub.view().nearby, vec![row("Lobby", 3, false)]);

	// The first device is 130 s silent, the others 30 s.
	step(&clock, 30);
	hub.tick().await;
	assert_eq!(hub.view().nearby, vec![row("Lobby", 2, false)]);
	assert_eq!(hub.listed_id("Lobby").map(|i| *i.as_bytes()), Some(id));
	a.daemon.shutdown().await.expect("a stops");
	c.daemon.shutdown().await.expect("c stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_name_with_two_ids_is_ambiguous_and_never_joinable_by_name() {
	let clock = Clock::starting_at(1_000_000);
	let a = side_with(440.0, Some(clock.clone())).await;
	let c = side_with(3_100.0, Some(clock.clone())).await;
	let hub = a.daemon.channels().expect("hub");

	hear(&hub, listed_advert(&c, 10, [1; 16], "Lobby")).await;
	hear(&hub, listed_advert(&c, 11, [2; 16], "Lobby")).await;
	assert_eq!(hub.view().nearby, vec![row("Lobby", 2, true)]);
	assert_eq!(hub.listed_id("Lobby"), None);

	// Hearing the first id again does not clear it; silence does.
	hear(&hub, listed_advert(&c, 10, [1; 16], "Lobby")).await;
	assert!(hub.view().nearby[0].ambiguous);
	step(&clock, LISTED_TTL.as_secs() + 1);
	hub.tick().await;
	assert!(hub.view().nearby.is_empty());

	// A name equal to a channel this device is in is not offered as a join either.
	let mut a = a;
	assert_applied(
		&a.cli
			.create_channel("Mine", policy(false, false, false), "")
			.await
			.expect("create"),
	);
	hear(&hub, listed_advert(&c, 13, [9; 16], "Mine")).await;
	assert_eq!(hub.view().nearby, vec![row("Mine", 1, true)]);
	assert_eq!(hub.listed_id("Mine"), None);
	a.daemon.shutdown().await.expect("a stops");
	c.daemon.shutdown().await.expect("c stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn at_most_thirty_two_names_are_kept_and_expired_ones_make_room() {
	let clock = Clock::starting_at(1_000_000);
	let a = side_with(440.0, Some(clock.clone())).await;
	let c = side_with(3_100.0, Some(clock.clone())).await;
	let hub = a.daemon.channels().expect("hub");

	for n in 0..40u8 {
		hear(
			&hub,
			listed_advert(&c, 10, [n + 1; 16], &format!("Room {n:02}")),
		)
		.await;
	}
	let names: Vec<String> = hub.view().nearby.into_iter().map(|n| n.name).collect();
	assert_eq!(names.len(), 32);
	assert_eq!(names.first().map(String::as_str), Some("Room 00"));
	assert_eq!(names.last().map(String::as_str), Some("Room 31"));

	step(&clock, LISTED_TTL.as_secs() + 1);
	hear(&hub, listed_advert(&c, 10, [99; 16], "Fresh")).await;
	assert_eq!(hub.view().nearby, vec![row("Fresh", 1, false)]);
	a.daemon.shutdown().await.expect("a stops");
	c.daemon.shutdown().await.expect("c stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_event_stream_carries_each_change_of_the_nearby_list() {
	use relay_api::pb::state_event::Event;
	use relay_api::{WatchItem, WatchOptions};

	let clock = Clock::starting_at(1_000_000);
	let mut a = side_with(440.0, Some(clock.clone())).await;
	let c = side_with(3_100.0, Some(clock.clone())).await;
	let hub = a.daemon.channels().expect("hub");
	let mut watch = a.cli.watch(WatchOptions::default()).await.expect("watch");
	assert!(matches!(
		watch.next().await.unwrap(),
		Some(WatchItem::Snapshot(s)) if s.nearby_channels.is_empty()
	));

	async fn next_list(watch: &mut relay_api::StateWatch) -> Vec<NearbyChannel> {
		loop {
			let item = tokio::time::timeout(Duration::from_secs(5), watch.next())
				.await
				.expect("an event")
				.unwrap()
				.expect("open");
			if let WatchItem::Event {
				event:
					relay_api::pb::StateEvent {
						event: Some(Event::NearbyChannels(list)),
					},
				..
			} = item
			{
				return list.channels;
			}
		}
	}

	hear(&hub, listed_advert(&c, 10, [1; 16], "Lobby")).await;
	assert_eq!(next_list(&mut watch).await, vec![row("Lobby", 1, false)]);
	// The same device again changes nothing, so nothing is published; a second one does.
	hear(&hub, listed_advert(&c, 10, [1; 16], "Lobby")).await;
	hear(&hub, listed_advert(&c, 11, [1; 16], "Lobby")).await;
	assert_eq!(next_list(&mut watch).await, vec![row("Lobby", 2, false)]);

	step(&clock, LISTED_TTL.as_secs() + 1);
	hub.tick().await;
	assert!(next_list(&mut watch).await.is_empty());
	a.daemon.shutdown().await.expect("a stops");
	c.daemon.shutdown().await.expect("c stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_hidden_device_lists_what_it_hears_and_still_hides_itself() {
	let mut a = side(440.0).await;
	let c = side(3_100.0).await;
	assert_applied(
		&a.cli
			.set_privacy(None, Some(relay_api::pb::AdvertMode::Hidden))
			.await
			.expect("privacy"),
	);
	let hub = a.daemon.channels().expect("hub");
	hear(&hub, listed_advert(&c, 10, [1; 16], "Lobby")).await;
	assert_eq!(hub.view().nearby, vec![row("Lobby", 1, false)]);
	// Hearing channels adds nothing to what this device advertises.
	assert!(hub.advert().extra_txt.is_empty());
	a.daemon.shutdown().await.expect("a stops");
	c.daemon.shutdown().await.expect("c stops");
}
