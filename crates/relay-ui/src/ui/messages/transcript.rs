//! The transcript list. Only the rows that intersect the viewport are laid
//! out: heights start as an estimate and are corrected by measurement
//! (`VirtualList`), so a history of thousands of messages costs a few rows.
//! New messages keep the list at the bottom unless the reader scrolled up;
//! then a "New messages" pill offers the way down.

use std::hash::{Hash, Hasher};

use egui::{Align, Rect, RichText, TextureHandle, Ui, pos2, vec2};

use super::bubble;
use crate::avatar::TextureCache;
use crate::layout::{self, MIN_TOUCH_TARGET};
use crate::messaging::actions::Intent;
use crate::messaging::gates::LinkGate;
use crate::messaging::grouping;
use crate::messaging::model::{SessionView, SettingsView};
use crate::messaging::state::Transcript;
use crate::messaging::virtual_list::VirtualList;
use crate::theme;
use crate::tr;
use relay_io::localtime::LocalZone;

/// First guess for an unmeasured row (points).
const ESTIMATE: f32 = 72.0;
/// Rows beyond the viewport that are laid out to avoid pop-in.
const OVERSCAN: f32 = 200.0;
/// Distance from the end that still counts as "at the bottom" (points).
const BOTTOM_SLACK: f32 = 24.0;

/// Filtered row indices plus measured heights and scroll bookkeeping for one session.
pub struct ListCache {
	rows: Vec<usize>,
	key: (u64, String, bool),
	list: VirtualList,
	/// Rows at the previous frame, to notice arrivals.
	seen_rows: usize,
	/// The reader was at the end of the list at the previous frame.
	at_bottom: bool,
	/// Messages arrived while the reader was elsewhere.
	unseen: bool,
	/// Scroll to the end on the next frame.
	jump: bool,
	/// The user just sent: scroll down as soon as the message is in the list.
	follow_own: bool,
}

impl Default for ListCache {
	fn default() -> Self {
		Self {
			rows: Vec::new(),
			key: (u64::MAX, String::new(), false),
			list: VirtualList::new(ESTIMATE),
			seen_rows: 0,
			at_bottom: true,
			unseen: false,
			jump: false,
			follow_own: false,
		}
	}
}

impl ListCache {
	/// The user sent a message: show the end of the list once it arrives.
	pub fn follow_own(&mut self) {
		self.follow_own = true;
	}
}

/// Entries that pass the search and the pinned filter.
pub fn filter(t: &Transcript, needle_lower: &str, pinned_only: bool) -> Vec<usize> {
	t.entries
		.iter()
		.enumerate()
		.filter(|(_, e)| (!pinned_only || e.pinned) && e.matches(needle_lower))
		.map(|(i, _)| i)
		.collect()
}

/// Everything a row needs besides the entry.
pub struct RowCx<'a> {
	pub session: &'a SessionView,
	pub settings: Option<SettingsView>,
	pub textures: &'a mut TextureCache<TextureHandle>,
	pub gate: &'a mut LinkGate,
	pub zone: &'a LocalZone,
	pub now_ms: u64,
	pub intents: &'a mut Vec<Intent>,
}

fn width_key(width: f32, needle: &str, pinned_only: bool) -> u64 {
	let mut h = std::collections::hash_map::DefaultHasher::new();
	((width / 8.0) as u32).hash(&mut h);
	needle.hash(&mut h);
	pinned_only.hash(&mut h);
	h.finish()
}

fn empty(ui: &mut Ui, title: &str, hint: Option<&str>) {
	ui.vertical_centered(|ui| {
		ui.add_space((ui.available_height() * 0.3).max(theme::SPACE_XL));
		ui.label(RichText::new(title).size(theme::TEXT_HEADING).strong());
		if let Some(hint) = hint {
			ui.label(RichText::new(hint).weak());
		}
	});
}

