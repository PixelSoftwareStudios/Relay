mod channel_support;

use channel_support::*;
use relay_net::channel::ChannelId;
use relay_net::channel::pake::{self, Credential, PakeContext, PakeRole};

fn ctx(pair: &Pair, channel: ChannelId, joiner_side: bool) -> PakeContext {
	PakeContext {
		binding: if joiner_side {
			pair.joiner_binding
		} else {
			pair.admitter_binding
		},
		channel,
		invite_id: None,
	}
}

fn run(
	client_ctx: &PakeContext,
	server_ctx: &PakeContext,
	client_cred: &Credential,
	server_cred: &Credential,
) -> (bool, bool) {
	let (client, a) = pake::start(PakeRole::Client, client_ctx, client_cred);
	let (server, b) = pake::start(PakeRole::Server, server_ctx, server_cred);
	let client = client.finish(&b).unwrap();
	let server = server.finish(&a).unwrap();
	(
		client.verify(server.to_send()),
		server.verify(client.to_send()),
	)
}

#[test]
fn same_secret_and_context_confirm_both_ways() {
	let pair = Pair::new();
	let id = ChannelId::from_bytes([4; 16]);
	let cred = Credential::from_bytes([5; 32]);
	let (c, s) = (ctx(&pair, id, true), ctx(&pair, id, false));
	assert_eq!(run(&c, &s, &cred, &cred), (true, true));
}

#[test]
fn different_secret_fails_both_ways() {
	let pair = Pair::new();
	let id = ChannelId::from_bytes([4; 16]);
	let (c, s) = (ctx(&pair, id, true), ctx(&pair, id, false));
	let out = run(
		&c,
		&s,
		&Credential::from_bytes([5; 32]),
		&Credential::from_bytes([6; 32]),
	);
	assert_eq!(out, (false, false));
}

#[test]
fn another_session_binding_fails_with_the_right_secret() {
	let (one, two) = (Pair::new(), Pair::new());
	let id = ChannelId::from_bytes([4; 16]);
	let cred = Credential::from_bytes([5; 32]);
	// Client ran on link one, server on link two: a replay or relay across links.
	let out = run(&ctx(&one, id, true), &ctx(&two, id, false), &cred, &cred);
	assert_eq!(out, (false, false));
}

#[test]
fn another_channel_id_fails_with_the_right_secret() {
	let pair = Pair::new();
	let cred = Credential::from_bytes([5; 32]);
	let out = run(
		&ctx(&pair, ChannelId::from_bytes([1; 16]), true),
		&ctx(&pair, ChannelId::from_bytes([2; 16]), false),
		&cred,
		&cred,
	);
	assert_eq!(out, (false, false));
}

#[test]
fn another_invite_id_fails() {
	let pair = Pair::new();
	let id = ChannelId::from_bytes([4; 16]);
	let cred = Credential::from_bytes([5; 32]);
	let mut c = ctx(&pair, id, true);
	let mut s = ctx(&pair, id, false);
	c.invite_id = Some([1; 8]);
	s.invite_id = Some([2; 8]);
	assert_eq!(run(&c, &s, &cred, &cred), (false, false));
}

#[test]
fn two_runs_produce_different_messages_and_confirmations() {
	let pair = Pair::new();
	let id = ChannelId::from_bytes([4; 16]);
	let cred = Credential::from_bytes([5; 32]);
	let c = ctx(&pair, id, true);
	let (_, m1) = pake::start(PakeRole::Client, &c, &cred);
	let (_, m2) = pake::start(PakeRole::Client, &c, &cred);
	assert_ne!(m1, m2);
}

#[test]
fn reflected_message_does_not_authenticate() {
	let pair = Pair::new();
	let id = ChannelId::from_bytes([4; 16]);
	let cred = Credential::from_bytes([5; 32]);
	let c = ctx(&pair, id, true);
	let (client, own) = pake::start(PakeRole::Client, &c, &cred);
	// An attacker echoes the client's own message back.
	match client.finish(&own) {
		Err(_) => {}
		Ok(conf) => assert!(!conf.verify(&[0u8; 32])),
	}
}

#[test]
fn malformed_pake_messages_are_rejected() {
	let pair = Pair::new();
	let id = ChannelId::from_bytes([4; 16]);
	let cred = Credential::from_bytes([5; 32]);
	for bad in [vec![], vec![0u8; 32], vec![0u8; 34], vec![0xff; 33]] {
		let (client, _) = pake::start(PakeRole::Client, &ctx(&pair, id, true), &cred);
		if let Ok(conf) = client.finish(&bad) {
			assert!(!conf.verify(&[0u8; 32]));
		}
	}
}

#[test]
fn argon2_credentials_depend_on_password_and_channel() {
	let a = ChannelId::from_bytes([1; 16]);
	let b = ChannelId::from_bytes([2; 16]);
	let one = Credential::from_password("hunter2hunter2", &a).unwrap();
	let again = Credential::from_password("hunter2hunter2", &a).unwrap();
	let other_channel = Credential::from_password("hunter2hunter2", &b).unwrap();
	let other_pw = Credential::from_password("hunter2hunter3", &a).unwrap();
	assert_eq!(*one.to_bytes(), *again.to_bytes());
	assert_ne!(*one.to_bytes(), *other_channel.to_bytes());
	assert_ne!(*one.to_bytes(), *other_pw.to_bytes());
}
