#![cfg(unix)]
//! Review F-11 (listed names), F-13 (proof expiry) and F-17 (invite secrecy).

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_media;

use relay_api::pb::CreateInviteResponse;
use relay_engine::channels::{Clock, Stored, StoredInvite};
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_listed_name_binds_to_the_first_id_and_a_clash_makes_it_ambiguous() {
	let a = side(440.0).await;
	let c = side(3_100.0).await;
	let hub = a.daemon.channels().expect("hub");
	let (real, fake) = ([1u8; 16], [2u8; 16]);

	hub.on_event(relay_engine::peers::ChannelEvent::Advert(Box::new(
		listed_advert(&c, 10, real, "Lobby"),
	)))
	.await;
	assert_eq!(
		hub.listed_id("Lobby").map(|id| *id.as_bytes()),
		Some(real),
		"the first id seen is bound to the name"
	);

	// A second device advertises the same name with its own id: the name stops resolving,
	// so "join by name" can no longer land on the attacker's channel.
	hub.on_event(relay_engine::peers::ChannelEvent::Advert(Box::new(
		listed_advert(&c, 11, fake, "Lobby"),
	)))
	.await;
	assert_eq!(hub.listed_id("Lobby"), None);

	// Advertising the same id again does not clear the conflict.
	hub.on_event(relay_engine::peers::ChannelEvent::Advert(Box::new(
		listed_advert(&c, 10, real, "Lobby"),
	)))
	.await;
	assert_eq!(hub.listed_id("Lobby"), None);
	a.daemon.shutdown().await.expect("a stops");
	c.daemon.shutdown().await.expect("c stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_member_whose_certificate_lapsed_stops_counting_as_shared() {
	let clock = Clock::starting_at(1_000_000);
	let mut a = side_with(440.0, Some(clock.clone())).await;
	let mut b = side_with(1_700.0, Some(clock.clone())).await;
	pair(&mut a, &mut b, 1).await;
	let id = create_open(&mut a.cli, "Lapse").await;
	assert!(
		relay_api::pb::OperationState::Applied as i32
			== b.cli.join_channel(&id, "", "").await.expect("join").state
	);
	wait_shared(&mut b.cli, &id_of(&a), &id).await;

	// Months pass with no tick on A, so nobody renews or re-proves. B's upkeep notices
	// that A's certificate has expired and stops counting A as a member.
	clock.advance(std::time::Duration::from_secs(60 * 86_400));
	b.daemon.channels().unwrap().tick().await;
	eventually!(
		shared_with(&mut b.cli, &id_of(&a))
			.await
			.is_empty()
			.then_some(())
	);
	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}

#[test]
fn a_stored_invite_is_never_printed_but_answers_a_retry() {
	let token = "relay-invite-SECRET-TOKEN-1234".to_owned();
	let invite = StoredInvite::new(token.clone(), 99);
	let stored = Stored {
		operation: relay_api::pb::Operation::default(),
		invite: Some(invite.clone()),
	};
	let printed = format!("{stored:?} {invite:?}");
	assert!(!printed.contains("SECRET"), "{printed}");
	assert!(printed.contains("redacted"));
	let again: CreateInviteResponse = invite.response();
	assert_eq!((again.invite, again.expires_unix), (token, 99));
}
