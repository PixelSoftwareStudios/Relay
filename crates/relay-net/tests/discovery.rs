use std::collections::HashSet;
use std::net::IpAddr;

use mdns_sd::{ResolvedService, ServiceInfo};
use relay_net::AddressPolicy;
use relay_net::discovery::{
	AdvertMode, Advertisement, DiscoveredPeer, DiscoveryTable, FingerprintHint, InterfacePolicy,
	SERVICE_TYPE, parse_resolved, sanitize_display_name,
};
use relay_net::identity::Identity;

fn resolved(addrs: &[IpAddr], txt: &[(&str, &str)]) -> ResolvedService {
	let props: Vec<(String, String)> = txt
		.iter()
		.map(|(k, v)| ((*k).into(), (*v).into()))
		.collect();
	ServiceInfo::new(
		SERVICE_TYPE,
		"relay-test",
		"relay-test.local.",
		addrs,
		4242,
		&props[..],
	)
	.unwrap()
	.as_resolved_service()
}

fn id_hex() -> String {
	"11".repeat(16)
}

#[test]
fn display_names_are_bounded_and_stripped_of_misleading_characters() {
	assert_eq!(sanitize_display_name("Alice"), "Alice");
	assert_eq!(sanitize_display_name("Al\u{202E}ice\u{0007}\n"), "Alice");
	assert_eq!(sanitize_display_name("<b>Bob</b>"), "bBob/b");
	assert_eq!(sanitize_display_name(""), "Unnamed device");
	assert_eq!(sanitize_display_name("\u{200B}\u{200B}"), "Unnamed device");
	let long = sanitize_display_name(&"é".repeat(500));
	assert!(long.chars().count() <= 64 && long.len() <= 256);
	let emoji = sanitize_display_name(&"👩‍👩‍👧".repeat(100));
	assert!(emoji.chars().count() <= 64 && emoji.len() <= 256);
}

#[test]
fn valid_record_with_lan_address_becomes_a_candidate() {
	let fp = Identity::generate().unwrap().fingerprint();
	let hint = FingerprintHint::of(&fp);
	let hint_hex: String = fp.as_bytes()[..10]
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect();
	let record = resolved(
		&["192.168.1.50".parse().unwrap()],
		&[
			("v", "1"),
			("id", &id_hex()),
			("n", "Kitchen\u{202E}"),
			("fp", &hint_hex),
			("caps", "opus"),
		],
	);
	let peer = parse_resolved(&record, &AddressPolicy::lan_only()).unwrap();
	assert_eq!(peer.display_name, "Kitchen");
	assert_eq!(peer.addresses, vec!["192.168.1.50:4242".parse().unwrap()]);
	assert_eq!(peer.fingerprint_hint, Some(hint));
	assert!(peer.fingerprint_hint.unwrap().matches(&fp));
	assert!(
		!peer
			.fingerprint_hint
			.unwrap()
			.matches(&Identity::generate().unwrap().fingerprint())
	);
}

#[test]
fn public_and_loopback_addresses_are_dropped_and_a_record_with_none_left_is_ignored() {
	let id = id_hex();
	let txt = [("v", "1"), ("id", id.as_str()), ("n", "x")];
	let mixed = resolved(
		&["8.8.8.8".parse().unwrap(), "10.1.2.3".parse().unwrap()],
		&txt,
	);
	let peer = parse_resolved(&mixed, &AddressPolicy::lan_only()).unwrap();
	assert_eq!(peer.addresses.len(), 1);
	assert!(peer.addresses[0].ip().to_string().starts_with("10."));

	let public_only = resolved(&["8.8.8.8".parse().unwrap()], &txt);
	assert!(parse_resolved(&public_only, &AddressPolicy::lan_only()).is_none());
	let loop_only = resolved(&["127.0.0.1".parse().unwrap()], &txt);
	assert!(parse_resolved(&loop_only, &AddressPolicy::lan_only()).is_none());
}

#[test]
fn malformed_txt_records_are_ignored() {
	let addr: [IpAddr; 1] = ["192.168.1.2".parse().unwrap()];
	let policy = AddressPolicy::lan_only();
	assert!(
		parse_resolved(&resolved(&addr, &[("id", &id_hex())]), &policy).is_none(),
		"no version"
	);
	assert!(
		parse_resolved(&resolved(&addr, &[("v", "2"), ("id", &id_hex())]), &policy).is_none(),
		"other major"
	);
	assert!(
		parse_resolved(&resolved(&addr, &[("v", "1")]), &policy).is_none(),
		"no id"
	);
	assert!(
		parse_resolved(&resolved(&addr, &[("v", "1"), ("id", "xyz")]), &policy).is_none(),
		"bad id"
	);
	let huge = "n".repeat(240);
	let ok = parse_resolved(
		&resolved(
			&addr,
			&[
				("v", "1"),
				("id", &id_hex()),
				("n", &huge),
				("fp", "nothex"),
			],
		),
		&policy,
	);
	let peer = ok.unwrap();
	assert!(peer.display_name.chars().count() <= 64);
	assert_eq!(
		peer.fingerprint_hint, None,
		"bad hint ignored, never trusted"
	);
}

