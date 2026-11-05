mod msgkit;

use std::path::PathBuf;
use std::sync::Arc;

use msgkit::*;
use relay_ui::actions::Actions;
use relay_ui::messaging::actions::Intent;
use relay_ui::messaging::engine::{MsgCommand, MsgEngine};

async fn rig(fail: Option<&str>) -> (Actions, Arc<FakeShell>, Arc<FakeEngine>) {
	let shell = Arc::new(FakeShell::default());
	let a = actions(shell.clone());
	let (engine, _tx) = FakeEngine::new(
		World {
			fail: fail.map(str::to_owned),
			..World::default()
		},
		0,
	);
	let conn = engine.connect().await.unwrap();
	a.shared.messages.set_ops(conn.ops);
	(a, shell, engine)
}

fn cmds(e: &FakeEngine) -> Vec<MsgCommand> {
	e.world.lock().unwrap().commands.clone()
}

#[tokio::test]
async fn download_asks_for_a_folder_then_accepts_into_it() {
	let (a, shell, engine) = rig(None).await;
	*shell.path.lock().unwrap() = Some(PathBuf::from("/tmp/downloads"));
	a.dispatch(Intent::Download {
		session: key(1),
		offer: vec![3; 16],
	});
	until(|| !cmds(&engine).is_empty()).await;
	assert_eq!(
		cmds(&engine),
		vec![MsgCommand::Respond {
			session: sid(1),
			offer: vec![3; 16],
			accept: true,
			destination_dir: "/tmp/downloads".into()
		}]
	);
}

#[tokio::test]
async fn a_cancelled_dialog_sends_nothing() {
	let (a, _shell, engine) = rig(None).await;
	a.dispatch(Intent::Download {
		session: key(1),
		offer: vec![3; 16],
	});
	a.dispatch(Intent::Export {
		session: key(1),
		suggested_name: "x.json".into(),
	});
	tokio::time::sleep(std::time::Duration::from_millis(100)).await;
	assert!(cmds(&engine).is_empty());
	assert!(a.shared.messages.read(|s| s.notice.is_none()));
}

#[tokio::test]
async fn a_relative_path_is_never_sent_to_the_daemon() {
	let (a, shell, engine) = rig(None).await;
	*shell.path.lock().unwrap() = Some(PathBuf::from("relative/file.txt"));
	a.dispatch(Intent::OfferFile { session: key(1) });
	until(|| a.shared.messages.read(|s| s.notice.is_some())).await;
	assert!(cmds(&engine).is_empty());
}

#[tokio::test]
async fn export_and_offer_send_absolute_paths() {
	let (a, shell, engine) = rig(None).await;
	*shell.path.lock().unwrap() = Some(PathBuf::from("/tmp/out.json"));
	a.dispatch(Intent::Export {
		session: key(1),
		suggested_name: "x.json".into(),
	});
	a.dispatch(Intent::OfferFile { session: key(1) });
	until(|| cmds(&engine).len() == 2).await;
	let c = cmds(&engine);
	assert!(c.iter().any(|c| matches!(c, MsgCommand::Export { destination, embed_images: false, .. } if destination == "/tmp/out.json")));
	assert!(
		c.iter()
			.any(|c| matches!(c, MsgCommand::OfferFile { path, .. } if path == "/tmp/out.json"))
	);
}

#[tokio::test]
async fn daemon_refusals_become_plain_notices_and_return_the_draft() {
	let (a, _s, _e) = rig(Some("SESSION_PERMISSION_OFF")).await;
	a.dispatch(Intent::Send {
		session: key(1),
		text: "hello".into(),
	});
	until(|| a.shared.messages.read(|s| s.notice.is_some())).await;
	let n = a.shared.messages.read(|s| s.notice.clone().unwrap());
	assert!(n.contains("turned off"), "{n}");
	assert_eq!(
		a.shared.messages.take_returned(),
		vec![(key(1), "hello".to_owned())]
	);
	a.dispatch(Intent::DismissNotice);
	assert!(a.shared.messages.read(|s| s.notice.is_none()));
}

#[tokio::test]
async fn copy_and_open_go_through_the_shell_and_bad_ids_are_refused() {
	let (a, shell, engine) = rig(None).await;
	a.dispatch(Intent::Copy("exact code".into()));
	assert_eq!(*shell.copied.lock().unwrap(), ["exact code"]);
	a.dispatch(Intent::Retry {
		session: "zz".into(),
		seq: 1,
	});
	assert!(a.shared.messages.read(|s| s.notice.is_some()));
	assert!(cmds(&engine).is_empty());
}

#[tokio::test]
async fn images_decode_off_thread_and_report_failures() {
	let (a, _s, _e) = rig(None).await;
	a.dispatch(Intent::DecodeImage {
		key: "k".into(),
		data: Arc::from(&b"junk"[..]),
	});
	until(|| {
		false || {
			let q = a.shared.messages.take_images();
			q.len() == 1 && q[0].1.is_err()
		}
	})
	.await;
}
