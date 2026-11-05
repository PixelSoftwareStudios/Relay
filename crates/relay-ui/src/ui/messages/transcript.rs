//! The transcript list. Only the rows that intersect the viewport are laid
//! out: heights start as an estimate and are corrected by measurement
//! (`VirtualList`), so a history of thousands of messages costs a few rows.

use std::hash::{Hash, Hasher};

use egui::{RichText, TextureHandle, Ui};

use super::{markdown, offer};
use crate::avatar::{Lookup, TextureCache};
use crate::messaging::actions::Intent;
use crate::messaging::gates::LinkGate;
use crate::messaging::model::{Body, Delivery, EntryView, ImageView, SessionView, SettingsView};
use crate::messaging::rules::{INLINE_IMAGE_MAX_BYTES, clock_label, delivery_failure, format_size};
use crate::messaging::state::Transcript;
use crate::messaging::virtual_list::VirtualList;
use crate::theme;

/// First guess for an unmeasured row (points).
const ESTIMATE: f32 = 72.0;
/// Rows beyond the viewport that are laid out to avoid pop-in.
const OVERSCAN: f32 = 200.0;

/// Filtered row indices plus measured heights for one session.
pub struct ListCache {
	rows: Vec<usize>,
	key: (u64, String, bool),
	list: VirtualList,
}

impl Default for ListCache {
	fn default() -> Self {
		Self {
			rows: Vec::new(),
			key: (u64::MAX, String::new(), false),
			list: VirtualList::new(ESTIMATE),
		}
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
	pub intents: &'a mut Vec<Intent>,
}

fn width_key(width: f32, needle: &str, pinned_only: bool) -> u64 {
	let mut h = std::collections::hash_map::DefaultHasher::new();
	((width / 8.0) as u32).hash(&mut h);
	needle.hash(&mut h);
	pinned_only.hash(&mut h);
	h.finish()
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
		ui.label(
			RichText::new(
				"Older messages are not shown here. Export the call for the complete history.",
			)
			.small()
			.weak(),
		);
	}
	if cache.rows.is_empty() {
		let text = if t.entries.is_empty() {
			"No messages yet."
		} else {
			"Nothing matches."
		};
		ui.label(RichText::new(text).weak());
		return;
	}
	cache.list.sync(
		cache.rows.len(),
		width_key(ui.available_width(), needle, pinned_only),
	);
	let total = cache.list.total();
	let mut remeasured = false;
	egui::ScrollArea::vertical()
		.id_salt(("relay-msg-scroll", &rcx.session.key))
		.auto_shrink([false, false])
		.stick_to_bottom(true)
		.scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
		.show_viewport(ui, |ui, viewport| {
			ui.set_height(total);
			let vis = cache
				.list
				.visible(viewport.min.y, viewport.height(), OVERSCAN);
			let origin = ui.min_rect().min + egui::vec2(0.0, vis.offset);
			let rect =
				egui::Rect::from_min_size(origin, egui::vec2(ui.available_width(), f32::INFINITY));
			ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
				for row in vis.rows {
					let Some(entry) = t.entries.get(cache.rows[row]) else {
						continue;
					};
					let top = ui.cursor().min.y;
					draw_row(ui, rcx, entry);
					let height = ui.cursor().min.y - top;
					remeasured |= cache.list.set_height(row, height);
				}
			});
		});
	if remeasured {
		// Estimates were corrected: lay out once more so offsets are exact.
		ui.ctx().request_repaint();
	}
}

fn draw_row(ui: &mut Ui, rcx: &mut RowCx<'_>, entry: &EntryView) {
	let fill = if entry.outgoing {
		ui.visuals().faint_bg_color
	} else {
		ui.visuals().extreme_bg_color
	};
	egui::Frame::new()
		.fill(fill)
		.corner_radius(6.0)
		.inner_margin(8.0)
		.show(ui, |ui| {
			ui.set_width(ui.available_width());
			header(ui, rcx, entry);
			match &entry.body {
				Body::Text(t) => markdown::document(ui, &t.doc, rcx.gate, rcx.intents),
				Body::Image(img) => image(ui, rcx, entry, img),
				Body::File(o) => offer::card(ui, o, rcx.session, rcx.settings, rcx.intents),
			}
			footer(ui, rcx, entry);
		});
	ui.add_space(theme::SPACE_S);
}

