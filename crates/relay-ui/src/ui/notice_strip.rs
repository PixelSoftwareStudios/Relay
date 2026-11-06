//! The notices strip: one card per notice with a close button, a "+N more"
//! expander past three and "Dismiss all" when more than one is showing.

use egui::{Align, Layout, RichText, Ui};

use crate::icons::{Icon, icon_button, icon_inline};
use crate::notice_words::Severity;
use crate::notices::{MAX_VISIBLE, Notice};
use crate::{theme, tr};

use super::{icon_style, kit};

/// What the user did to the strip in this frame.
#[derive(Debug, Default)]
pub struct Outcome {
	/// Identities dismissed by a close button.
	pub dismissed: Vec<u64>,
	/// "Dismiss all" was pressed.
	pub dismiss_all: bool,
	/// "+N more" or "Show fewer" was pressed.
	pub toggle: bool,
	/// Identities of the notices the pointer rests on.
	pub hovered: Vec<u64>,
}

/// How many of `total` notices are drawn.
pub fn shown_count(total: usize, expanded: bool) -> usize {
	if expanded {
		total
	} else {
		total.min(MAX_VISIBLE)
	}
}

/// Draw `notices`; the caller applies the outcome.
pub fn show(ui: &mut Ui, notices: &[Notice], expanded: bool) -> Outcome {
	let mut out = Outcome::default();
	for notice in &notices[..shown_count(notices.len(), expanded)] {
		let (close, hovered) = card(ui, notice);
		if close {
			out.dismissed.extend(&notice.ids);
		}
		if hovered {
			out.hovered.extend(&notice.ids);
		}
	}
	let hidden = notices.len().saturating_sub(MAX_VISIBLE);
	if hidden == 0 && notices.len() < 2 {
		return out;
	}
	ui.horizontal(|ui| {
		if hidden > 0 {
			let label = if expanded {
				tr!("notice-fewer").to_owned()
			} else {
				tr!("notice-more", count = hidden as i64)
			};
			out.toggle = ui.button(label).clicked();
		}
		if notices.len() > 1 {
			out.dismiss_all = ui.button(tr!("notice-dismiss-all")).clicked();
		}
	});
	out
}

/// One notice as a card: whether its close button was pressed and whether the
/// pointer rests on it.
pub fn card(ui: &mut Ui, notice: &Notice) -> (bool, bool) {
	let color = match notice.severity {
		Severity::Info => icon_style::info_color(ui),
		Severity::Warning => theme::WARN,
		Severity::Error => theme::ERROR,
	};
	let icon = match notice.severity {
		Severity::Info => Icon::Info,
		Severity::Warning => Icon::Warning,
		Severity::Error => Icon::Error,
	};
	let mut close = false;
	let frame = egui::Frame::group(ui.style())
		.stroke(egui::Stroke::new(1.0, color))
		.inner_margin(theme::SPACE_M)
		.show(ui, |ui| {
			ui.set_width(ui.available_width());
			ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
				close = icon_button(ui, Icon::Close, tr!("action-dismiss")).clicked();
				ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
					icon_inline(ui, icon, color);
					ui.vertical(|ui| body(ui, notice));
				});
			});
		});
	(close, ui.rect_contains_pointer(frame.response.rect))
}

fn body(ui: &mut Ui, notice: &Notice) {
	let title = if notice.count > 1 {
		tr!(
			"notice-repeated",
			title = notice.title.as_str(),
			count = notice.count as i64
		)
	} else {
		notice.title.clone()
	};
	ui.add(egui::Label::new(RichText::new(title).strong()).wrap());
	if let Some(step) = &notice.next_step {
		kit::help(ui, step);
	}
	if let Some(details) = &notice.details {
		kit::details(ui, ("notice-details", notice.id), |ui| {
			kit::help(ui, details)
		});
	}
}

/// Carry out what the user did: dismissals (a dismissed passing note is also
/// cleared from the state, so the same text later shows again), "Dismiss all"
/// and the expander.
pub fn apply(cx: &mut kit::Cx, notices: &[Notice], outcome: Outcome) {
	let mut gone = outcome.dismissed;
	if outcome.dismiss_all {
		gone.extend(notices.iter().flat_map(|n| &n.ids));
	}
	cx.local
		.notices
		.hold(&outcome.hovered, cx.actions.shell.now_unix_ms());
	if outcome.toggle {
		cx.local.notices.expanded ^= true;
	}
	if notices.len() <= MAX_VISIBLE {
		cx.local.notices.expanded = false;
	}
	cx.local.notices.dismiss(&gone);
	if let Some(text) = &cx.state.notice
		&& cx
			.local
			.notices
			.is_dismissed(crate::notices::note_identity(text))
	{
		let text = text.clone();
		cx.actions
			.hub
			.update(|s| s.notice = s.notice.take().filter(|n| *n != text));
	}
}
