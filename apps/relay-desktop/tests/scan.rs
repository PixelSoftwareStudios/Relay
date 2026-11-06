//! The desktop has no camera scanner: the shared UI must not show Scan.

use std::sync::Arc;

use relay_desktop::host::DesktopShell;
use relay_desktop::login::{LoginBackend, LoginItem};
use relay_desktop::notify::{Dispatcher, Notifier};
use relay_desktop::options::DesktopOptions;
use relay_ui::shell::Shell;

struct Silent;

impl Notifier for Silent {
	fn incoming_call(&self, _: &str) -> Result<(), String> {
		Ok(())
	}
}

struct NoLogin;

impl LoginBackend for NoLogin {
	fn is_enabled(&self) -> Result<bool, String> {
		Ok(false)
	}
	fn enable(&self) -> Result<(), String> {
		Ok(())
	}
	fn disable(&self) -> Result<(), String> {
		Ok(())
	}
}

#[tokio::test]
async fn the_desktop_shell_cannot_scan_qr_codes() {
	let backend: Arc<dyn LoginBackend> = Arc::new(NoLogin);
	let options = DesktopOptions::new(
		None,
		Arc::new(Dispatcher::new(Arc::new(Silent))),
		LoginItem::new(Ok(backend)),
		Box::new(Vec::new),
	);
	let shell = DesktopShell::new(egui::Context::default(), Arc::new(options));
	assert!(!shell.can_scan_qr());
	assert_eq!(shell.scan_qr().await, Ok(None));
}
