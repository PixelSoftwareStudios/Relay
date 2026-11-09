//! The update button, its progress and failure lines, and the Updates group fit their space in
//! all nine languages, in the sidebar and at phone-like widths. One test, because the language
//! is process-wide state.

mod phone;

use std::sync::atomic::Ordering;

use phone::Phone;
use relay_ui::capabilities::SettingsCategory;
use relay_ui::i18n::Language;
use relay_ui::updates::{UpdateFailure, UpdateOffer, UpdateState};

fn offer(can_install: bool) -> UpdateOffer {
	UpdateOffer {
		version: "10.22.333".into(),
		release_url: "https://example.invalid/release".into(),
		can_install,
	}
}

fn states() -> Vec<UpdateState> {
	let o = offer(true);
	let mut all = vec![
		UpdateState::Available(o.clone()),
		UpdateState::Available(offer(false)),
		UpdateState::Downloading {
			offer: o.clone(),
			received: 99,
			total: 100,
		},
		UpdateState::Verifying(o.clone()),
		UpdateState::Installing(o.clone()),
		UpdateState::UpToDate("10.22.333".into()),
		UpdateState::Checking,
	];
	for failure in [
		UpdateFailure::Offline,
		UpdateFailure::Translocated,
		UpdateFailure::ReadOnly,
		UpdateFailure::NotWritable,
		UpdateFailure::Rejected,
		UpdateFailure::Damaged,
		UpdateFailure::Other,
	] {
		all.push(UpdateState::Failed {
			failure,
			offer: Some(o.clone()),
		});
	}
	for failure in [UpdateFailure::Offline, UpdateFailure::Rejected] {
		all.push(UpdateState::Failed {
			failure,
			offer: None,
		});
	}
	all
}

#[tokio::test]
async fn the_update_controls_fit_in_every_language() {
	let mut bad = Vec::new();
	for language in Language::ALL {
		for (w, h) in [(720.0, 800.0), (360.0, 800.0)] {
			for state in states() {
				for in_call in [false, true]
					.into_iter()
					.filter(|c| !c || state == states()[0])
				{
					let mut phone = Phone::desktop(w, h, language);
					phone.shell.self_update.store(true, Ordering::SeqCst);
					let mut shown = phone::busy_state();
					if !in_call {
						shown.calls.clear();
					}
					phone.show_state(&shown);
					phone.actions.shared.updates.set(state.clone());
					phone.app.open_settings(Some(SettingsCategory::Startup));
					let drawn = phone.settle();
					let screen = phone.screen().expand(1.0);
					let tag = format!("{language:?} {w}x{h} call={in_call} {state:?}");
					for r in drawn.widgets.iter().chain(&drawn.painted) {
						if r.min.x < screen.min.x || r.max.x > screen.max.x {
							bad.push(format!("{tag}: {r:?}"));
						}
					}
				}
			}
		}
	}
	bad.dedup();
	assert!(
		bad.is_empty(),
		"{} overflows: {:#?}",
		bad.len(),
		&bad[..bad.len().min(8)]
	);
}

#[tokio::test]
async fn the_sidebar_block_stays_inside_the_sidebar_in_every_language() {
	let mut bad = Vec::new();
	for language in Language::ALL {
		for state in states() {
			let mut phone = Phone::desktop(900.0, 800.0, language);
			phone.shell.self_update.store(true, Ordering::SeqCst);
			phone.actions.shared.updates.set(state.clone());
			let drawn = phone.settle();
			for (t, r) in &drawn.named {
				if r.min.x < relay_ui::theme::SIDEBAR_WIDTH - 4.0
					&& r.max.x > relay_ui::theme::SIDEBAR_WIDTH
				{
					bad.push(format!("{language:?} {state:?}: {t:?} {r:?}"));
				}
			}
		}
	}
	assert!(bad.is_empty(), "{bad:#?}");
}
