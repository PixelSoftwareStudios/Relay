//! What the Messages panel can ask for. The panel draws while holding the
//! state lock and only records [`Intent`]s; they run afterwards through
//! [`Actions::dispatch`], on the shell's runtime, never on the UI thread.

use std::path::PathBuf;
use std::sync::Arc;

use super::engine::MsgCommand;
use super::image::decode_inline;
use super::model::SessionKey;
use super::rules::problem_text;
use crate::actions::Actions;
use crate::engine::EngineError;

#[derive(Clone, PartialEq)]
pub enum Intent {
	/// Allow or withdraw the app's `messages` registration.
	Enable(bool),
	/// Turn messaging on in the daemon (`SetMessagingSettings`).
	EnableDaemon,
	Send {
		session: SessionKey,
		text: String,
	},
	Retry {
		session: SessionKey,
		seq: u64,
	},
	Pin {
		session: SessionKey,
		outgoing: bool,
		seq: u64,
		pinned: bool,
	},
	Copy(String),
	/// Only ever created after the link confirmation dialog.
	OpenUrl(String),
	OfferFile {
		session: SessionKey,
	},
	Download {
		session: SessionKey,
		offer: Vec<u8>,
	},
	Reject {
		session: SessionKey,
		offer: Vec<u8>,
	},
	Cancel {
		session: SessionKey,
		offer: Vec<u8>,
	},
	RequestPreview {
		session: SessionKey,
		offer: Vec<u8>,
	},
	ApprovePreview {
		session: SessionKey,
		offer: Vec<u8>,
		approve: bool,
	},
	SetPerms {
		session: SessionKey,
		allow_messages: Option<bool>,
		allow_files: Option<bool>,
	},
	Clear {
		session: SessionKey,
	},
	Export {
		session: SessionKey,
		suggested_name: String,
	},
	DecodeImage {
		key: String,
		data: Arc<[u8]>,
	},
	DismissNotice,
}

impl std::fmt::Debug for Intent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		// Text, URLs and bytes are deliberately absent.
		write!(f, "Intent(<redacted>)")
	}
}

fn session_id(key: &str) -> Option<Vec<u8>> {
	hex::decode(key).ok().filter(|b| b.len() == 16)
}

fn failure_text(error: &EngineError) -> String {
	match error {
		EngineError::Rejected { code, .. } => problem_text(code),
		EngineError::Unavailable(_) | EngineError::Broken(_) => {
			"Lost the connection to the daemon.".to_owned()
		}
		EngineError::Timeout => "The daemon did not answer in time.".to_owned(),
	}
}

impl Actions {
	/// Run one intent. Safe to call from the UI thread: it only spawns.
	pub fn dispatch(&self, intent: Intent) {
		match intent {
			Intent::Enable(on) => {
				self.shared.messages.set_enabled(on);
				self.shell.request_repaint();
			}
			Intent::DismissNotice => self.shared.messages.set_notice(None),
			Intent::Copy(text) => self.shell.copy_text(&text),
			Intent::OpenUrl(url) => self.shell.open_url(&url),
			Intent::EnableDaemon => self.msg_run(MsgCommand::SetDaemonEnabled(true), None),
			Intent::DecodeImage { key, data } => self.decode_image(key, data),
			other => self.dispatch_session(other),
		}
	}

	fn dispatch_session(&self, intent: Intent) {
		let key_of = |i: &Intent| match i {
			Intent::Send { session, .. }
			| Intent::Retry { session, .. }
			| Intent::Pin { session, .. }
			| Intent::OfferFile { session }
			| Intent::Download { session, .. }
			| Intent::Reject { session, .. }
			| Intent::Cancel { session, .. }
			| Intent::RequestPreview { session, .. }
			| Intent::ApprovePreview { session, .. }
			| Intent::SetPerms { session, .. }
			| Intent::Clear { session }
			| Intent::Export { session, .. } => Some(session.clone()),
			_ => None,
		};
		let Some(key) = key_of(&intent) else { return };
		let Some(session) = session_id(&key) else {
			self.shared
				.messages
				.set_notice(Some("That session id is not valid.".to_owned()));
			return;
		};
		match intent {
			Intent::Send { text, .. } => {
				let draft = Some((key, text.clone()));
				self.msg_run(MsgCommand::SendText { session, text }, draft);
			}
			Intent::Retry { seq, .. } => self.msg_run(MsgCommand::Retry { session, seq }, None),
			Intent::Pin {
				outgoing,
				seq,
				pinned,
				..
			} => {
				self.msg_run(
					MsgCommand::Pin {
						session,
						outgoing,
						seq,
						pinned,
					},
					None,
				);
			}
			Intent::OfferFile { .. } => self.pick_and_offer(session),
			Intent::Download { offer, .. } => self.pick_and_download(session, offer),
			Intent::Reject { offer, .. } => self.msg_run(
				MsgCommand::Respond {
					session,
					offer,
					accept: false,
					destination_dir: String::new(),
				},
				None,
			),
			Intent::Cancel { offer, .. } => {
				self.msg_run(MsgCommand::Cancel { session, offer }, None)
			}
			Intent::RequestPreview { offer, .. } => {
				self.msg_run(MsgCommand::RequestPreview { session, offer }, None);
			}
			Intent::ApprovePreview { offer, approve, .. } => {
				self.msg_run(
					MsgCommand::ApprovePreview {
						session,
						offer,
						approve,
					},
					None,
				);
			}
			Intent::SetPerms {
				allow_messages,
				allow_files,
				..
			} => self.msg_run(
				MsgCommand::SetPermissions {
					session,
					allow_messages,
					allow_files,
				},
				None,
			),
			Intent::Clear { .. } => self.msg_run(MsgCommand::Clear { session }, None),
			Intent::Export { suggested_name, .. } => self.pick_and_export(session, suggested_name),
			_ => {}
		}
	}

