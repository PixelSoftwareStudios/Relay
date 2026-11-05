mod common;

use common::*;
use relay_api::pb::{self, state_event::Event};
use relay_client::ChannelOptions;
use relay_desktop::channels::{JoinTarget, parse_join_target};
use relay_desktop::controller::ControlError;
use relay_desktop::engine::Command;
use relay_desktop::model::{Advert, AppState, Inbound};
use relay_desktop::viewmodel::peer_rows;

fn chan_hex() -> String {
	hex::encode(CHAN)
}

fn opts(password: bool) -> ChannelOptions {
	ChannelOptions {
		listed: true,
		password,
		invite_only: false,
	}
}

#[test]
fn join_targets_are_classified() {
	assert_eq!(
		parse_join_target(" RLYI1-abc "),
		Ok(JoinTarget::Invite("RLYI1-abc".into()))
	);
	assert_eq!(
		parse_join_target(&chan_hex()),
		Ok(JoinTarget::ChannelId(CHAN.to_vec()))
	);
	assert!(parse_join_target("RLYI1 with space").is_err());
	assert!(parse_join_target(&format!("RLYI1{}", "x".repeat(300))).is_err());
	assert!(parse_join_target("nonsense").is_err());
	assert!(parse_join_target("abcd").is_err());
}

#[tokio::test]
async fn create_validates_the_name_and_password() {
	let (ctl, _hub, ops) = controller_with(snapshot(1, vec![]), 0);
	assert!(ctl.create_channel("  ", opts(false), "").await.is_err());
	assert!(
		ctl.create_channel(&"n".repeat(33), opts(false), "")
			.await
			.is_err()
	);
	assert!(ctl.create_channel("dorm", opts(true), "").await.is_err());
	assert!(ops.recorded().is_empty());
	ctl.create_channel(" dorm ", opts(true), "pw")
		.await
		.unwrap();
	ctl.create_channel("open", opts(false), "ignored")
		.await
		.unwrap();
	let sent = ops.recorded();
	assert!(matches!(&sent[0], Command::CreateChannel { name, .. } if name == "dorm"));
	let Command::CreateChannel { password, .. } = &sent[1] else {
		panic!("wrong command");
	};
	assert_eq!(
		password.expose(),
		"",
		"a password is only sent when one is required"
	);
}

#[tokio::test]
async fn join_by_invite_or_by_id_and_password() {
	let (ctl, _hub, ops) = controller_with(snapshot(1, vec![]), 0);
	assert!(
		ctl.join_channel(&chan_hex(), "").await.is_err(),
		"id needs a password"
	);
	ctl.join_channel("RLYI1-token", "").await.unwrap();
	ctl.join_channel(&chan_hex(), "pw").await.unwrap();
	let sent = ops.recorded();
	let Command::JoinChannel {
		channel_id, invite, ..
	} = &sent[0]
	else {
		panic!("wrong command");
	};
	assert!(channel_id.is_empty());
	assert_eq!(invite.expose(), "RLYI1-token");
	let Command::JoinChannel {
		channel_id,
		password,
		..
	} = &sent[1]
	else {
		panic!("wrong command");
	};
	assert_eq!(channel_id, &CHAN.to_vec());
	assert_eq!(password.expose(), "pw");
}

#[tokio::test]
async fn secrets_never_reach_debug_output_of_commands_or_outcomes() {
	let (ctl, _hub, ops) = controller_with(snapshot_in_channel("owner"), 0);
	ctl.join_channel(&chan_hex(), "hunter2-secret")
		.await
		.unwrap();
	let token = ctl.create_invite(&chan_hex(), 3600, 1).await.unwrap();
	assert_eq!(token.expose(), "RLYI1-test-token");
	let text = format!("{:?} {token:?}", ops.recorded());
	assert!(
		!text.contains("hunter2") && !text.contains("test-token"),
		"{text}"
	);
}

#[tokio::test]
async fn invites_need_an_admin_role_and_sane_limits() {
	let (ctl, _hub, ops) = controller_with(snapshot_in_channel("member"), 0);
	assert!(ctl.create_invite(&chan_hex(), 3600, 1).await.is_err());
	let (ctl, _hub, ops2) = controller_with(snapshot_in_channel("admin"), 0);
	assert!(
		ctl.create_invite(&chan_hex(), 5, 1).await.is_err(),
		"ttl too short"
	);
	assert!(ctl.create_invite(&chan_hex(), 3600, 0).await.is_err());
	assert!(ctl.create_invite(&chan_hex(), 3600, 101).await.is_err());
	assert!(
		ctl.create_invite(&hex::encode([9u8; 16]), 3600, 1)
			.await
			.is_err(),
		"unknown channel"
	);
	assert!(ops.recorded().is_empty() && ops2.recorded().is_empty());
	assert!(ctl.create_invite(&chan_hex(), 3600, 5).await.is_ok());
}

