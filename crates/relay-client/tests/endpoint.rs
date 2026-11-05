use std::ffi::OsString;

use relay_api::Endpoint;
use relay_client::endpoint::{describe, from_vars};

fn vars<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
	move |name| {
		pairs
			.iter()
			.find(|(k, _)| *k == name)
			.map(|(_, v)| OsString::from(v))
	}
}

#[test]
fn nothing_set_defers_to_the_default() {
	assert!(from_vars(vars(&[])).is_none());
}

#[test]
fn tcp_profile_wins_over_socket() {
	let got = from_vars(vars(&[
		("RELAY_SOCKET", "/tmp/a.sock"),
		("RELAY_TCP_PROFILE", "/tmp/p"),
	]));
	match got {
		Some(Endpoint::LoopbackTcp { profile_path }) => {
			assert_eq!(profile_path, std::path::PathBuf::from("/tmp/p"));
			assert_eq!(
				describe(&Endpoint::LoopbackTcp { profile_path }),
				"tcp profile /tmp/p"
			);
		}
		other => panic!("unexpected {other:?}"),
	}
}

#[cfg(unix)]
#[test]
fn socket_variable_selects_a_unix_endpoint() {
	let got = from_vars(vars(&[("RELAY_SOCKET", "/tmp/a.sock")]));
	assert!(
		matches!(&got, Some(Endpoint::Unix(p)) if p.ends_with("a.sock")),
		"{got:?}"
	);
	assert_eq!(describe(&got.unwrap()), "/tmp/a.sock");
}
