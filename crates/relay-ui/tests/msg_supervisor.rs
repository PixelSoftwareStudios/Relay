mod msgkit;

use std::sync::Arc;

use msgkit::*;
use relay_api::SessionItem;
use relay_api::pb::{self, session_event::Event};
use relay_ui::engine::EngineError;
use relay_ui::messaging::hub::MsgHub;
use relay_ui::messaging::state::Feed;
use relay_ui::messaging::supervisor::supervise;

fn world() -> World {
	World {
		list: list(vec![summary(1, "Ada", true)], true),
		transcripts: vec![(1, vec![text(98, false, "hello"), text(99, true, "yo")])],
		..World::default()
	}
}

struct Rig {
	hub: Arc<MsgHub>,
	shell: Arc<FakeShell>,
	task: tokio::task::JoinHandle<()>,
}

fn start(engine: Arc<FakeEngine>) -> Rig {
	let hub = Arc::new(MsgHub::default());
	let shell = Arc::new(FakeShell::default());
	let task = tokio::spawn(supervise(engine, hub.clone(), shell.clone()));
	Rig { hub, shell, task }
}

fn count(hub: &MsgHub) -> usize {
	hub.read(|s| s.transcript(&key(1)).map_or(0, |t| t.entries.len()))
}

#[tokio::test]
async fn nothing_connects_until_the_user_opts_in() {
	let (engine, _tx) = FakeEngine::new(world(), 1);
	let rig = start(engine.clone());
	tokio::time::sleep(std::time::Duration::from_millis(50)).await;
	assert_eq!(*engine.connects.lock().unwrap(), 0);
	assert_eq!(rig.hub.read(|s| s.feed.clone()), Feed::Off);
	rig.hub.set_enabled(true);
	until(|| rig.hub.read(|s| s.feed == Feed::Live)).await;
	assert_eq!(count(&rig.hub), 2);
	assert!(rig.hub.ops().is_some());
	// The watch resumes at the revision of the list read.
	assert_eq!(engine.world.lock().unwrap().watch_after, vec![10]);
	rig.task.abort();
}

#[tokio::test]
async fn events_arrive_and_repaints_are_coalesced() {
	let (engine, tx) = FakeEngine::new(world(), 1);
	let rig = start(engine);
	rig.hub.set_enabled(true);
	until(|| rig.hub.read(|s| s.feed == Feed::Live)).await;
	let before = rig.shell.repaints.load(std::sync::atomic::Ordering::SeqCst);
	for n in 0..50u64 {
		tx[0]
			.send(Ok(Some(item_event(
				11 + n,
				new_entry(1, text(100 + n, false, "burst")),
			))))
			.unwrap();
	}
	until(|| count(&rig.hub) == 52).await;
	tokio::time::sleep(std::time::Duration::from_millis(120)).await;
	let used = rig.shell.repaints.load(std::sync::atomic::Ordering::SeqCst) - before;
	assert!((1..10).contains(&used), "{used} repaints for 50 events");
	rig.task.abort();
}

#[tokio::test]
async fn a_resync_rereads_list_and_transcript_then_watches_again() {
	let (engine, tx) = FakeEngine::new(world(), 2);
	let rig = start(engine.clone());
	rig.hub.set_enabled(true);
	until(|| rig.hub.read(|s| s.feed == Feed::Live)).await;
	{
		let mut w = engine.world.lock().unwrap();
		w.list.revision = 500;
		w.transcripts[0]
			.1
			.push(text(100, false, "missed while lagging"));
	}
	tx[0]
		.send(Ok(Some(SessionItem::Resync(pb::SessionResync {
			current_revision: 500,
			reason: "lag".into(),
		}))))
		.unwrap();
	until(|| count(&rig.hub) == 3).await;
	until(|| engine.world.lock().unwrap().watch_after.len() == 2).await;
	assert_eq!(engine.world.lock().unwrap().watch_after, vec![10, 500]);
	assert_eq!(*engine.connects.lock().unwrap(), 1, "same connection");
	tx[1]
		.send(Ok(Some(item_event(
			501,
			new_entry(1, text(101, false, "after")),
		))))
		.unwrap();
	until(|| count(&rig.hub) == 4).await;
	rig.task.abort();
}

