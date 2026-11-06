//! The live statistics stream of one call, for the connection details panel. The stream is
//! opened only while the panel is on screen (the UI asks every frame it is drawn) and dropped
//! the first frame it is not, so a hidden panel costs the engine nothing.

use std::sync::Mutex;

use relay_io::LockExt;
use tokio::task::JoinHandle;

use crate::actions::Actions;
use crate::controller::{CALL_ID_LEN, ControlError, Controller, decode_id};
use crate::engine::CallStatsSource;
use crate::model::CallStats;
use crate::tr;

/// The stream being read and the newest sample from it.
#[derive(Default)]
pub struct StatsFeed {
	open: Mutex<Option<(String, JoinHandle<()>)>>,
	latest: Mutex<Option<CallStats>>,
}

impl StatsFeed {
	/// The call whose stream is open (hex id).
	pub fn watching(&self) -> Option<String> {
		self.open.lock_unpoisoned().as_ref().map(|(id, _)| id.clone())
	}

	/// The newest sample of `call_id`, if one has arrived.
	pub fn latest(&self, call_id: &str) -> Option<CallStats> {
		self.latest
			.lock_unpoisoned()
			.clone()
			.filter(|s| s.call_id == call_id)
	}

	/// Closes the stream and forgets its samples.
	pub fn stop(&self) {
		if let Some((_, task)) = self.open.lock_unpoisoned().take() {
			task.abort();
		}
		*self.latest.lock_unpoisoned() = None;
	}
}

impl Controller {
	/// Opens the statistics stream of a call.
	pub async fn open_call_stats(
		&self,
		call_id: &str,
	) -> Result<Box<dyn CallStatsSource>, ControlError> {
		let id = decode_id(call_id, CALL_ID_LEN, tr!("err-bad-call-id"))?;
		let session = self.slot.get().ok_or(ControlError::NotConnected)?;
		Ok(session.ops.watch_call_stats(id).await?)
	}
}

impl Actions {
	/// Keeps the statistics of `call_id` flowing; call it every frame the panel is drawn.
	pub fn watch_stats(&self, call_id: &str) {
		let feed = &self.shared.stats;
		let mut open = feed.open.lock_unpoisoned();
		if open.as_ref().is_some_and(|(id, _)| id == call_id) {
			return;
		}
		if let Some((_, task)) = open.take() {
			task.abort();
		}
		*feed.latest.lock_unpoisoned() = None;
		let (controller, shared, shell) = (
			self.controller.clone(),
			self.shared.clone(),
			self.shell.clone(),
		);
		let id = call_id.to_owned();
		let task = self.runtime.spawn(async move {
			let Ok(mut source) = controller.open_call_stats(&id).await else {
				return;
			};
			while let Ok(Some(sample)) = source.next().await {
				*shared.stats.latest.lock_unpoisoned() = Some(sample);
				shell.request_repaint();
			}
		});
		*open = Some((call_id.to_owned(), task));
	}

	/// Closes the statistics stream (the panel is hidden or the call is over).
	pub fn stop_stats(&self) {
		self.shared.stats.stop();
	}
}
