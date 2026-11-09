//! Self-update, as the shared UI sees it. The UI never talks to the network and does not depend
//! on the update crate (which brings an HTTP client and zip reader that the Android build must not
//! carry): a shell that can update itself says so through [`Shell::can_self_update`] and does the
//! work in [`Shell::check_for_update`] and [`Shell::install_update`]. Everything here is plain
//! data and the glue that keeps [`UpdateState`] current.
//!
//! The check is the only internet call the app makes. It runs at launch only when the user opted
//! in (`Shell::update_check_on_launch`, off by default), or when the user presses "Check for
//! updates". Nothing polls.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use relay_io::LockExt;
use std::sync::Mutex;

use crate::actions::Actions;
use crate::tr;

/// How long after the first frame the opt-in launch check waits, so it never sits on the start-up
/// critical path.
pub const LAUNCH_CHECK_DELAY: Duration = Duration::from_secs(4);

/// A newer release the shell found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOffer {
	/// The version to show (`0.2.0`).
	pub version: String,
	/// The release page, opened by "View release".
	pub release_url: String,
	/// The shell can download, verify and replace this installation. When false (a package
	/// install, a development build) only the release page is offered.
	pub can_install: bool,
}

/// How far an install is, reported by the shell while it works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateProgress {
	Downloading {
		received: u64,
		total: u64,
	},
	/// Everything arrived; the signature is being checked.
	Verifying,
	Installing,
}

/// Called by the shell from its worker thread; cheap and non-blocking.
pub type ProgressFn = Arc<dyn Fn(UpdateProgress) + Send + Sync>;

/// Why a check or an install did not finish, in words the UI can translate. The shell maps its
/// own errors to these; none carries a technical string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateFailure {
	/// GitHub could not be reached or answered badly.
	Offline,
	/// macOS runs the app from a quarantined copy; it must be moved first.
	Translocated,
	/// The app sits on a read-only volume (a disk image).
	ReadOnly,
	/// The folder the app is installed in cannot be written.
	NotWritable,
	/// A security check failed (signature, source, package). Nothing was installed.
	Rejected,
	/// The download was incomplete or unreadable. Nothing was installed.
	Damaged,
	/// Anything else; the current version still works.
	Other,
	/// The user quit while it ran.
	Cancelled,
}

impl UpdateFailure {
	/// The calm sentence for the sidebar and Settings. Security failures say the update was NOT
	/// installed and why; none offers a way around a check.
	pub fn text(self) -> &'static str {
		match self {
			Self::Offline => tr!("update-failed-offline"),
			Self::Translocated => tr!("update-failed-translocated"),
			Self::ReadOnly => tr!("update-failed-readonly"),
			Self::NotWritable => tr!("update-failed-not-writable"),
			Self::Rejected => tr!("update-failed-rejected"),
			Self::Damaged => tr!("update-failed-damaged"),
			Self::Other | Self::Cancelled => tr!("update-failed-other"),
		}
	}
}

/// Where the update stands. UI owned: it works with no daemon.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateState {
	#[default]
	Idle,
	Checking,
	/// A check the user asked for found nothing newer than this version.
	UpToDate(String),
	Available(UpdateOffer),
	Downloading {
		offer: UpdateOffer,
		received: u64,
		total: u64,
	},
	Verifying(UpdateOffer),
	Installing(UpdateOffer),
	/// `offer` is set when an install failed (Try again installs it again), else a check failed.
	Failed {
		failure: UpdateFailure,
		offer: Option<UpdateOffer>,
	},
}

impl UpdateState {
	/// A check or install is running.
	pub fn busy(&self) -> bool {
		matches!(
			self,
			Self::Checking | Self::Downloading { .. } | Self::Verifying(_) | Self::Installing(_)
		)
	}

	/// The release the sidebar button is about, if any.
	pub fn offer(&self) -> Option<&UpdateOffer> {
		match self {
			Self::Available(o)
			| Self::Verifying(o)
			| Self::Installing(o)
			| Self::Downloading { offer: o, .. } => Some(o),
			Self::Failed { offer, .. } => offer.as_ref(),
			_ => None,
		}
	}

	/// The whole-number percent downloaded, 0 while the size is unknown.
	pub fn percent(&self) -> u8 {
		match self {
			Self::Downloading {
				received, total, ..
			} if *total > 0 => u8::try_from((received.min(total) * 100) / total).unwrap_or(100),
			_ => 0,
		}
	}

	/// The progress wording that replaces the button label while an install runs.
	pub fn progress_label(&self) -> Option<String> {
		match self {
			Self::Downloading { .. } => Some(tr!("update-downloading", percent = self.percent())),
			Self::Verifying(_) => Some(tr!("update-verifying").to_owned()),
			Self::Installing(_) => Some(tr!("update-installing").to_owned()),
			_ => None,
		}
	}
}

/// The shared update state and the once-per-launch flag.
#[derive(Default)]
pub struct UpdateHub {
	state: Mutex<UpdateState>,
	launch_check_done: AtomicBool,
}

impl UpdateHub {
	pub fn state(&self) -> UpdateState {
		self.state.lock_unpoisoned().clone()
	}

	pub fn set(&self, state: UpdateState) {
		*self.state.lock_unpoisoned() = state;
	}

	/// Replace the state only when it is `from` (so a late result cannot overwrite a newer one).
	fn replace_if(&self, from: impl Fn(&UpdateState) -> bool, to: UpdateState) -> bool {
		let mut state = self.state.lock_unpoisoned();
		if from(&state) {
			*state = to;
			true
		} else {
			false
		}
	}

	/// The launch check has not run yet.
	pub fn launch_check_pending(&self) -> bool {
		!self.launch_check_done.load(Ordering::Acquire)
	}