#[tokio::test]
async fn a_broken_watch_reconnects_and_reloads() {
	let (engine, tx) = FakeEngine::new(world(), 2);
	let rig = start(engine.clone());
	rig.hub.set_enabled(true);
	until(|| rig.hub.read(|s| s.feed == Feed::Live)).await;
	tx[0].send(Err(EngineError::Broken("gap".into()))).unwrap();
	until(|| *engine.connects.lock().unwrap() == 2).await;
	until(|| rig.hub.read(|s| s.feed == Feed::Live) && count(&rig.hub) == 2).await;
	rig.task.abort();
}

#[tokio::test]
async fn denied_capability_is_reported_not_hidden() {
	let (engine, _tx) = FakeEngine::new(world(), 1);
	*engine.connect_error.lock().unwrap() = Some(EngineError::Rejected {
		code: "PermissionDenied".into(),
		message: String::new(),
	});
	let rig = start(engine);
	rig.hub.set_enabled(true);
	until(|| rig.hub.read(|s| s.feed == Feed::Denied)).await;
	rig.task.abort();
}

#[tokio::test]
async fn an_unreachable_daemon_is_offline_with_no_stale_transcripts() {
	let (engine, tx) = FakeEngine::new(world(), 1);
	let rig = start(engine.clone());
	rig.hub.set_enabled(true);
	until(|| rig.hub.read(|s| s.feed == Feed::Live)).await;
	*engine.connect_error.lock().unwrap() = Some(EngineError::Unavailable("down".into()));
	tx[0].send(Ok(None)).unwrap();
	until(|| matches!(rig.hub.read(|s| s.feed.clone()), Feed::Offline(_))).await;
	assert_eq!(count(&rig.hub), 0);
	assert!(rig.hub.ops().is_none());
	rig.task.abort();
}

#[tokio::test]
async fn opting_out_forgets_everything_at_once() {
	let (engine, _tx) = FakeEngine::new(world(), 1);
	let rig = start(engine);
	rig.hub.set_enabled(true);
	until(|| rig.hub.read(|s| s.feed == Feed::Live)).await;
	rig.hub.set_enabled(false);
	assert_eq!(rig.hub.read(|s| s.feed.clone()), Feed::Off);
	assert_eq!(count(&rig.hub), 0);
	assert!(rig.hub.ops().is_none());
	tokio::time::sleep(std::time::Duration::from_millis(100)).await;
	assert_eq!(
		rig.hub.read(|s| s.feed.clone()),
		Feed::Off,
		"the old connection cannot write back"
	);
	// Opening the panel does not undo a deliberate opt-out.
	rig.hub.open_default();
	assert!(!rig.hub.enabled());
	rig.task.abort();
}

#[tokio::test]
async fn opening_the_panel_opts_in_by_default() {
	let hub = MsgHub::default();
	hub.open_default();
	assert!(hub.enabled());
}

#[tokio::test]
async fn a_new_session_event_shows_up_with_its_entries() {
	let (engine, tx) = FakeEngine::new(world(), 1);
	let rig = start(engine);
	rig.hub.set_enabled(true);
	until(|| rig.hub.read(|s| s.feed == Feed::Live)).await;
	tx[0]
		.send(Ok(Some(item_event(
			11,
			event(Event::Session(summary(2, "Bo", true))),
		))))
		.unwrap();
	tx[0]
		.send(Ok(Some(item_event(
			12,
			new_entry(2, text(1, false, "hello")),
		))))
		.unwrap();
	until(|| {
		rig.hub
			.read(|s| s.transcript(&key(2)).is_some_and(|t| t.entries.len() == 1))
	})
	.await;
	let _ = pb::SessionState::Active;
	rig.task.abort();
}
