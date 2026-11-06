//! The Scan card of the Connect page: hidden unless the shell can scan. A scanned
//! string takes the same checks as a pasted one, then the device is dialled and
//! trusted (`connect_and_trust`) and the page says how that went.

mod common;
mod phone;

use std::sync::Arc;

use std::sync::atomic::Ordering;
use std::time::Duration;

use common::FakeOps;
use phone::{Drawn, Phone};
use relay_client::connection::ConnectionStringProblem;
use relay_ui::engine::Command;
use relay_ui::i18n::Language;
use relay_ui::layout::Page;
use relay_ui::model::ConnectionView;
use relay_ui::model::{Peer, Trust};
use relay_ui::qr_scan::{self, ScanError, ScanNote, ScanTrust, Scanned};
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

/// A phone whose daemon is the fake; the device in `OTHER` is already known by
/// name (as the engine reports it once it is trusted).
fn with_ops() -> (Phone, Arc<FakeOps>) {
	let phone = people(false, true);
	let ops = Arc::new(FakeOps::default());
	phone.slot.set(ops.clone());
	let mut state = phone::busy_state();
	state.peers.clear();
	state.calls.clear();
	state.peers.push(Peer {
		peer_id: "cd".repeat(32),
		display_name: "Dantes Laptop".into(),
		addresses: Vec::new(),
		trust: Trust::Verified,
		reachable: true,
		avatar_hash: String::new(),
		fingerprint_short: String::new(),
		shared_channels: Vec::new(),
		favorite: false,
		favorite_position: 0,
		port: 7000,
		last_seen_unix_ms: 0,
	});
	phone.show_state(&state);
	(phone, ops)
}

#[tokio::test]
async fn a_scanned_code_connects_trusts_and_says_so() {
	let (mut phone, ops) = with_ops();
	let drawn = scan(&mut phone, Ok(Some(format!(" {OTHER}\n")))).await;
	tokio::time::sleep(Duration::from_millis(50)).await;
	assert_eq!(
		ops.recorded(),
		vec![Command::ConnectAndTrust { text: OTHER.into() }]
	);
	let done = phone.settle();
	assert!(
		has(&done, "Added Dantes Laptop to Contacts."),
		"{:?} / {:?}",
		drawn.labels,
		done.labels
	);
	assert!(!has(&done, OTHER), "nothing is put in the paste field");
}

#[tokio::test]
async fn failures_are_said_in_plain_words_and_nothing_is_added() {
	for (engine_says, shown) in [
		("the peer could not be reached", tr!("scan-unreachable")),
		(
			"static key does not match the fingerprint",
			tr!("scan-mismatch"),
		),
	] {
		let (mut phone, ops) = with_ops();
		*ops.fail_with.lock().unwrap() = Some(engine_says.into());
		scan(&mut phone, Ok(Some(OTHER.into()))).await;
		tokio::time::sleep(Duration::from_millis(50)).await;
		let done = phone.settle();
		assert!(has(&done, shown), "{engine_says}: {:?}", done.labels);
		assert!(!done.labels.iter().any(|l| l.starts_with("Added ")));
	}
	let (mut phone, ops) = with_ops();
	*ops.fail_with.lock().unwrap() = Some("something odd".into());
	scan(&mut phone, Ok(Some(OTHER.into()))).await;
	tokio::time::sleep(Duration::from_millis(50)).await;
	let done = phone.settle();
	assert!(
		has(&done, "Could not add that device: something odd"),
		"{:?}",
		done.labels
	);
}

#[tokio::test]
async fn a_cancelled_scan_changes_nothing() {
	let mut phone = people(false, true);
	let drawn = scan(&mut phone, Ok(None)).await;
	for key in [
		"scan-trusting",
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
	let (mut phone, ops) = with_ops();
	scan(&mut phone, Ok(Some("https://example.com/x".into()))).await;
	tokio::time::sleep(Duration::from_millis(30)).await;
	assert!(ops.recorded().is_empty(), "a wrong code dials nobody");
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
		Some(Scanned::Connect(OTHER.into()))
	);
	assert_eq!(
		qr_scan::outcome(Ok(Some("nope".into()))),
		Some(Scanned::Note(ScanNote::Wrong(
			ConnectionStringProblem::Scheme
		)))
	);
	assert_eq!(
		qr_scan::outcome(Err(ScanError::Unavailable)),
		Some(Scanned::Note(ScanNote::Failed(ScanError::Unavailable)))
	);
	assert_eq!(ScanTrust::digest_of(OTHER), "cd".repeat(32));
	for error in [
		ScanError::CameraDenied,
		ScanError::CameraBlocked,
		ScanError::Unavailable,
		ScanError::Failed,
	] {
		assert!(!ScanNote::Failed(error).message().is_empty());
	}
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