	/// The first caller gets `true`, every later one `false`.
	pub fn take_launch_check(&self) -> bool {
		!self.launch_check_done.swap(true, Ordering::AcqRel)
	}
}

impl Actions {
	/// Ask the shell for the newest version. `manual` is the "Check for updates" button: it
	/// reports "up to date"; the launch check stays silent when there is nothing new.
	pub fn check_for_update(&self, manual: bool) {
		if !self.shell.can_self_update() {
			return;
		}
		let hub = &self.shared.updates;
		let started = hub.replace_if(
			|s| !s.busy() && !matches!(s, UpdateState::Available(_)),
			UpdateState::Checking,
		);
		if !started {
			return;
		}
		let (shared, shell) = (self.shared.clone(), self.shell.clone());
		self.runtime.spawn(async move {
			let next = match shell.check_for_update().await {
				Ok(Some(offer)) => UpdateState::Available(offer),
				Ok(None) if manual => UpdateState::UpToDate(relay_api::VERSION.to_owned()),
				Ok(None) => UpdateState::Idle,
				Err(failure) => UpdateState::Failed {
					failure,
					offer: None,
				},
			};
			shared.updates.set(next);
			shell.request_repaint();
		});
	}

	/// Whether an install may start now: an offer that can install, no call in progress.
	pub fn install_blocked_by_call(&self) -> bool {
		// The state the UI shows, so the button and this guard always agree.
		self.shared
			.state()
			.calls
			.iter()
			.any(|c| !c.phase.is_terminal())
	}

	/// Download, verify and install the offered release, then quit so the new version starts.
	/// Refused while a call is active (applying quits the app).
	pub fn install_update(&self) {
		let hub = &self.shared.updates;
		let Some(offer) = hub.state().offer().cloned().filter(|o| o.can_install) else {
			return;
		};
		if self.install_blocked_by_call() {
			return;
		}
		let from = |s: &UpdateState| {
			matches!(
				s,
				UpdateState::Available(_) | UpdateState::Failed { offer: Some(_), .. }
			)
		};
		let start = UpdateState::Downloading {
			offer: offer.clone(),
			received: 0,
			total: 0,
		};
		if !hub.replace_if(from, start) {
			return;
		}
		// The app quits right after, and nothing may bring the background service back while the
		// installation is replaced (on Windows it holds the files). A failure undoes this.
		self.shared.autostart.user_quit();
		let (actions, shell) = (self.clone(), self.shell.clone());
		let progress: ProgressFn = {
			let (shared, shell, offer) = (self.shared.clone(), self.shell.clone(), offer.clone());
			Arc::new(move |p| {
				let next = match p {
					UpdateProgress::Downloading { received, total } => UpdateState::Downloading {
						offer: offer.clone(),
						received,
						total,
					},
					UpdateProgress::Verifying => UpdateState::Verifying(offer.clone()),
					UpdateProgress::Installing => UpdateState::Installing(offer.clone()),
				};
				// Repaint only when the shown wording changes, not on every chunk.
				let changed = shared.updates.state_label() != label_of(&next);
				shared.updates.set(next);
				if changed {
					shell.request_repaint();
				}
			})
		};
		self.runtime.spawn(async move {
			match shell.install_update(offer.clone(), progress).await {
				Ok(()) => actions.quit_app(),
				Err(failure) => {
					actions.shared.autostart.user_start();
					let next = if failure == UpdateFailure::Cancelled {
						UpdateState::Available(offer)
					} else {
						UpdateState::Failed {
							failure,
							offer: Some(offer),
						}
					};
					actions.shared.updates.set(next);
					shell.request_repaint();
				}
			}
		});
	}

	/// Open the release page of the offer (the manual path, and the release notes).
	pub fn open_release(&self) {
		if let Some(offer) = self.shared.updates.state().offer() {
			self.shell.open_release(&offer.release_url);
		}
	}

	/// The sidebar button or the Settings button: install where possible, else open the page.
	pub fn run_update(&self) {
		let state = self.shared.updates.state();
		match state.offer() {
			Some(_) if matches!(state, UpdateState::Failed { .. }) => self.retry_update(),
			Some(offer) if offer.can_install => self.install_update(),
			Some(_) => self.open_release(),
			None => {}
		}
	}

	/// Try again after a failure: install again when an install failed, else check again.
	pub fn retry_update(&self) {
		match self.shared.updates.state() {
			UpdateState::Failed { offer: Some(_), .. } => self.install_update(),
			UpdateState::Failed { offer: None, .. } => self.check_for_update(true),
			_ => {}
		}
	}

	/// The opt-in check at launch: runs once per process, only when the user turned it on.
	pub fn launch_update_check(&self) {
		if !self.shared.updates.take_launch_check() {
			return;
		}
		if self.shell.can_self_update() && self.shell.update_check_on_launch() {
			self.check_for_update(false);
		}
	}
}

impl UpdateHub {
	fn state_label(&self) -> Option<String> {
		label_of(&self.state())
	}
}

/// What the shown progress reads, so a repaint is requested only when it changes.
fn label_of(state: &UpdateState) -> Option<String> {
	state.progress_label()
}

/// The status line under the Settings button.
pub fn status_text(state: &UpdateState) -> Option<String> {
	match state {
		UpdateState::Idle => None,
		UpdateState::Checking => Some(tr!("update-checking").to_owned()),
		UpdateState::UpToDate(version) => {
			Some(tr!("update-up-to-date", version = version.as_str()))
		}
		UpdateState::Available(offer) => {
			Some(tr!("update-available", version = offer.version.as_str()))
		}
		UpdateState::Failed { failure, .. } => Some(failure.text().to_owned()),
		other => other.progress_label(),
	}
}
