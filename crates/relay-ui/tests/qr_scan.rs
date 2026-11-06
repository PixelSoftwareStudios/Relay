//! The Scan button under the QR code: hidden unless the shell can scan, and a
//! scanned string takes the same checks as a pasted one.

mod phone;

use std::sync::atomic::Ordering;
use std::time::Duration;

use phone::{Drawn, Phone};
use relay_client::connection::ConnectionStringProblem;
use relay_ui::i18n::Language;
use relay_ui::layout::Page;
use relay_ui::model::ConnectionView;
use relay_ui::qr_scan::{self, ScanError, ScanNote};
use relay_ui::shell::{BoxFuture, FilePicker, PickedFile, Shell};
use relay_ui::tr;

const OWN: &str =
	"relay://192.168.1.40:7000#abababababababababababababababababababababababababababababababab";
const OTHER: &str =
	"relay://10.0.0.9:7001#cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";

fn has(drawn: &Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l == text)
}

fn answer(phone: &Phone) {
	phone
		.actions
		.shared
		.inbox
		.set_connection(Ok(ConnectionView {
			addresses: vec!["192.168.1.40".into()],
			port: 7000,
			fingerprint: "ab".repeat(32),
			connection_string: OWN.into(),
		}));
}

fn people(desktop: bool, scanner: bool) -> Phone {
	let phone = if desktop {
		Phone::desktop(720.0, 1800.0, Language::En)
	} else {
		Phone::new(360.0, 2400.0, Language::En)
	};
	phone.shell.scanner.store(scanner, Ordering::SeqCst);
	let mut state = phone::busy_state();
	state.peers.clear();
	state.calls.clear();
	phone.show_state(&state);
	answer(&phone);
	phone
}

/// Press Scan with `reply` as the shell's answer and let the task finish.
async fn scan(phone: &mut Phone, reply: Result<Option<String>, ScanError>) -> Drawn {
	*phone.shell.scan_reply.lock().unwrap() = Some(reply);
	phone.page(Page::Connect);
	phone.click_label(tr!("connection-scan"));
	tokio::time::sleep(Duration::from_millis(30)).await;
	// The fake daemon refused the first fetch; the page keeps the seeded answer.
	answer(phone);
	phone.settle()
}

#[tokio::test]
async fn no_button_when_the_shell_cannot_scan() {
	for desktop in [true, false] {
		let mut phone = people(desktop, false);
		let drawn = phone.page(Page::Connect);
		assert!(
			drawn.labels.iter().any(|l| l.contains("Scan this code")),
			"the QR is drawn"
		);
		assert!(!has(&drawn, tr!("connection-scan")), "desktop={desktop}");
	}
}

#[tokio::test]
async fn the_button_asks_the_shell_to_scan() {
	for desktop in [true, false] {
		let mut phone = people(desktop, true);
		let drawn = phone.page(Page::Connect);
		assert!(has(&drawn, tr!("connection-scan")), "desktop={desktop}");
		assert_eq!(phone.shell.scans.load(Ordering::SeqCst), 0);
		scan(&mut phone, Ok(None)).await;
		assert_eq!(phone.shell.scans.load(Ordering::SeqCst), 1);
	}
}

#[tokio::test]
async fn a_scanned_address_fills_the_field_and_calls_nobody() {
	let mut phone = people(false, true);
	let drawn = scan(&mut phone, Ok(Some(format!(" {OTHER}\n")))).await;
	assert!(has(&drawn, tr!("scan-filled")), "{:?}", drawn.labels);
	assert!(has(&drawn, OTHER), "the address is in the field");
	assert_eq!(phone.shell.copied.lock().unwrap().len(), 0);
}

#[tokio::test]
async fn a_cancelled_scan_changes_nothing() {
	let mut phone = people(false, true);
	let drawn = scan(&mut phone, Ok(None)).await;
	for key in [
		"scan-filled",
		"scan-wrong",
		"scan-failed",
		"scan-camera-denied",
	] {
		assert!(
			!drawn
				.labels
				.iter()
				.any(|l| l.contains(relay_ui::i18n::t(key)))
		);
	}
	assert!(
		has(&drawn, tr!("connection-scan")),
		"the button is usable again"
	);
}