#[test]
fn table_is_bounded_and_keyed_by_instance_not_name() {
	let mk = |i: usize, name: &str| DiscoveredPeer {
		fullname: format!("inst{i}"),
		instance_id: [0; 16],
		display_name: name.into(),
		fingerprint_hint: None,
		capabilities: String::new(),
		addresses: vec!["192.168.1.1:1".parse().unwrap()],
		txt: Default::default(),
	};
	let mut table = DiscoveryTable::new();
	table.upsert(mk(0, "Same")).unwrap();
	table.upsert(mk(1, "Same")).unwrap();
	assert_eq!(table.len(), 2, "duplicate names are permitted");
	for i in 2..256 {
		table.upsert(mk(i, "x")).unwrap();
	}
	assert!(table.upsert(mk(256, "x")).is_err());
	table.upsert(mk(3, "renamed")).unwrap();
	assert_eq!(table.len(), 256);
	assert!(table.remove("inst3").is_some());
	table.upsert(mk(256, "x")).unwrap();
	let names: HashSet<_> = table.iter().map(|p| p.fullname.clone()).collect();
	assert_eq!(names.len(), table.len());
}

/// Needs working multicast on loopback or a LAN interface, so it is opt-in:
/// `cargo test -p relay-net --test discovery -- --ignored`.
#[tokio::test]
#[ignore = "requires multicast-capable network"]
async fn live_mdns_advertise_and_browse() {
	use relay_net::discovery::{Discovery, DiscoveryEvent};
	use std::time::Duration;

	let policy = InterfacePolicy::default();
	let mut host = Discovery::start(&policy, AddressPolicy::lan_only()).unwrap();
	let fingerprint = Identity::generate().unwrap().fingerprint();
	host.advertise(&Advertisement {
		instance_id: [9; 16],
		display_name: "Live test".into(),
		port: 4000,
		fingerprint,
		capabilities: "opus".into(),
		mode: Default::default(),
		extra_txt: Vec::new(),
	})
	.unwrap();
	let browser = Discovery::start(&policy, AddressPolicy::lan_only()).unwrap();
	let found = tokio::time::timeout(Duration::from_secs(10), async {
		loop {
			if let Some(DiscoveryEvent::Found(peer)) = browser.next_event().await
				&& peer.display_name == "Live test"
			{
				return peer;
			}
		}
	})
	.await
	.expect("advertisement not seen");
	assert!(found.fingerprint_hint.unwrap().matches(&fingerprint));
	host.withdraw().unwrap();
}

fn advertisement(mode: AdvertMode, extra: &[(&str, &str)]) -> Advertisement {
	Advertisement {
		instance_id: [3; 16],
		display_name: "Alice's laptop".into(),
		port: 4000,
		fingerprint: Identity::generate().unwrap().fingerprint(),
		capabilities: "opus".into(),
		mode,
		extra_txt: extra
			.iter()
			.map(|(k, v)| ((*k).into(), (*v).into()))
			.collect(),
	}
}

#[test]
fn hidden_mode_publishes_neither_name_nor_fingerprint_hint() {
	let open = advertisement(AdvertMode::Open, &[]).txt().unwrap();
	assert!(open.contains_key("n") && open.contains_key("fp"));
	let hidden = advertisement(AdvertMode::Hidden, &[("ct", "00ff")])
		.txt()
		.unwrap();
	assert!(!hidden.contains_key("n") && !hidden.contains_key("fp"));
	assert!(hidden.values().all(|v| !v.contains("Alice")));
	assert_eq!(hidden["ct"], "00ff");
}

#[test]
fn extra_txt_cannot_shadow_core_keys_or_exceed_bounds() {
	for bad in [
		("n", "x"),
		("fp", "x"),
		("v", "9"),
		("UPPER", "x"),
		("", "x"),
		("toolongkey", "x"),
		("ok", "é"),
	] {
		assert!(
			advertisement(AdvertMode::Open, &[bad]).txt().is_err(),
			"{bad:?}"
		);
	}
	assert!(
		advertisement(AdvertMode::Open, &[("ok", &"x".repeat(201))])
			.txt()
			.is_err()
	);
	assert!(
		advertisement(AdvertMode::Open, &[("a", "1"), ("a", "2")])
			.txt()
			.is_err()
	);
	let many: Vec<(String, String)> = (0..9).map(|i| (format!("k{i}"), "v".into())).collect();
	let mut ad = advertisement(AdvertMode::Open, &[]);
	ad.extra_txt = many;
	assert!(ad.txt().is_err());
}

#[test]
fn discovered_peers_expose_bounded_raw_txt_but_not_core_keys() {
	let record = resolved(
		&["192.168.1.50".parse().unwrap()],
		&[
			("v", "1"),
			("id", &id_hex()),
			("ce", "123"),
			("ct", "00ff00ff00ff00ff"),
			("junk", &"x".repeat(250)),
		],
	);
	let peer = parse_resolved(&record, &AddressPolicy::lan_only()).unwrap();
	assert_eq!(peer.txt.get("ce").map(String::as_str), Some("123"));
	assert!(!peer.txt.contains_key("id") && !peer.txt.contains_key("v"));
	assert!(
		!peer.txt.contains_key("junk"),
		"oversize values are dropped"
	);
}