fn header(ui: &mut Ui, rcx: &mut RowCx<'_>, entry: &EntryView) {
	ui.horizontal_wrapped(|ui| {
		let who = if entry.outgoing {
			"You"
		} else {
			rcx.session.peer_name.as_str()
		};
		ui.label(RichText::new(who).strong());
		ui.label(RichText::new(clock_label(entry.received_ms)).small().weak())
			.on_hover_text(format!(
				"UTC, when this device received or sent it.\nThe sender's own clock claims {}.",
				clock_label(entry.sent_ms)
			));
		if entry.pinned {
			ui.label(RichText::new("pinned").small().color(theme::WARN));
		}
		if entry.outgoing {
			match entry.delivery {
				Delivery::Sending => {
					ui.label(RichText::new("sending…").small().weak());
				}
				Delivery::Delivered => {
					ui.label(RichText::new("delivered").small().weak());
				}
				Delivery::Failed => {
					ui.label(
						RichText::new(delivery_failure(&entry.failure))
							.small()
							.color(theme::ERROR),
					);
				}
			}
		}
	});
}

fn footer(ui: &mut Ui, rcx: &mut RowCx<'_>, entry: &EntryView) {
	ui.horizontal_wrapped(|ui| {
		if let Some(text) = entry.text()
			&& ui.small_button("Copy").clicked()
		{
			rcx.intents.push(Intent::Copy(text.to_owned()));
		}
		let label = if entry.pinned { "Unpin" } else { "Pin" };
		if ui
			.small_button(label)
			.on_hover_text("Pins stay on this device only")
			.clicked()
		{
			rcx.intents.push(Intent::Pin {
				session: rcx.session.key.clone(),
				outgoing: entry.outgoing,
				seq: entry.seq,
				pinned: !entry.pinned,
			});
		}
		if entry.outgoing
			&& entry.delivery == Delivery::Failed
			&& rcx.session.is_live()
			&& ui.small_button("Retry").clicked()
		{
			rcx.intents.push(Intent::Retry {
				session: rcx.session.key.clone(),
				seq: entry.seq,
			});
		}
	});
}

fn image(ui: &mut Ui, rcx: &mut RowCx<'_>, entry: &EntryView, img: &ImageView) {
	let caption = format!(
		"{} · {}x{} · {}",
		img.media_type.trim_start_matches("image/"),
		img.width,
		img.height,
		format_size(img.data.len() as u64)
	);
	if img.data.len() > INLINE_IMAGE_MAX_BYTES {
		ui.label(
			RichText::new(
				"This image is over the inline limit and is not shown. Ask for it as a file.",
			)
			.color(theme::WARN),
		);
		return;
	}
	let key = format!("{}:{}", rcx.session.key, entry.ordinal);
	match rcx.textures.get(&key) {
		Lookup::Ready(tex) => {
			let max = ui.available_width().min(320.0);
			let size = tex.size_vec2();
			let scale = (max / size.x).min(1.0);
			ui.add(egui::Image::new(tex).fit_to_exact_size(size * scale));
		}
		Lookup::Missing => {
			ui.label(
				RichText::new("This image cannot be shown (it is not a valid or safe picture).")
					.color(theme::WARN),
			);
		}
		Lookup::Pending => {
			ui.label(RichText::new("Loading image…").weak());
		}
		Lookup::Unknown => {
			if rcx.textures.request(&key) {
				rcx.intents.push(Intent::DecodeImage {
					key,
					data: img.data.clone(),
				});
			}
		}
	}
	ui.label(RichText::new(caption).small().weak());
}
