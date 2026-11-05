mod msgkit;

use msgkit::*;
use relay_messages::markdown::{Block, Inline, parse};
use relay_ui::messaging::convert;
use relay_ui::messaging::gates::{
	Destination, LinkGate, Target, clipboard_problem, clipboard_target,
};
use relay_ui::messaging::model::{OfferStatus, OfferView, SessionView};
use relay_ui::messaging::rules::*;
use relay_ui::messaging::state::Feed;

fn session(live: bool) -> SessionView {
	convert::session(&summary(1, "Ada", live))
}

fn offer_view(state: relay_api::pb::OfferState, outgoing: bool) -> OfferView {
	convert::offer(&offer(1, state, outgoing))
}

#[test]
fn counter_tracks_bytes_not_chars_and_limit() {
	assert_eq!(Counter::of("héllo").used, 6);
	let at = "a".repeat(4096);
	assert!(!Counter::of(&at).over() && Counter::of(&at).sendable(&at));
	let over = "a".repeat(4097);
	assert!(Counter::of(&over).over() && !Counter::of(&over).sendable(&over));
	assert!(!Counter::of("  \n").sendable("  \n"));
	assert_eq!(Counter::of("ab").label(), "2 / 4096 bytes");
}

#[test]
fn compose_is_blocked_with_a_reason_per_state() {
	let live = session(true);
	assert!(compose_blocked(&Feed::Live, true, &live).is_none());
	assert!(compose_blocked(&Feed::Off, true, &live).is_some());
	let off = compose_blocked(&Feed::Live, false, &live).unwrap();
	assert!(off.reason.contains("daemon") && !off.export_clear);
	let ended = compose_blocked(&Feed::Live, true, &session(false)).unwrap();
	assert!(ended.export_clear);
	let mut no_perm = session(true);
	no_perm.perms.allow_messages = false;
	let b = compose_blocked(&Feed::Live, true, &no_perm).unwrap();
	assert!(b.reason.contains("Both sides") && !b.export_clear);
	let mut full = session(true);
	full.used_bytes = full.limit_bytes;
	let b = compose_blocked(&Feed::Live, true, &full).unwrap();
	assert!(b.export_clear && b.reason.contains("full"));
}

#[test]
fn files_need_a_live_call_and_permission() {
	assert!(files_blocked(&session(true)).is_none());
	assert!(files_blocked(&session(false)).is_some());
	let mut s = session(true);
	s.perms.allow_files = false;
	assert!(files_blocked(&s).is_some());
}

#[test]
fn offer_buttons_follow_state_and_direction() {
	use relay_api::pb::OfferState as O;
	let s = session(true);
	let a = offer_actions(&offer_view(O::Offered, false), &s);
	assert!(a.download && a.reject && a.request_preview && !a.cancel);
	let mut previewed = offer_view(O::Offered, false);
	previewed.preview_requested = true;
	assert!(!offer_actions(&previewed, &s).request_preview);
	let mut pending = offer_view(O::Offered, true);
	pending.preview_approval_pending = true;
	let a = offer_actions(&pending, &s);
	assert!(a.cancel && a.approve_preview && !a.download);
	assert!(offer_actions(&offer_view(O::InProgress, false), &s).cancel);
	assert_eq!(
		offer_actions(&offer_view(O::Completed, false), &s),
		OfferActions::default()
	);
	// An ended call offers nothing: Download is unavailable.
	assert_eq!(
		offer_actions(&offer_view(O::Offered, false), &session(false)),
		OfferActions::default()
	);
	let mut no_files = session(true);
	no_files.perms.allow_files = false;
	assert!(!offer_actions(&offer_view(O::Offered, false), &no_files).download);
}

