use relay_core::channel::{ChannelName, Policy};
use relay_net::channel::tag::{self, ChannelAdvert, blind, epoch_of, hidden_instance_id};
use relay_net::channel::{ChannelId, limits};

const INSTANCE: [u8; 16] = [7; 16];
const EPOCH: u64 = 500_000;

fn id(n: u8) -> ChannelId {
	ChannelId::from_bytes([n; 16])
}

fn name() -> ChannelName {
	ChannelName::new("Secret Club").unwrap()
}

const HIDDEN: Policy = Policy {
	listed: false,
	password: true,
	invite_only: false,
};

fn advert(ids: &[ChannelId], instance: &[u8; 16], epoch: u64, policy: &Policy) -> ChannelAdvert {
	let n = name();
	ChannelAdvert::build(ids.iter().map(|i| (i, &n, policy)), epoch, instance)
}

#[test]
fn members_recognise_their_channel_and_only_it() {
	let ad = advert(&[id(1), id(2)], &INSTANCE, EPOCH, &HIDDEN);
	let found = ad.recognise(&[id(1), id(2), id(3)], &INSTANCE, EPOCH);
	assert_eq!(found, vec![id(1), id(2)]);
}

#[test]
fn outsiders_recognise_nothing() {
	let ad = advert(&[id(1)], &INSTANCE, EPOCH, &HIDDEN);
	assert!(ad.recognise(&[id(9), id(10)], &INSTANCE, EPOCH).is_empty());
}

#[test]
fn recognition_tolerates_one_epoch_of_clock_skew_but_no_more() {
	let ad = advert(&[id(1)], &INSTANCE, EPOCH, &HIDDEN);
	assert!(!ad.recognise(&[id(1)], &INSTANCE, EPOCH + 1).is_empty());
	assert!(!ad.recognise(&[id(1)], &INSTANCE, EPOCH - 1).is_empty());
	assert!(ad.recognise(&[id(1)], &INSTANCE, EPOCH + 2).is_empty());
}

#[test]
fn tags_are_unlinkable_across_devices_and_epochs() {
	// Same channel: different device or hour gives a different tag, so a sniffer cannot tell
	// that two adverts belong to the same group.
	let base = blind(&id(1), EPOCH, &INSTANCE);
	assert_ne!(base, blind(&id(1), EPOCH, &[8; 16]));
	assert_ne!(base, blind(&id(1), EPOCH + 1, &INSTANCE));
	assert_ne!(base, blind(&id(2), EPOCH, &INSTANCE));
	assert_eq!(base, blind(&id(1), EPOCH, &INSTANCE));
}

#[test]
fn an_advert_replayed_from_another_instance_does_not_match() {
	let ad = advert(&[id(1)], &INSTANCE, EPOCH, &HIDDEN);
	// A forger re-publishing the tags under a different instance id is not recognised.
	assert!(ad.recognise(&[id(1)], &[8; 16], EPOCH).is_empty());
}

#[test]
fn tag_slots_hide_the_channel_count_coarsely_and_are_bounded() {
	for (n, slots) in [(0usize, 4usize), (1, 4), (4, 4), (5, 8), (8, 8), (12, 8)] {
		let ids: Vec<ChannelId> = (0..n as u8).map(|i| id(i + 1)).collect();
		let ad = advert(&ids, &INSTANCE, EPOCH, &HIDDEN);
		assert_eq!(ad.tags.len(), slots, "{n} channels");
	}
}

#[test]
fn decoys_differ_between_builds() {
	let a = advert(&[], &INSTANCE, EPOCH, &HIDDEN);
	let b = advert(&[], &INSTANCE, EPOCH, &HIDDEN);
	assert_ne!(a.tags, b.tags);
}

#[test]
fn hidden_channels_are_never_named_in_the_txt_record() {
	let ad = advert(&[id(1)], &INSTANCE, EPOCH, &HIDDEN);
	let txt = ad.to_txt();
	assert!(txt.iter().all(|(_, v)| !v.contains("Secret Club")));
	assert!(txt.iter().all(|(k, _)| k != "chl"));
	assert!(txt.iter().all(|(_, v)| v.len() <= 255));
}

#[test]
fn listed_channels_advertise_their_name_and_roundtrip() {
	let listed = Policy {
		listed: true,
		..HIDDEN
	};
	let ad = advert(&[id(1)], &INSTANCE, EPOCH, &listed);
	let txt = ad.to_txt();
	let get = |k: &str| {
		txt.iter()
			.find(|(key, _)| key == k)
			.map(|(_, v)| v.as_str())
	};
	let chl: Vec<&str> = txt
		.iter()
		.filter(|(k, _)| k == "chl")
		.map(|(_, v)| v.as_str())
		.collect();
	assert_eq!(chl.len(), 1);
	let parsed = ChannelAdvert::from_txt(get("ce"), get("ct"), &chl).unwrap();
	assert_eq!(parsed.tags, ad.tags);
	assert_eq!(parsed.listed.len(), 1);
	assert_eq!(parsed.listed[0].name.as_str(), "Secret Club");
	assert_eq!(parsed.epoch, EPOCH);
}

#[test]
fn listed_adverts_are_capped() {
	let listed = Policy {
		listed: true,
		..HIDDEN
	};
	let ids: Vec<ChannelId> = (1..=6).map(id).collect();
	let ad = advert(&ids, &INSTANCE, EPOCH, &listed);
	assert_eq!(ad.listed.len(), limits::LISTED_ADVERTISED_MAX);
}

#[test]
fn malformed_txt_is_rejected_not_trusted() {
	let ok = "0123456789abcdef";
	assert!(ChannelAdvert::from_txt(None, Some(ok), &[]).is_none());
	assert!(ChannelAdvert::from_txt(Some("x"), Some(ok), &[]).is_none());
	assert!(ChannelAdvert::from_txt(Some("5"), Some("zz"), &[]).is_none());
	assert!(ChannelAdvert::from_txt(Some("5"), Some("0123456789abcde"), &[]).is_none());
	let too_many = ok.repeat(9);
	assert!(ChannelAdvert::from_txt(Some("5"), Some(&too_many), &[]).is_none());
	// A bad listed entry is dropped while the rest survives.
	let parsed = ChannelAdvert::from_txt(Some("5"), Some(ok), &["nonsense", "zz:Name"]).unwrap();
	assert!(parsed.listed.is_empty());
}

#[test]
fn hidden_instance_ids_rotate_and_depend_on_the_secret() {
	let secret = [3u8; 32];
	assert_ne!(
		hidden_instance_id(&secret, 1),
		hidden_instance_id(&secret, 2)
	);
	assert_ne!(
		hidden_instance_id(&secret, 1),
		hidden_instance_id(&[4u8; 32], 1)
	);
	assert_eq!(
		hidden_instance_id(&secret, 1),
		hidden_instance_id(&secret, 1)
	);
}

#[test]
fn epoch_function_is_hourly() {
	assert_eq!(epoch_of(0), 0);
	assert_eq!(epoch_of(3599), 0);
	assert_eq!(epoch_of(3600), 1);
	let _ = tag::epoch_of;
}