/// Draw the list of one session inside the remaining space.
pub fn show(
	ui: &mut Ui,
	cache: &mut ListCache,
	t: &Transcript,
	needle: &str,
	pinned_only: bool,
	rcx: &mut RowCx<'_>,
) {
	let key = (t.generation, needle.to_owned(), pinned_only);
	if cache.key != key {
		cache.rows = filter(t, needle, pinned_only);
		cache.key = key;
	}
	if t.truncated {
		ui.label(RichText::new(tr!("msg-older-hidden")).small().weak());
	}
	if cache.rows.is_empty() {
		if t.entries.is_empty() {
			empty(ui, tr!("msg-empty-title"), Some(tr!("msg-empty-hint")));
		} else {
			empty(ui, tr!("msg-no-match"), None);
		}
		return;
	}
	cache.list.sync(
		cache.rows.len(),
		width_key(ui.available_width(), needle, pinned_only),
	);
	if cache.rows.len() > cache.seen_rows {
		if cache.follow_own {
			cache.jump = true;
			cache.follow_own = false;
		} else if !cache.at_bottom && cache.seen_rows > 0 {
			cache.unseen = true;
		}
	}
	cache.seen_rows = cache.rows.len();
	let total = cache.list.total();
	let jump = std::mem::take(&mut cache.jump);
	let mut remeasured = false;
	let area = ui.max_rect();
	let output = egui::ScrollArea::vertical()
		.id_salt(("relay-msg-scroll", &rcx.session.key))
		.auto_shrink([false, false])
		.stick_to_bottom(true)
		.animated(false)
		.scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
		.show_viewport(ui, |ui, viewport| {
			ui.set_height(total);
			let vis = cache
				.list
				.visible(viewport.min.y, viewport.height(), OVERSCAN);
			let top = ui.min_rect().min;
			let origin = top + vec2(0.0, vis.offset);
			let rect = Rect::from_min_size(origin, vec2(ui.available_width(), f32::INFINITY));
			ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
				for row in vis.rows {
					let Some(entry) = t.entries.get(cache.rows[row]) else {
						continue;
					};
					let at = |n: Option<usize>| n.and_then(|n| t.entries.get(*cache.rows.get(n)?));
					let place =
						grouping::place(rcx.zone, at(row.checked_sub(1)), entry, at(Some(row + 1)));
					let start = ui.cursor().min.y;
					bubble::row(ui, rcx, entry, place);
					let height = ui.cursor().min.y - start;
					remeasured |= cache.list.set_height(row, height);
				}
			});
			if jump {
				let end = Rect::from_min_size(top + vec2(0.0, total), vec2(1.0, 1.0));
				ui.scroll_to_rect(end, Some(Align::BOTTOM));
			}
		});
	let reached = output.state.clamped_offset().y + output.inner_rect.height();
	cache.at_bottom = reached >= output.content_size.y - BOTTOM_SLACK;
	if cache.at_bottom {
		cache.unseen = false;
	}
	if cache.unseen {
		jump_pill(ui, cache, area);
	}
	if remeasured || jump {
		// Estimates were corrected, or the end was asked for: lay out once more.
		ui.ctx().request_repaint();
	}
}

/// The "New messages" pill over the bottom of the list.
fn jump_pill(ui: &mut Ui, cache: &mut ListCache, area: Rect) {
	let height = if layout::is_touch(ui) {
		MIN_TOUCH_TARGET
	} else {
		32.0
	};
	let text = RichText::new(tr!("msg-new-messages"))
		.strong()
		.color(ui.visuals().selection.stroke.color);
	let width = (area.width() - 2.0 * theme::SPACE_L).clamp(120.0, 200.0);
	let rect = Rect::from_center_size(
		pos2(area.center().x, area.max.y - theme::SPACE_M - height / 2.0),
		vec2(width, height),
	);
	let pill = egui::Button::new(text)
		.fill(ui.visuals().selection.bg_fill)
		.corner_radius(height / 2.0);
	if ui.put(rect, pill).clicked() {
		cache.jump = true;
		cache.unseen = false;
		ui.ctx().request_repaint();
	}
}
