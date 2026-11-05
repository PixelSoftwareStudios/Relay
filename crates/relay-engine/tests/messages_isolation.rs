#![cfg(unix)]
//! A in calls with B and C: a message for one call never reaches the other.

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_media;
mod support_messages;

use support_channels::{pair, side};
use support_messages::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn messages_never_cross_between_calls() {
	let mut a = side(440.0).await;
	let mut b = side(1_700.0).await;
	let mut c = side(3_100.0).await;
	pair(&mut a, &mut b, 1).await;
	pair(&mut a, &mut c, 10).await;
	pair(&mut b, &mut c, 20).await;
	connect(&mut a, &mut b, 30).await;
	connect(&mut a, &mut c, 40).await;
	let (mut ma, mut mb, mut mc) = (
		messenger(&a).await,
		messenger(&b).await,
		messenger(&c).await,
	);

	let sessions =
		eventually!(Some(ma.list_sessions().await.unwrap().sessions).filter(|s| s.len() == 2));
	let (a_to_b, a_to_c) = {
		let peer_b = support_channels::id_of(&b);
		let to_b = sessions
			.iter()
			.find(|s| s.peer_id == peer_b)
			.expect("A-B session");
		let to_c = sessions
			.iter()
			.find(|s| s.peer_id != peer_b)
			.expect("A-C session");
		(to_b.session_id.clone(), to_c.session_id.clone())
	};
	assert_ne!(a_to_b, a_to_c);
	let sb = active_session(&mut mb).await.session_id;
	let sc = active_session(&mut mc).await.session_id;

	assert_ok(
		&ma.send_session_message(&a_to_b, "canary-for-B")
			.await
			.unwrap(),
	);
	assert_ok(
		&ma.send_session_message(&a_to_c, "canary-for-C")
			.await
			.unwrap(),
	);
	assert_ok(
		&mb.send_session_message(&sb, "B-says-hi-to-A")
			.await
			.unwrap(),
	);
	assert_ok(
		&mc.send_session_message(&sc, "C-says-hi-to-A")
			.await
			.unwrap(),
	);

	let on_b = wait_transcript(&mut mb, &sb, |e| e.len() == 2).await;
	let on_c = wait_transcript(&mut mc, &sc, |e| e.len() == 2).await;
	let at_a_b = wait_transcript(&mut ma, &a_to_b, |e| e.len() == 2).await;
	let at_a_c = wait_transcript(&mut ma, &a_to_c, |e| e.len() == 2).await;
	let mut got = texts(&on_b);
	got.sort();
	assert_eq!(got, ["B-says-hi-to-A", "canary-for-B"]);
	let mut got = texts(&on_c);
	got.sort();
	assert_eq!(got, ["C-says-hi-to-A", "canary-for-C"]);
	let mut got = texts(&at_a_b);
	got.sort();
	assert_eq!(got, ["B-says-hi-to-A", "canary-for-B"]);
	let mut got = texts(&at_a_c);
	got.sort();
	assert_eq!(got, ["C-says-hi-to-A", "canary-for-C"]);

	// B and C talk to each other in their own call; A never sees it.
	connect(&mut b, &mut c, 50).await;
	let sessions_b =
		eventually!(Some(mb.list_sessions().await.unwrap().sessions).filter(|s| s.len() == 2));
	let peer_c = support_channels::id_of(&c);
	let b_to_c = sessions_b
		.iter()
		.find(|s| s.peer_id == peer_c)
		.expect("B-C")
		.session_id
		.clone();
	assert_ok(
		&mb.send_session_message(&b_to_c, "just-between-B-and-C")
			.await
			.unwrap(),
	);
	let c_sessions =
		eventually!(Some(mc.list_sessions().await.unwrap().sessions).filter(|s| s.len() == 2));
	let peer_b = support_channels::id_of(&b);
	let c_to_b = c_sessions
		.iter()
		.find(|s| s.peer_id == peer_b)
		.expect("C-B")
		.session_id
		.clone();
	wait_transcript(&mut mc, &c_to_b, |e| texts(e) == ["just-between-B-and-C"]).await;
	support_channels::settle(300).await;
	let mut seen = texts(&transcript(&mut ma, &a_to_b).await);
	seen.extend(texts(&transcript(&mut ma, &a_to_c).await));
	seen.extend(texts(&transcript(&mut mb, &sb).await));
	seen.extend(texts(&transcript(&mut mc, &sc).await));
	assert!(!seen.iter().any(|t| t.contains("just-between")), "{seen:?}");
	let mut got = texts(&transcript(&mut mb, &sb).await);
	got.sort();
	assert_eq!(
		got.len(),
		2,
		"the A-B transcript still holds exactly its two messages"
	);

	a.daemon.shutdown().await.unwrap();
	b.daemon.shutdown().await.unwrap();
	c.daemon.shutdown().await.unwrap();
}
