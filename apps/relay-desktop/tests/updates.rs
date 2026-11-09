//! The desktop's self-update glue with a fake updater: nothing here touches the network, the
//! user's data directory or the real installation.

use std::ops::ControlFlow;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use relay_desktop::prefs::DesktopPrefs;
use relay_desktop::updates::{UpdateService, UpdateSource, failure_of};
use relay_ui::updates::{ProgressFn, UpdateFailure, UpdateOffer, UpdateProgress};
use relay_update::Error;

fn offer() -> UpdateOffer {
	UpdateOffer {
		version: "0.2.0".into(),
		release_url: "https://example.invalid/r".into(),
		can_install: true,
	}
}

#[derive(Default)]
struct Fake {
	checks: AtomicUsize,
	check_thread: Mutex<Option<std::thread::ThreadId>>,
	log: Mutex<Vec<String>>,
	fail: Mutex<Option<fn() -> Error>>,
	/// Keep reporting download progress this many times (a cancel stops it early).
	chunks: usize,
}

impl UpdateSource for Fake {
	fn check(&self) -> Result<Option<UpdateOffer>, Error> {
		self.checks.fetch_add(1, Ordering::SeqCst);
		*self.check_thread.lock().unwrap() = Some(std::thread::current().id());
		match *self.fail.lock().unwrap() {
			Some(make) => Err(make()),
			None => Ok(Some(offer())),
		}
	}

	fn install(
		&self,
		_offer: &UpdateOffer,
		step: &mut dyn FnMut(UpdateProgress) -> ControlFlow<()>,
		before_apply: &dyn Fn(),
	) -> Result<(), Error> {
		for n in 0..self.chunks {
			let p = UpdateProgress::Downloading {
				received: n as u64,
				total: self.chunks as u64,
			};
			if step(p).is_break() {
				self.log.lock().unwrap().push("cancelled".into());
				return Err(Error::Cancelled);
			}
		}
		let _ = step(UpdateProgress::Verifying);
		let _ = step(UpdateProgress::Installing);
		self.log.lock().unwrap().push("before".into());
		before_apply();
		self.log.lock().unwrap().push("apply".into());
		Ok(())
	}
}

fn service(fake: &Arc<Fake>, before: Arc<dyn Fn() + Send + Sync>) -> UpdateService {
	UpdateService::with_source(Some(fake.clone() as Arc<dyn UpdateSource>), before)
}

#[test]
fn no_source_means_no_self_update() {
	assert!(!UpdateService::none().can_update());
	let fake = Arc::new(Fake::default());
	assert!(service(&fake, Arc::new(|| {})).can_update());
}

#[tokio::test]
async fn a_check_runs_off_the_calling_thread_and_only_when_asked() {
	let fake = Arc::new(Fake::default());
	let svc = service(&fake, Arc::new(|| {}));
	assert_eq!(
		fake.checks.load(Ordering::SeqCst),
		0,
		"building asks nothing"
	);
	assert_eq!(svc.check().await.unwrap(), Some(offer()));
	assert_eq!(fake.checks.load(Ordering::SeqCst), 1);
	assert_ne!(
		*fake.check_thread.lock().unwrap(),
		Some(std::thread::current().id()),
		"a worker thread did the blocking call"
	);
}

#[tokio::test]
async fn an_install_reports_progress_and_runs_the_hook_right_before_the_swap() {
	let fake = Arc::new(Fake {
		chunks: 3,
		..Fake::default()
	});
	let hook_log = fake.clone();
	let svc = service(
		&fake,
		Arc::new(move || hook_log.log.lock().unwrap().push("hook".into())),
	);
	let seen = Arc::new(Mutex::new(Vec::new()));
	let sink = seen.clone();
	let progress: ProgressFn = Arc::new(move |p| sink.lock().unwrap().push(p));
	svc.install(offer(), progress).await.unwrap();
	let seen = seen.lock().unwrap();
	assert_eq!(seen.last(), Some(&UpdateProgress::Installing));
	assert!(seen.contains(&UpdateProgress::Verifying));
	assert_eq!(*fake.log.lock().unwrap(), ["before", "hook", "apply"]);
}