#[test]
fn offer_text_and_warnings() {
	use relay_api::pb::OfferState as O;
	let mut o = offer_view(O::InProgress, false);
	o.transferred = 250_000;
	assert_eq!(status_label(&o), "Transferring 25%");
	assert!((progress(&o) - 0.25).abs() < 1e-6);
	assert_eq!(hash_short(&[0xab; 32]), "abababab…");
	assert_eq!(format_size(1536), "1.5 KiB");
	let mut exe = offer_view(O::Offered, false);
	exe.executable_warning = true;
	exe.size = 2 * 1024 * 1024 * 1024;
	let w = offer_warnings(&exe, Some(1024));
	assert!(w.iter().any(|t| t.contains("program")));
	assert!(w.iter().any(|t| t.contains("Large file")));
	assert!(w.iter().any(|t| t.contains("limit")));
	assert_eq!(required_space(10), 10 + 16 * 1024 * 1024);
	assert_eq!(type_guess("a.zip", "application/zip", false), "Archive");
	assert_eq!(
		type_guess("a.sh", "", true),
		"Program or script (runs code)"
	);
	assert!(!OfferStatus::Offered.is_final());
}

fn dest(n: u8) -> Destination {
	Destination {
		session: key(n),
		peer_name: format!("peer{n}"),
	}
}

#[test]
fn clipboard_never_has_a_silent_default_with_several_calls() {
	assert_eq!(clipboard_target(&[], None), Target::NoCall);
	assert_eq!(clipboard_target(&[dest(1)], None), Target::Confirm(dest(1)));
	let two = [dest(1), dest(2)];
	assert_eq!(clipboard_target(&two, None), Target::MustPick);
	assert_eq!(clipboard_target(&two, Some(5)), Target::MustPick);
	assert_eq!(clipboard_target(&two, Some(1)), Target::Confirm(dest(2)));
}

#[test]
fn clipboard_text_is_refused_not_truncated() {
	assert!(clipboard_problem("hi").is_none());
	assert!(clipboard_problem(" ").is_some());
	assert!(
		clipboard_problem(&"x".repeat(4097))
			.unwrap()
			.contains("Nothing was sent")
	);
}

fn first_link(text: &str) -> relay_messages::markdown::Link {
	let doc = parse(text);
	let Block::Paragraph(p) = &doc.blocks[0] else {
		panic!()
	};
	p.iter()
		.find_map(|i| {
			if let Inline::Link(l) = i {
				Some(l.clone())
			} else {
				None
			}
		})
		.expect("link")
}

#[test]
fn a_link_opens_only_after_ask_then_confirm_and_only_once() {
	let mut gate = LinkGate::default();
	assert_eq!(gate.confirm(), None, "nothing was asked");
	gate.ask(&first_link("see https://example.com/a now"));
	assert_eq!(gate.pending().unwrap().host, "example.com");
	gate.cancel();
	assert_eq!(gate.confirm(), None, "cancelled");
	gate.ask(&first_link("https://example.com/a"));
	assert_eq!(gate.confirm().as_deref(), Some("https://example.com/a"));
	assert_eq!(gate.confirm(), None, "single use");
}

#[test]
fn lookalike_hosts_show_the_ascii_form_and_a_warning() {
	let mut gate = LinkGate::default();
	gate.ask(&first_link("https://xn--pple-43d.com/x"));
	let p = gate.pending().unwrap();
	assert_eq!(p.host, "xn--pple-43d.com");
	assert!(p.warning_text().is_some());
}

#[test]
fn session_choice_prefers_the_asked_call_then_current_then_live() {
	let a = convert::session(&summary(1, "A", false));
	let b = convert::session(&summary(2, "B", true));
	let c = convert::session(&summary(3, "C", true));
	let all = [a.clone(), b.clone(), c.clone()];
	assert_eq!(
		choose_session(&all, None, Some(&c.call_id)),
		Some(c.key.clone())
	);
	assert_eq!(
		choose_session(&all, Some(&a.key), None),
		Some(a.key.clone())
	);
	assert_eq!(
		choose_session(&all, Some("gone"), None),
		Some(b.key.clone())
	);
	assert_eq!(
		choose_session(std::slice::from_ref(&a), None, None),
		Some(a.key)
	);
	assert_eq!(choose_session(&[], None, None), None);
}

#[test]
fn problem_codes_have_text_without_content() {
	for code in [
		"MESSAGES_DISABLED",
		"SESSION_PERMISSION_OFF",
		"HISTORY_FULL",
		"NO_SPACE",
		"weird",
	] {
		assert!(!problem_text(code).is_empty());
	}
	assert!(delivery_failure("timeout").contains("did not answer"));
}
