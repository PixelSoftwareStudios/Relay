//! Tasks that carry adapter output into the control actor and surface what
//! needs the user's attention.

use std::sync::Arc;
use std::time::Duration;

use relay_api::pb::Problem;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::channels::ChannelHub;
use crate::media::{EventSink, MediaEvent, MediaSystem};
use crate::messaging::MessagingHandle;
use crate::peers::{PeerAdapter, PeerOutput, fingerprint};
use crate::service::ControlHandle;
use crate::service::ports::{DeviceControl, PeerControl, PeerInfo, PeerTrust};

const DEVICE_POLL: Duration = Duration::from_secs(1);
const DEVICE_FIRST_POLL: Duration = Duration::from_millis(50);

fn problem(code: &str, message: String, recovery: &str) -> Problem {
	Problem {
		code: code.to_owned(),
		message,
		retryable: false,
		recovery_action: recovery.to_owned(),
	}
}

/// What the user is told when a media thread panicked, hung or gave up.
fn worker_failed_message(failure: &crate::media::WorkerFailure) -> String {
	use crate::media::{FailureKind, WorkerId};
	if failure.permanent {
		return "Audio stopped working and could not be restarted. Restart Relay.".to_owned();
	}
	let part = match failure.worker {
		WorkerId::Capture => "Your microphone",
		WorkerId::Render => "Audio from the other side",
		WorkerId::Devices => "Audio device handling",
		WorkerId::Network => "The media connection",
	};
	let what = match failure.kind {
		FailureKind::Panic => "failed",
		FailureKind::Hang => "stopped",
		FailureKind::Socket => "lost its connection",
	};
	format!("{part} {what}. Relay restarted it.")
}

/// The warning code raised when the microphone cannot be opened.
const MICROPHONE_UNAVAILABLE: &str = "MICROPHONE_UNAVAILABLE";

/// What to tell the user when the microphone cannot be opened. On macOS the
/// first open triggers the system prompt; a refused or undetermined choice
/// shows up here, and only the user can change it.
fn microphone_unavailable_message(reason: &str) -> String {
	let hint = if cfg!(target_os = "macos") {
		"Allow microphone access for Relay in System Settings > Privacy & Security > Microphone, then try again."
	} else if cfg!(target_os = "android") {
		"Allow the microphone permission for Relay in the system settings, then try again."
	} else {
		"Check that a microphone is connected and not used exclusively by another program."
	};
	format!("The microphone could not be opened ({reason}). {hint}")
}

fn flagged(peer: relay_core::PeerId, name: String, trust: PeerTrust) -> PeerInfo {
	PeerInfo {
		display_name: name,
		trust,
		..PeerInfo::bare(peer)
	}
}

/// A media sink that never blocks the worker calling it, and the task that
/// delivers its events to the control actor.
pub(super) fn media_events(handle: ControlHandle) -> (EventSink, JoinHandle<()>) {
	let (tx, mut rx) = mpsc::unbounded_channel::<MediaEvent>();
	let sink: EventSink = Arc::new(move |event| {
		let _ = tx.send(event);
	});
	let task = tokio::spawn(async move {
		while let Some(event) = rx.recv().await {
			let sent = match event {
				MediaEvent::System(event) => handle.system(event).await,
				MediaEvent::Peer(event) => handle.peer(event).await,
				MediaEvent::OutputLost => {
					let warning = problem(
						"OUTPUT_DEVICE_LOST",
						"The playback device went away; remote audio is paused.".to_owned(),
						"select_output_device",
					);
					handle.attention(None, warning).await
				}
				MediaEvent::CaptureUnavailable(reason) => {
					let warning = problem(
						MICROPHONE_UNAVAILABLE,
						microphone_unavailable_message(&reason),
						"check_microphone_access",
					);
					handle.attention(None, warning).await
				}
				MediaEvent::CaptureOpened => handle.clear_warning(MICROPHONE_UNAVAILABLE).await,
				MediaEvent::WorkerFailed(failure) => {
					let warning = problem(
						"MEDIA_WORKER_FAILED",
						worker_failed_message(&failure),
						"restart_daemon",
					);
					handle.attention(None, warning).await
				}
			};
			if sent.is_err() {
				break;
			}
		}
	});
	(sink, task)
}