#[tokio::test]
async fn quitting_cancels_a_running_download() {
	let fake = Arc::new(Fake {
		chunks: 1000,
		..Fake::default()
	});
	let svc = Arc::new(service(&fake, Arc::new(|| {})));
	let worker = svc.clone();
	let progress: ProgressFn =
		Arc::new(|_| std::thread::sleep(std::time::Duration::from_millis(2)));
	let task = tokio::spawn(async move { worker.install(offer(), progress).await });
	tokio::time::sleep(std::time::Duration::from_millis(30)).await;
	svc.cancel();
	assert_eq!(task.await.unwrap(), Err(UpdateFailure::Cancelled));
	assert_eq!(*fake.log.lock().unwrap(), ["cancelled"]);
}

#[tokio::test]
async fn errors_reach_the_ui_as_failures_not_strings() {
	let fake = Arc::new(Fake::default());
	*fake.fail.lock().unwrap() = Some(|| Error::SignatureInvalid("bad".into()));
	let svc = service(&fake, Arc::new(|| {}));
	assert_eq!(svc.check().await, Err(UpdateFailure::Rejected));
}

#[test]
fn every_error_maps_to_the_right_failure() {
	use std::path::PathBuf;
	let p = || PathBuf::from("/x");
	let table: Vec<(Error, UpdateFailure)> = vec![
		(Error::Network("down".into()), UpdateFailure::Offline),
		(Error::HttpStatus(503), UpdateFailure::Offline),
		(Error::TooManyRedirects, UpdateFailure::Offline),
		(
			Error::SizeMismatch {
				expected: 2,
				received: 1,
			},
			UpdateFailure::Damaged,
		),
		(Error::ResponseTooLarge { limit: 1 }, UpdateFailure::Damaged),
		(Error::Archive("x".into()), UpdateFailure::Damaged),
		(Error::SignatureInvalid("x".into()), UpdateFailure::Rejected),
		(Error::SignatureMissing("x".into()), UpdateFailure::Rejected),
		(
			Error::TrustedCommentMismatch {
				expected: "a".into(),
				found: "b".into(),
			},
			UpdateFailure::Rejected,
		),
		(Error::PayloadChanged, UpdateFailure::Rejected),
		(
			Error::HostNotAllowed("evil.example".into()),
			UpdateFailure::Rejected,
		),
		(Error::InsecureUrl, UpdateFailure::Rejected),
		(Error::UnsafeArchive("..".into()), UpdateFailure::Rejected),
		(Error::BundleInvalid("x".into()), UpdateFailure::Rejected),
		(Error::CodesignFailed("x".into()), UpdateFailure::Rejected),
		(
			Error::TagMismatch { tag: "v9".into() },
			UpdateFailure::Rejected,
		),
		(Error::AppTranslocated, UpdateFailure::Translocated),
		(Error::ReadOnlyVolume(p()), UpdateFailure::ReadOnly),
		(Error::ParentNotWritable(p()), UpdateFailure::NotWritable),
		(Error::Cancelled, UpdateFailure::Cancelled),
		(Error::ManualOnly, UpdateFailure::Other),
		(Error::Helper("x".into()), UpdateFailure::Other),
	];
	for (error, want) in table {
		assert_eq!(failure_of(&error), want, "{error}");
	}
}

#[test]
fn the_launch_check_preference_is_off_by_default_and_round_trips() {
	assert!(!DesktopPrefs::default().check_updates_on_launch);
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("desktop.toml");
	assert!(
		!DesktopPrefs::load(&path).check_updates_on_launch,
		"no file"
	);
	let on = DesktopPrefs {
		check_updates_on_launch: true,
		..DesktopPrefs::default()
	};
	on.save(&path).unwrap();
	assert!(DesktopPrefs::load(&path).check_updates_on_launch);
}
