#![cfg(unix)]
//! Named channels on whole engines: real loopback sockets and Noise links, a
//! real SQLite store, driven only through the local API.

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_media;

use relay_api::pb::{InboundPolicy, OperationState};
use relay_engine::channels::Clock;
use service_support::*;
use support_channels::*;

const PASSWORD: &str = "correct horse battery";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn invite_join_proof_and_leave() {
	let mut a = side(440.0).await;
	let mut b = side(1_700.0).await;
	pair(&mut a, &mut b, 1).await;

	// A creates an invite-only channel with a password and mints an invite.
	assert_applied(
		&a.cli
			.create_channel("Dorm 3", policy(false, true, true), PASSWORD)
			.await
			.expect("create"),
	);
	let channels = a.cli.list_channels().await.expect("list");
	assert_eq!(channels.len(), 1);
	assert_eq!(channels[0].role, "owner");
	assert_eq!(channels[0].name, "Dorm 3");
	let id = channels[0].channel_id.clone();
	let invite = a.cli.create_invite(&id, 0, 0).await.expect("invite");
	assert_applied(invite.operation.as_ref().expect("operation"));
	assert!(invite.invite.starts_with("RLYI1"));

	// A non-owner gets a FAILED operation, not an API error.
	let forged = b
		.cli
		.create_invite(&id, 0, 0)
		.await
		.expect("not a grpc error");
	assert_denied(forged.operation.as_ref().expect("operation"));
	assert!(forged.invite.is_empty());

	// Invite alone, or password alone, is not enough; both are uniformly denied.
	let wrong = b.cli.join_channel(&id, PASSWORD, "").await.expect("join");
	assert_denied(&wrong);
	let bad_password = b
		.cli
		.join_channel(&id, "wrong password here", &invite.invite)
		.await
		.expect("join");
	assert_denied(&bad_password);
	assert_eq!(wrong.problem, bad_password.problem, "no oracle");

	// Both secrets: B becomes a member and both sides prove it.
	let joined = b
		.cli
		.join_channel(&id, PASSWORD, &invite.invite)
		.await
		.expect("join");
	assert_applied(&joined);
	let seen = channel_of(&mut b.cli, &id).await.expect("listed on B");
	assert_eq!(seen.role, "member");
	assert!(seen.cert_expires_unix > 0);
	wait_shared(&mut a.cli, &id_of(&b), &id).await;
	wait_shared(&mut b.cli, &id_of(&a), &id).await;
	assert_eq!(channel_of(&mut a.cli, &id).await.unwrap().shared_peers, 1);

	// The invite was single use.
	let mut c = side(3_100.0).await;
	pair(&mut a, &mut c, 20).await;
	let spent = c
		.cli
		.join_channel(&id, PASSWORD, &invite.invite)
		.await
		.expect("join");
	assert_denied(&spent);
	assert!(shared_with(&mut a.cli, &id_of(&c)).await.is_empty());

	// Leaving deletes the membership; the owner stops counting B as shared.
	assert_applied(&b.cli.leave_channel(&id).await.expect("leave"));
	assert!(channel_of(&mut b.cli, &id).await.is_none());
	eventually!(
		shared_with(&mut b.cli, &id_of(&a))
			.await
			.is_empty()
			.then_some(())
	);
	let gone = b.cli.leave_channel(&id).await.expect("leave twice");
	assert_denied(&gone);

	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
	c.daemon.shutdown().await.expect("c stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn joiner_is_paced_and_names_are_validated() {
	let mut a = side(440.0).await;
	let mut b = side(1_700.0).await;
	pair(&mut a, &mut b, 1).await;
	let id = {
		assert_applied(
			&a.cli
				.create_channel("Lab", policy(false, true, false), PASSWORD)
				.await
				.expect("create"),
		);
		channel_of_name(&mut a.cli, "Lab").await
	};

	// Unknown channel and wrong password look the same.
	let unknown = b
		.cli
		.join_channel(&[9; 16], "whatever password", "")
		.await
		.expect("join");
	assert_denied(&unknown);
	let first = b
		.cli
		.join_channel(&id, "wrong password here", "")
		.await
		.expect("join");
	assert_denied(&first);
	assert_eq!(unknown.problem, first.problem);

	// Repeated failures lock this device out of that channel; only this differs.
	let mut last = first;
	for _ in 0..8 {
		last = b
			.cli
			.join_channel(&id, "wrong password here", "")
			.await
			.expect("join");
		if last
			.problem
			.as_ref()
			.is_some_and(|p| p.code != "CHANNEL_DENIED")
		{
			break;
		}
	}
	assert_failed(&last, "CHANNEL_LOCKED_OUT");
	assert!(last.problem.unwrap().retryable);

	// Name and limit rules.
	let bad = a
		.cli
		.create_channel("   ", policy(false, false, false), "")
		.await
		.expect("create");
	assert_failed(&bad, "CHANNEL_NAME_INVALID");
	let mismatch = a
		.cli
		.create_channel("Mismatch", policy(false, true, false), "")
		.await;
	assert!(
		mismatch.is_err(),
		"policy asks for a password but none given"
	);
	for n in 0..15 {
		assert_applied(
			&a.cli
				.create_channel(&format!("c{n}"), policy(false, false, false), "")
				.await
				.expect("create"),
		);
	}
	let over = a
		.cli
		.create_channel("one too many", policy(false, false, false), "")
		.await
		.expect("create");
	assert_failed(&over, "CHANNEL_LIMIT");

	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}

async fn channel_of_name(cli: &mut relay_api::RelayClient, name: &str) -> Vec<u8> {
	cli.list_channels()
		.await
		.expect("list")
		.into_iter()
		.find(|c| c.name == name)
		.expect("channel")
		.channel_id
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn revocation_removes_proof_and_operations_are_idempotent() {
	let mut a = side(440.0).await;
	let mut b = side(1_700.0).await;
	pair(&mut a, &mut b, 1).await;
	let id = create_open(&mut a.cli, "Open room").await;
	assert_applied(&b.cli.join_channel(&id, "", "").await.expect("join"));
	wait_shared(&mut a.cli, &id_of(&b), &id).await;

	// The same operation id with the same request returns the stored outcome;
	// with another request it is refused.
	let context = a.cli.new_mutation().expect("context");
	let request = relay_api::pb::LeaveChannelRequest {
		context: Some(context.clone()),
		channel_id: vec![7; 16],
	};
	let first = a
		.cli
		.raw()
		.leave_channel(request.clone())
		.await
		.expect("first");
	let again = a.cli.raw().leave_channel(request).await.expect("replay");
	assert_eq!(first.into_inner(), again.into_inner());
	let reused = relay_api::pb::LeaveChannelRequest {
		context: Some(context),
		channel_id: vec![8; 16],
	};
	let refused = a.cli.raw().leave_channel(reused).await.expect_err("reused");
	assert_eq!(refused.code(), tonic::Code::AlreadyExists);

	// Only an admitter may revoke.
	let by_member = b
		.cli
		.revoke_channel_member(&id, &id_of(&a))
		.await
		.expect("revoke");
	assert_denied(&by_member);
	let revoked = a
		.cli
		.revoke_channel_member(&id, &id_of(&b))
		.await
		.expect("revoke");
	assert_eq!(revoked.state, OperationState::Applied as i32, "{revoked:?}");
	assert!(shared_with(&mut a.cli, &id_of(&b)).await.is_empty());

	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shared_channel_only_screens_invites_by_proof() {
	let mut a = side(440.0).await;
	let mut b = side(1_700.0).await;
	let mut c = side(3_100.0).await;
	pair(&mut a, &mut b, 1).await;
	pair(&mut a, &mut c, 10).await;
	let id = create_open(&mut a.cli, "Floor").await;
	assert_applied(&b.cli.join_channel(&id, "", "").await.expect("join"));
	wait_shared(&mut a.cli, &id_of(&b), &id).await;

	// A only lets peers that proved a shared channel ring.
	assert_applied(
		&a.cli
			.set_privacy(Some(InboundPolicy::SharedChannelOnly), None)
			.await
			.expect("privacy"),
	);

	// C shares nothing: its invite never rings, and C sees a plain decline.
	let unproven = ring(&mut c, &a, 30).await;
	eventually!(
		call_in(&mut c.cli, &unproven)
			.await
			.filter(|call| call.state == relay_api::pb::CallState::Declined as i32)
	);
	assert!(call_in(&mut a.cli, &unproven).await.is_none());

	// B proved the channel: it rings, but only the user can accept it.
	let proven = ring(&mut b, &a, 40).await;
	eventually!(
		call_in(&mut a.cli, &proven)
			.await
			.filter(|call| call.state == relay_api::pb::CallState::IncomingRinging as i32)
	);

	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
	c.daemon.shutdown().await.expect("c stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn advert_follows_channels_epoch_and_hidden_mode() {
	let clock = Clock::starting_at(10 * 3600);
	let mut a = side_with(440.0, Some(clock.clone())).await;
	let hub = a.daemon.channels().expect("hub");
	assert!(
		hub.advert().extra_txt.is_empty(),
		"no channels, no channel TXT"
	);

	create_open(&mut a.cli, "Quiet").await;
	let first = hub.advert();
	let tags = |spec: &relay_engine::peers::AdvertSpec| {
		spec.extra_txt
			.iter()
			.find(|(k, _)| k == "ct")
			.map(|(_, v)| v.clone())
			.expect("tags")
	};
	assert!(first.extra_txt.iter().any(|(k, _)| k == "ce"));
	let before = tags(&first);

	// The next tag epoch re-advertises with other tags.
	clock.advance(std::time::Duration::from_secs(3600));
	hub.tick().await;
	let second = hub.advert();
	assert_ne!(tags(&second), before, "tags rotate with the epoch");

	// Hidden mode: a pseudonymous instance id that rotates with the epoch.
	assert_applied(
		&a.cli
			.set_privacy(None, Some(relay_api::pb::AdvertMode::Hidden))
			.await
			.expect("privacy"),
	);
	hub.tick().await;
	let hidden = hub.advert();
	assert!(hidden.hidden);
	assert_ne!(hidden.instance_id, second.instance_id);
	clock.advance(std::time::Duration::from_secs(3600));
	hub.tick().await;
	assert_ne!(hub.advert().instance_id, hidden.instance_id);

	// Leaving the channel stops advertising its tags at once.
	let id = channel_of_name(&mut a.cli, "Quiet").await;
	assert_applied(&a.cli.leave_channel(&id).await.expect("leave"));
	assert!(hub.advert().extra_txt.is_empty());
	a.daemon.shutdown().await.expect("a stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_recognised_tag_dials_the_member_and_a_proof_follows() {
	let mut a = side(440.0).await;
	let mut b = side(1_700.0).await;
	pair(&mut a, &mut b, 1).await;
	let id = create_open(&mut a.cli, "Reunion").await;
	assert_applied(&b.cli.join_channel(&id, "", "").await.expect("join"));
	wait_shared(&mut a.cli, &id_of(&b), &id).await;

	// B restarts on a new port; A has no link and no address for it.
	let mut b = b.restart().await;
	let b_id = id_of(&b);
	assert_eq!(
		channel_of(&mut b.cli, &id).await.map(|c| c.role),
		Some("member".to_owned()),
		"membership survived the restart"
	);
	eventually!(
		shared_with(&mut a.cli, &b_id)
			.await
			.is_empty()
			.then_some(())
	);

	// Discovery hands A B's advertisement: the blinded tag is recognised, A dials.
	let hub = a.daemon.channels().expect("hub");
	hub.on_event(relay_engine::peers::ChannelEvent::Advert(Box::new(
		b.discovered(),
	)))
	.await;
	wait_shared(&mut a.cli, &b_id, &id).await;

	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_forged_tag_from_an_untrusted_device_causes_no_dial_and_proves_nothing() {
	let mut a = side(440.0).await;
	let mut c = side(3_100.0).await;
	let id = create_open(&mut a.cli, "Sealed").await;
	assert_applied(
		&a.cli
			.set_privacy(Some(InboundPolicy::SharedChannelOnly), None)
			.await
			.expect("privacy"),
	);

	// Anyone who knows the channel id can forge a tag that names C.
	let epoch = relay_net::channel::tag::epoch_of(
		std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap()
			.as_secs(),
	);
	let instance = [5u8; 16];
	let channel = relay_net::channel::ChannelId::from_bytes(id.clone().try_into().unwrap());
	let name = relay_net::channel::ChannelName::new("Sealed").unwrap();
	let policy = relay_net::channel::Policy::default();
	let c_id = id_of(&c);
	let c_fingerprint =
		relay_net::identity::Fingerprint::from_bytes(c_id.clone().try_into().unwrap());
	let forged = relay_net::channel::ChannelAdvert::build(
		[(&channel, &name, &policy)],
		epoch,
		&instance,
		&c_fingerprint,
	);
	let mut found = c.discovered();
	found.instance_id = instance;
	found.txt = forged.to_txt().into_iter().collect();
	a.daemon
		.channels()
		.expect("hub")
		.on_event(relay_engine::peers::ChannelEvent::Advert(Box::new(found)))
		.await;

	// C is not pinned by A, so the tag is not even followed up: no link, no proof.
	settle(800).await;
	assert!(
		a.cli
			.snapshot()
			.await
			.expect("snapshot")
			.peers
			.iter()
			.all(|p| p.peer_id != c_id || !p.reachable),
		"an unpinned device named by a tag is never dialled"
	);
	assert!(shared_with(&mut a.cli, &c_id).await.is_empty());

	// So its call does not ring under SharedChannelOnly.
	let call = ring(&mut c, &a, 5).await;
	settle(500).await;
	assert!(call_in(&mut a.cli, &call).await.is_none());

	a.daemon.shutdown().await.expect("a stops");
	c.daemon.shutdown().await.expect("c stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_member_certificate_is_renewed_by_the_admitter_before_it_lapses() {
	let clock = Clock::starting_at(1_000_000);
	let mut a = side_with(440.0, Some(clock.clone())).await;
	let mut b = side_with(1_700.0, Some(clock.clone())).await;
	pair(&mut a, &mut b, 1).await;
	let id = create_open(&mut a.cli, "Renewal").await;
	assert_applied(&b.cli.join_channel(&id, "", "").await.expect("join"));
	wait_shared(&mut a.cli, &id_of(&b), &id).await;
	wait_shared(&mut b.cli, &id_of(&a), &id).await;
	let before = channel_of(&mut b.cli, &id).await.unwrap().cert_expires_unix;

	// Eleven of the fourteen days pass: the certificate is due for renewal.
	clock.advance(std::time::Duration::from_secs(11 * 86_400));
	a.daemon.channels().unwrap().tick().await;
	b.daemon.channels().unwrap().tick().await;
	eventually!(
		channel_of(&mut b.cli, &id)
			.await
			.filter(|c| c.cert_expires_unix > before + 5 * 86_400)
	);

	// Still a member in good standing on both sides.
	assert!(!shared_with(&mut a.cli, &id_of(&b)).await.is_empty());
	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}