/// Routes peer-manager output to the domain, to media and to the API.
pub(super) fn peer_output(
	mut output: mpsc::Receiver<PeerOutput>,
	handle: ControlHandle,
	media: MediaSystem,
	adapter: Arc<PeerAdapter>,
	channels: ChannelHub,
	messaging: MessagingHandle,
) -> JoinHandle<()> {
	tokio::spawn(async move {
		while let Some(item) = output.recv().await {
			let refresh = matches!(
				item,
				PeerOutput::Profile { .. }
					| PeerOutput::AvatarReady { .. }
					| PeerOutput::Core(
						relay_core::PeerEvent::LinkUp(_) | relay_core::PeerEvent::LinkDown(_)
					)
			);
			let delivered = match item {
				// An invite from outside the user's chosen scope never reaches call state.
				PeerOutput::Core(event) if channels.screen(&event) => handle.peer(event).await,
				PeerOutput::Core(_) => Ok(()),
				PeerOutput::Media { peer, signal } => {
					media.handle_signal(peer, signal);
					Ok(())
				}
				PeerOutput::TrustRequired { peer, name, .. } => {
					let warning = problem(
						"TRUST_REQUIRED",
						format!(
							"`{name}` ({}) is waiting for a trust decision",
							fingerprint(peer).short()
						),
						"decide_peer_trust",
					);
					let info = flagged(peer, name, PeerTrust::Unknown);
					handle.attention(Some(info), warning).await
				}
				PeerOutput::KeyChanged {
					peer,
					name,
					previous,
				} => {
					let warning = problem(
						"PEER_KEY_CHANGED",
						format!(
							"`{name}` presented key {} instead of the pinned {}; the link was refused",
							fingerprint(peer).short(),
							fingerprint(previous).short()
						),
						"verify_out_of_band",
					);
					let info = PeerInfo {
						key_change: Some(previous),
						..flagged(peer, name, PeerTrust::KeyChanged)
					};
					handle.attention(Some(info), warning).await
				}
				PeerOutput::AvatarRejected { peer, reason, .. } => {
					let warning = problem(
						"AVATAR_REJECTED",
						format!(
							"avatar from {} refused: {reason}",
							fingerprint(peer).short()
						),
						"",
					);
					handle.attention(None, warning).await
				}
				// Channel traffic goes to the channel driver and nowhere else.
				PeerOutput::Channel(event) => {
					channels.on_event(event).await;
					Ok(())
				}
				// Session traffic goes to the messaging actor and nowhere else.
				PeerOutput::Session(event) => {
					messaging.peer_event(event).await;
					Ok(())
				}
				PeerOutput::Profile { .. } | PeerOutput::AvatarReady { .. } => Ok(()),
			};
			if delivered.is_err() {
				break;
			}
			if refresh && handle.peers_changed(adapter.peers()).await.is_err() {
				break;
			}
		}
	})
}

/// Publishes device list changes; the device thread only exposes a listing.
pub(super) fn device_watch(media: MediaSystem, handle: ControlHandle) -> JoinHandle<()> {
	tokio::spawn(async move {
		// Start from nothing: the control actor may have been built from an
		// earlier listing, and it only reports what actually differs.
		let mut known = Vec::new();
		let mut wait = DEVICE_FIRST_POLL;
		loop {
			tokio::time::sleep(wait).await;
			wait = DEVICE_POLL;
			let now = media.devices();
			if now != known {
				known.clone_from(&now);
				if handle.devices_changed(now).await.is_err() {
					break;
				}
			}
		}
	})
}