#[tokio::test]
async fn revoke_requires_admin_and_a_proven_member() {
	let peer = hex::encode(PEER);
	let (ctl, _h, ops) = controller_with(snapshot_in_channel("member"), 0);
	assert!(ctl.revoke_member(&chan_hex(), &peer).await.is_err());
	let (ctl, hub, ops2) = controller_with(snapshot_in_channel("owner"), 0);
	assert!(ctl.revoke_member(&chan_hex(), "zz").await.is_err());
	hub.update(|s| s.peers[0].shared_channels.clear());
	assert!(
		ctl.revoke_member(&chan_hex(), &peer).await.is_err(),
		"not a proven member"
	);
	hub.update(|s| s.peers[0].shared_channels = vec![chan_hex()]);
	ctl.revoke_member(&chan_hex(), &peer).await.unwrap();
	assert!(ops.recorded().is_empty());
	assert_eq!(
		ops2.recorded(),
		vec![Command::RevokeMember {
			channel_id: CHAN.to_vec(),
			fingerprint: PEER.to_vec()
		}]
	);
}

#[tokio::test]
async fn leave_needs_a_known_channel() {
	let (ctl, _h, ops) = controller_with(snapshot_in_channel("member"), 0);
	assert!(ctl.leave_channel(&hex::encode([9u8; 16])).await.is_err());
	ctl.leave_channel(&chan_hex()).await.unwrap();
	assert_eq!(
		ops.recorded(),
		vec![Command::LeaveChannel {
			channel_id: CHAN.to_vec()
		}]
	);
}

#[tokio::test]
async fn privacy_changes_pass_through_and_empty_requests_are_refused() {
	let (ctl, _h, ops) = controller_with(snapshot(1, vec![]), 0);
	assert!(ctl.set_privacy(None, None).await.is_err());
	ctl.set_privacy(Some(Inbound::SharedChannelOnly), Some(Advert::Hidden))
		.await
		.unwrap();
	assert_eq!(
		ops.recorded(),
		vec![Command::SetPrivacy {
			inbound: Some(Inbound::SharedChannelOnly),
			advert: Some(Advert::Hidden)
		}]
	);
}

#[tokio::test]
async fn pasted_connection_strings_are_checked_locally_first() {
	let (ctl, _h, ops) = controller_with(snapshot(1, vec![]), 0);
	let err = ctl.start_call_to("http://evil").await.unwrap_err();
	assert!(matches!(err, ControlError::Invalid(_)));
	let good = format!("relay://192.168.1.2:7000#{}", "ab".repeat(32));
	ctl.start_call_to(&format!("  {good} ")).await.unwrap();
	assert_eq!(
		ops.recorded(),
		vec![Command::StartCallConnection { text: good }]
	);
}

#[tokio::test]
async fn connection_info_and_avatar_download_go_through_the_session() {
	let (ctl, _h, _ops) = controller_with(snapshot(1, vec![]), 0);
	let info = ctl.connection_info().await.unwrap();
	assert!(info.connection_string.starts_with("relay://"));
	assert!(ctl.avatar_png("not-hex").await.is_err());
	assert!(
		ctl.avatar_png(&"ab".repeat(16)).await.is_err(),
		"hash must be 32 bytes"
	);
	assert!(ctl.avatar_png(&"ab".repeat(32)).await.is_ok());
}

#[test]
fn channel_events_and_removals_keep_the_list_current() {
	let mut s = AppState::default();
	s.load_snapshot(&snapshot_in_channel("member"));
	assert_eq!(s.channels.len(), 1);
	s.apply_event(
		2,
		&pb::StateEvent {
			event: Some(Event::Channel(pb_channel("admin"))),
		},
	);
	assert_eq!(
		(s.channels.len(), s.channels[0].role.as_str()),
		(1, "admin")
	);
	s.apply_event(
		3,
		&pb::StateEvent {
			event: Some(Event::Removed(pb::Removed {
				id: CHAN.to_vec(),
				kind: "channel".into(),
			})),
		},
	);
	assert!(s.channels.is_empty());
}

#[test]
fn peer_rows_carry_channel_badges_and_revoke_rights() {
	let mut s = AppState {
		client_id: ME.to_vec(),
		..AppState::default()
	};
	s.load_snapshot(&snapshot_in_channel("member"));
	let row = &peer_rows(&s)[0];
	assert_eq!(row.badges, vec!["dorm".to_owned()]);
	assert!(row.revocable.is_empty(), "members cannot revoke");
	s.load_snapshot(&snapshot_in_channel("owner"));
	assert_eq!(
		peer_rows(&s)[0].revocable,
		vec![(chan_hex(), "dorm".to_owned())]
	);
	s.peers[0].shared_channels.clear();
	assert!(peer_rows(&s)[0].badges.is_empty());
}

#[test]
fn unspecified_privacy_values_read_conservatively() {
	let mut s = AppState::default();
	let mut snap = snapshot(1, vec![]);
	snap.settings = Some(pb::Settings::default());
	s.load_snapshot(&snap);
	let settings = s.settings.unwrap();
	assert_eq!(settings.inbound, Inbound::SharedChannelOnly);
	assert_eq!(settings.advert, Advert::Hidden);
}