	/// Send a command to the daemon and report a failure as a notice. A failed
	/// text send gives the draft back to the compose box.
	fn msg_run(&self, command: MsgCommand, draft: Option<(SessionKey, String)>) {
		let (hub, shell) = (self.shared.messages.clone(), self.shell.clone());
		self.runtime.spawn(async move {
			let result = match hub.ops() {
				Some(ops) => ops.run(command).await,
				None => Err(EngineError::Unavailable("not connected".into())),
			};
			finish(&hub, result, draft);
			shell.request_repaint();
		});
	}

	fn pick_and_offer(&self, session: Vec<u8>) {
		self.picked_path(
			|shell| async move { shell.pick_file("Offer a file").await },
			move |path| Some(MsgCommand::OfferFile { session, path }),
		);
	}

	fn pick_and_download(&self, session: Vec<u8>, offer: Vec<u8>) {
		self.picked_path(
			|shell| async move { shell.pick_directory("Save the file in").await },
			move |destination_dir| {
				Some(MsgCommand::Respond {
					session,
					offer,
					accept: true,
					destination_dir,
				})
			},
		);
	}

	fn pick_and_export(&self, session: Vec<u8>, name: String) {
		self.picked_path(
			|shell| async move { shell.save_as(&name).await },
			move |destination| {
				Some(MsgCommand::Export {
					session,
					embed_images: false,
					destination,
				})
			},
		);
	}

	/// Run a native picker, then the command built from the absolute path.
	fn picked_path<F, Fut>(
		&self,
		pick: F,
		build: impl FnOnce(String) -> Option<MsgCommand> + Send + 'static,
	) where
		F: FnOnce(Arc<dyn crate::shell::Shell>) -> Fut,
		Fut: std::future::Future<Output = Result<Option<PathBuf>, String>> + Send + 'static,
	{
		let (hub, shell) = (self.shared.messages.clone(), self.shell.clone());
		let picking = pick(shell.clone());
		self.runtime.spawn(async move {
			let notice = match picking.await {
				Ok(Some(path)) => match absolute_text(&path) {
					Ok(text) => {
						if let (Some(command), Some(ops)) = (build(text), hub.ops()) {
							let result = ops.run(command).await;
							finish(&hub, result, None);
						}
						None
					}
					Err(text) => Some(text),
				},
				Ok(None) => None,
				Err(text) => Some(text),
			};
			if notice.is_some() {
				hub.set_notice(notice);
			}
			shell.request_repaint();
		});
	}

	fn decode_image(&self, key: String, data: Arc<[u8]>) {
		let (hub, shell) = (self.shared.messages.clone(), self.shell.clone());
		self.runtime.spawn(async move {
			let result = tokio::task::spawn_blocking(move || decode_inline(&data))
				.await
				.map_err(|_| "The image could not be decoded.".to_owned())
				.and_then(|r| r.map_err(|e| e.to_string()));
			hub.push_image(key, result);
			shell.request_repaint();
		});
	}
}

/// The daemon needs a real absolute path; reject anything else here.
fn absolute_text(path: &std::path::Path) -> Result<String, String> {
	if !path.is_absolute() {
		return Err("The chosen location is not an absolute path.".to_owned());
	}
	path.to_str()
		.map(str::to_owned)
		.ok_or_else(|| "That path cannot be sent to the daemon (it is not valid text).".to_owned())
}

fn finish(
	hub: &super::hub::MsgHub,
	result: Result<(), EngineError>,
	draft: Option<(SessionKey, String)>,
) {
	match result {
		Ok(()) => hub.set_notice(None),
		Err(error) => {
			hub.set_notice(Some(failure_text(&error)));
			if let Some((session, text)) = draft {
				hub.return_draft(session, text);
			}
		}
	}
}
