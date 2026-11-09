//! The desktop's side of self-update: `relay-update` behind a trait (so tests use a fake and never
//! touch the network), run on blocking worker threads so the UI thread never waits, with progress,
//! cancel on quit and the error mapping the UI translates. `relay-ui` does not depend on
//! `relay-update`; this is the only place that does.
//!
//! The only internet call the app makes is [`UpdateSource::check`]. It is made at launch only when
//! the user opted in, or when they press "Check for updates"; nothing here polls.

use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use relay_io::LockExt;
use relay_ui::updates::{ProgressFn, UpdateFailure, UpdateOffer, UpdateProgress};
use relay_update::{Available, Error, Plan, Updater};

/// What the service needs from the update crate. The real one talks to GitHub; a test fake
/// answers from memory.
pub trait UpdateSource: Send + Sync {
	/// Ask for the newest release (blocking). `None`: nothing newer.
	fn check(&self) -> Result<Option<UpdateOffer>, Error>;
	/// Download, verify and replace this installation with `offer` (blocking). `step` reports
	/// progress and returns `Break` to cancel; `before_apply` runs after the signature checked
	/// and right before the files are replaced.
	fn install(
		&self,
		offer: &UpdateOffer,
		step: &mut dyn FnMut(UpdateProgress) -> ControlFlow<()>,
		before_apply: &dyn Fn(),
	) -> Result<(), Error>;
}

/// The production source: `relay_update::Updater` and the release the last check found.
pub struct GithubSource {
	updater: Updater,
	found: Mutex<Option<Available>>,
}

impl GithubSource {
	/// For the running version, with leftovers of an earlier update cleaned up once. `None` when
	/// the version is not a semantic version (the build cannot update itself).
	pub fn detect(version: &str) -> Option<Self> {
		let updater = Updater::detect(version).ok()?;
		relay_update::cleanup_leftovers(updater.platform());
		Some(Self {
			updater,
			found: Mutex::new(None),
		})
	}
}

impl UpdateSource for GithubSource {
	fn check(&self) -> Result<Option<UpdateOffer>, Error> {
		let found = self.updater.check()?;
		let offer = found.as_ref().map(|a| UpdateOffer {
			version: a.version.to_string(),
			release_url: a.release_url.clone(),
			can_install: matches!(a.plan, Plan::InPlace(_)),
		});
		*self.found.lock_unpoisoned() = found;
		Ok(offer)
	}

	fn install(
		&self,
		offer: &UpdateOffer,
		step: &mut dyn FnMut(UpdateProgress) -> ControlFlow<()>,
		before_apply: &dyn Fn(),
	) -> Result<(), Error> {
		let available = self
			.found
			.lock_unpoisoned()
			.clone()
			.filter(|a| a.version.to_string() == offer.version)
			.ok_or_else(|| Error::BadRelease("no checked release matches".into()))?;
		let verified = self.updater.download(&available, &mut |p| {
			let flow = step(UpdateProgress::Downloading {
				received: p.received,
				total: p.total,
			});
			if p.received >= p.total {
				// All bytes are in; the signature is finished off before `download` returns.
				let _ = step(UpdateProgress::Verifying);
			}
			flow
		})?;
		let _ = step(UpdateProgress::Installing);
		before_apply();
		// The helper that starts the new version is already running; the caller quits now.
		let _relaunch = self.updater.apply(verified)?;
		Ok(())
	}
}

/// What the shell offers the UI: check, install, cancel.
pub struct UpdateService {
	source: Option<Arc<dyn UpdateSource>>,
	cancel: Arc<AtomicBool>,
	before_apply: Arc<dyn Fn() + Send + Sync>,
}

impl UpdateService {
	/// No self-update (a build that cannot tell its own version): the UI shows none of it.
	pub fn none() -> Self {
		Self::with_source(None, Arc::new(|| {}))
	}

	/// The real updater for this build. `before_apply` runs just before the files are replaced
	/// (the caller stops this app's own daemon there on Windows, where it holds them).
	pub fn system(before_apply: Arc<dyn Fn() + Send + Sync>) -> Self {
		let source = GithubSource::detect(env!("CARGO_PKG_VERSION"))
			.map(|source| Arc::new(source) as Arc<dyn UpdateSource>);
		Self::with_source(source, before_apply)
	}

	pub fn with_source(
		source: Option<Arc<dyn UpdateSource>>,
		before_apply: Arc<dyn Fn() + Send + Sync>,
	) -> Self {
		Self {
			source,
			cancel: Arc::default(),
			before_apply,
		}
	}

	pub fn can_update(&self) -> bool {
		self.source.is_some()
	}

	/// Stop a download in progress (the app is quitting).
	pub fn cancel(&self) {
		self.cancel.store(true, Ordering::Release);
	}

	/// The one internet call, on a worker thread.
	pub async fn check(&self) -> Result<Option<UpdateOffer>, UpdateFailure> {
		let Some(source) = self.source.clone() else {
			return Err(UpdateFailure::Other);
		};
		tokio::task::spawn_blocking(move || source.check())
			.await
			.map_err(|_| UpdateFailure::Other)?
			.map_err(|e| failure_of(&e))
	}

	/// Download, verify and install on a worker thread, reporting progress.
	pub async fn install(
		&self,
		offer: UpdateOffer,
		progress: ProgressFn,
	) -> Result<(), UpdateFailure> {
		let Some(source) = self.source.clone() else {
			return Err(UpdateFailure::Other);
		};
		self.cancel.store(false, Ordering::Release);
		let (cancel, before_apply) = (self.cancel.clone(), self.before_apply.clone());
		tokio::task::spawn_blocking(move || {
			let mut step = |p: UpdateProgress| {
				progress(p);
				if cancel.load(Ordering::Acquire) {
					ControlFlow::Break(())
				} else {
					ControlFlow::Continue(())
				}
			};
			source.install(&offer, &mut step, &*before_apply)
		})
		.await
		.map_err(|_| UpdateFailure::Other)?
		.map_err(|e| failure_of(&e))
	}
}

impl Drop for UpdateService {
	fn drop(&mut self) {
		self.cancel();
	}
}

/// Map an update error to what the UI says. Security failures are `Rejected` (the update was not
/// installed because it could not be trusted); none of them offers a way around the check. The
/// technical text goes to the log only.
pub fn failure_of(error: &Error) -> UpdateFailure {
	let failure = match error {
		Error::Network(_) | Error::HttpStatus(_) | Error::TooManyRedirects => {
			UpdateFailure::Offline
		}
		Error::ResponseTooLarge { .. } | Error::SizeMismatch { .. } | Error::Archive(_) => {
			UpdateFailure::Damaged
		}
		Error::HostNotAllowed(_)
		| Error::InsecureUrl
		| Error::SignatureMissing(_)
		| Error::SignatureInvalid(_)
		| Error::TrustedCommentMismatch { .. }
		| Error::PayloadChanged
		| Error::UnsafeArchive(_)
		| Error::BundleInvalid(_)
		| Error::CodesignFailed(_)
		| Error::TagMismatch { .. } => UpdateFailure::Rejected,
		Error::AppTranslocated => UpdateFailure::Translocated,
		Error::ReadOnlyVolume(_) => UpdateFailure::ReadOnly,
		Error::ParentNotWritable(_) => UpdateFailure::NotWritable,
		Error::Cancelled => UpdateFailure::Cancelled,
		_ => UpdateFailure::Other,
	};
	if failure != UpdateFailure::Cancelled {
		tracing::warn!(%error, ?failure, "update did not finish");
	}
	failure
}
