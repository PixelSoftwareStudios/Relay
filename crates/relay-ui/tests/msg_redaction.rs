//! Message text must never reach Debug or tracing output.
mod msgkit;

use std::io::Write;
use std::sync::{Arc, Mutex};

use msgkit::*;
use relay_ui::messaging::actions::Intent;
use relay_ui::messaging::engine::MsgCommand;
use relay_ui::messaging::hub::MsgHub;
use relay_ui::messaging::supervisor::supervise;

#[derive(Clone, Default)]
struct Buf(Arc<Mutex<Vec<u8>>>);

impl Write for Buf {
	fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
		self.0.lock().unwrap().extend_from_slice(b);
		Ok(b.len())
	}
	fn flush(&mut self) -> std::io::Result<()> {
		Ok(())
	}
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Buf {
	type Writer = Buf;
	fn make_writer(&'a self) -> Buf {
		self.clone()
	}
}

#[test]
fn debug_of_every_message_type_hides_content() {
	let mut s = relay_ui::messaging::state::MessagesState::default();
	s.load_list(&[summary(1, "Ada", true)], None);
	s.set_transcript(
		&key(1),
		&[
			text(1, false, &format!("see {SECRET} and `{SECRET}`")),
			file(2, 3, relay_api::pb::OfferState::Offered, false),
		],
	);
	let dump = format!("{s:?}");
	assert!(!dump.contains(SECRET), "{dump}");
	assert!(!dump.contains("report.pdf"), "file names are private too");
	let cmd = format!(
		"{:?}",
		MsgCommand::SendText {
			session: sid(1),
			text: SECRET.into()
		}
	);
	assert!(!cmd.contains(SECRET));
	let intent = format!(
		"{:?}",
		Intent::Send {
			session: key(1),
			text: SECRET.into()
		}
	);
	assert!(!intent.contains(SECRET));
	let cp = format!("{:?}", Intent::Copy(SECRET.into()));
	assert!(!cp.contains(SECRET));
}

#[tokio::test]
async fn trace_output_of_a_whole_messaging_run_has_no_content() {
	let buf = Buf::default();
	let sub = tracing_subscriber::fmt()
		.with_max_level(tracing::Level::TRACE)
		.with_writer(buf.clone())
		.finish();
	let _guard = tracing::subscriber::set_default(sub);

	let mut w = World {
		list: list(vec![summary(1, "Ada", true)], true),
		transcripts: vec![(
			1,
			vec![
				text(1, false, SECRET),
				file(2, 3, relay_api::pb::OfferState::Offered, false),
			],
		)],
		..World::default()
	};
	w.fail = Some("TEXT_INVALID".into());
	let (engine, tx) = FakeEngine::new(w, 1);
	let hub = Arc::new(MsgHub::default());
	let shell = Arc::new(FakeShell::default());
	let task = tokio::spawn(supervise(engine, hub.clone(), shell.clone()));
	hub.set_enabled(true);
	until(|| hub.read(|s| s.transcript(&key(1)).is_some())).await;
	tx[0]
		.send(Ok(Some(item_event(
			11,
			new_entry(1, text(3, true, SECRET)),
		))))
		.unwrap();
	until(|| hub.read(|s| s.transcript(&key(1)).is_some_and(|t| t.entries.len() == 3))).await;

	let a = actions(shell);
	let a = relay_ui::actions::Actions {
		shared: Arc::new(relay_ui::actions::Shared::default()),
		..a
	};
	// A failing send goes through the real action path and keeps the draft out of the notice.
	let hub2 = a.shared.messages.clone();
	let (engine2, _t) = FakeEngine::new(
		World {
			fail: Some("TEXT_INVALID".into()),
			..World::default()
		},
		0,
	);
	let conn = relay_ui::messaging::engine::MsgEngine::connect(engine2.as_ref())
		.await
		.unwrap();
	hub2.set_ops(conn.ops);
	a.dispatch(Intent::Send {
		session: key(1),
		text: SECRET.into(),
	});
	until(|| hub2.read(|s| s.notice.is_some())).await;
	let notice = hub2.read(|s| s.notice.clone().unwrap());
	assert!(!notice.contains(SECRET));
	assert_eq!(
		hub2.take_returned(),
		vec![(key(1), SECRET.to_owned())],
		"the draft is given back, not logged"
	);

	task.abort();
	let logged = String::from_utf8_lossy(&buf.0.lock().unwrap()).into_owned();
	assert!(
		!logged.contains(SECRET),
		"TRACE output leaked content:\n{logged}"
	);
	assert!(!logged.contains("report.pdf"));
}