#[tokio::test]
async fn a_code_that_is_not_relay_is_refused_in_plain_words() {
	let mut phone = people(false, true);
	let drawn = scan(&mut phone, Ok(Some("https://example.com/x".into()))).await;
	assert!(has(&drawn, tr!("scan-wrong")), "{:?}", drawn.labels);
	assert!(
		!has(&drawn, "https://example.com/x"),
		"not put in the field"
	);
	assert!(!has(&drawn, tr!("scan-filled")));
}

#[tokio::test]
async fn a_blocked_camera_explains_and_opens_the_settings() {
	let mut phone = people(false, true);
	let drawn = scan(&mut phone, Err(ScanError::CameraBlocked)).await;
	assert!(
		has(&drawn, tr!("scan-camera-blocked")),
		"{:?}",
		drawn.labels
	);
	phone.click_label(tr!("mic-open-settings"));
	assert_eq!(phone.shell.camera_settings.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_denied_camera_says_so_and_can_be_retried() {
	let mut phone = people(false, true);
	let drawn = scan(&mut phone, Err(ScanError::CameraDenied)).await;
	assert!(has(&drawn, tr!("scan-camera-denied")));
	assert!(
		!has(&drawn, tr!("mic-open-settings")),
		"the OS will ask again"
	);
	phone.click_label(tr!("connection-scan"));
	tokio::time::sleep(Duration::from_millis(30)).await;
	assert_eq!(phone.shell.scans.load(Ordering::SeqCst), 2);
}

#[test]
fn the_scanned_text_takes_the_pasted_path() {
	assert_eq!(qr_scan::read(OTHER).as_deref(), Ok(OTHER));
	assert_eq!(qr_scan::read(&format!("\t{OTHER} ")).as_deref(), Ok(OTHER));
	let problem = |text: &str| qr_scan::read(text).unwrap_err();
	assert_eq!(problem("hello"), ConnectionStringProblem::Scheme);
	assert_eq!(
		problem("relay://10.0.0.9:7001"),
		ConnectionStringProblem::MissingFingerprint
	);
	assert_eq!(
		problem("relay://10.0.0.9:7001#abcd"),
		ConnectionStringProblem::BadFingerprint
	);
	assert_eq!(
		problem(&OTHER.replace("10.0.0.9", "example.com")),
		ConnectionStringProblem::BadAddress
	);
}

#[test]
fn every_scan_result_has_a_note() {
	assert_eq!(qr_scan::outcome(Ok(None)), None);
	assert_eq!(
		qr_scan::outcome(Ok(Some(OTHER.into()))),
		Some((ScanNote::Filled, Some(OTHER.into())))
	);
	assert_eq!(
		qr_scan::outcome(Ok(Some("nope".into()))),
		Some((ScanNote::Wrong(ConnectionStringProblem::Scheme), None))
	);
	assert_eq!(
		qr_scan::outcome(Err(ScanError::Unavailable)),
		Some((ScanNote::Failed(ScanError::Unavailable), None))
	);
	for error in [
		ScanError::CameraDenied,
		ScanError::CameraBlocked,
		ScanError::Unavailable,
		ScanError::Failed,
	] {
		assert!(!ScanNote::Failed(error).message().is_empty());
		assert!(ScanNote::Failed(error).is_problem());
	}
	assert!(!ScanNote::Filled.is_problem());
}

/// A shell that implements only the required methods.
struct Bare;

impl FilePicker for Bare {
	fn pick_image(&self) -> BoxFuture<'_, Result<Option<PickedFile>, String>> {
		Box::pin(async { Ok(None) })
	}
}

impl Shell for Bare {
	fn copy_text(&self, _: &str) {}
	fn request_repaint(&self) {}
	fn window_visible(&self) -> bool {
		true
	}
	fn show_window(&self) {}
	fn hide_window(&self) {}
	fn quit(&self) {}
	fn hides_to_tray(&self) -> bool {
		false
	}
}

#[tokio::test]
async fn a_shell_without_a_scanner_says_so() {
	assert!(!Bare.can_scan_qr());
	assert_eq!(Bare.scan_qr().await, Ok(None));
	Bare.open_camera_settings();
}
