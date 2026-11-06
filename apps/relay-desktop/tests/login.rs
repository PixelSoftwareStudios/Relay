//! Start at login: off by default, and the status is whatever the OS backend says.

use std::sync::{Arc, Mutex};

use relay_desktop::login::{LoginBackend, LoginItem};
use relay_desktop::notify::{Dispatcher, Notifier};
use relay_desktop::options::DesktopOptions;
use relay_desktop::prefs::DesktopPrefs;
use relay_ui::host_options::{HostOptions, LoginStatus, login_text, notify_text};

#[derive(Default)]
struct Os {
	on: Mutex<bool>,
	refuse: Mutex<Option<String>>,
	unreadable: Mutex<Option<String>>,
}

impl LoginBackend for Os {
	fn is_enabled(&self) -> Result<bool, String> {
		match self.unreadable.lock().unwrap().clone() {
			Some(reason) => Err(reason),
			None => Ok(*self.on.lock().unwrap()),
		}
	}
	fn enable(&self) -> Result<(), String> {
		match self.refuse.lock().unwrap().clone() {
			Some(reason) => Err(reason),
			None => {
				*self.on.lock().unwrap() = true;
				Ok(())
			}
		}
	}
	fn disable(&self) -> Result<(), String> {
		*self.on.lock().unwrap() = false;
		Ok(())
	}
}

fn item(os: &Arc<Os>) -> LoginItem {
	LoginItem::new(Ok(os.clone() as Arc<dyn LoginBackend>))
}

#[test]
fn it_is_off_until_asked_and_the_status_follows_the_os() {
	let os = Arc::new(Os::default());
	let login = item(&os);
	assert_eq!(login.status(), LoginStatus::Off);
	login.set(true).unwrap();
	assert_eq!(login.status(), LoginStatus::On);
	*os.on.lock().unwrap() = false; // removed behind our back
	login.refresh();
	assert_eq!(login.status(), LoginStatus::Off);
	*os.on.lock().unwrap() = true;
	login.set(false).unwrap();
	assert_eq!(login.status(), LoginStatus::Off);
}

#[test]
fn a_refusal_is_returned_and_the_status_stays_the_truth() {
	let os = Arc::new(Os::default());
	let login = item(&os);
	*os.refuse.lock().unwrap() = Some("permission denied".into());
	assert_eq!(login.set(true).unwrap_err(), "permission denied");
	assert_eq!(login.status(), LoginStatus::Off);
	*os.unreadable.lock().unwrap() = Some("cannot read the agent".into());
	login.refresh();
	assert_eq!(
		login.status(),
		LoginStatus::Failed("cannot read the agent".into())
	);
	assert!(login_text(&login.status()).contains("cannot read the agent"));
}

#[test]
fn an_unsupported_system_says_why_and_cannot_be_changed() {
	let login = LoginItem::new(Err("no login items here".into()));
	assert_eq!(
		login.status(),
		LoginStatus::Unsupported("no login items here".into())
	);
	assert!(login.set(true).is_err());
	assert!(login_text(&login.status()).starts_with("Not available here"));
}

struct Silent;
impl Notifier for Silent {
	fn incoming_call(&self, _: &str) -> Result<(), String> {
		Err("blocked".into())
	}
}

#[test]
fn the_notification_switch_defaults_on_and_is_stored() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("cfg").join("desktop.toml");
	let dispatcher = Arc::new(Dispatcher::new(Arc::new(Silent)));
	let options = DesktopOptions::new(
		Some(path.clone()),
		dispatcher.clone(),
		item(&Arc::default()),
		Box::new(Vec::new),
	);
	assert!(options.notify_incoming(), "on by default");
	options.set_notify_incoming(false);
	assert!(!DesktopPrefs::load(&path).notify_incoming);
	let again = DesktopOptions::new(
		Some(path.clone()),
		dispatcher.clone(),
		item(&Arc::default()),
		Box::new(Vec::new),
	);
	assert!(!again.notify_incoming(), "remembered");

	assert_eq!(options.notify_failure(), None);
	dispatcher.deliver("Ada");
	assert_eq!(options.notify_failure().as_deref(), Some("blocked"));
	assert!(notify_text(true, options.notify_failure().as_deref()).contains("blocked"));
	assert!(notify_text(true, None).contains("never message text"));
}

#[test]
fn a_damaged_prefs_file_gives_the_defaults() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("desktop.toml");
	std::fs::write(&path, "notify_incoming = [oops").unwrap();
	assert!(DesktopPrefs::load(&path).notify_incoming);
	std::fs::write(&path, vec![b'#'; 64 * 1024]).unwrap();
	assert!(DesktopPrefs::load(&path).notify_incoming);
}
