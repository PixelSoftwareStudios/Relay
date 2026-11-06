//! Navigation of the regular layout: who you are and whether the daemon is
//! connected at the top, one entry per page, and the Settings gear at the
//! bottom.

use egui::{self, Align, Layout, RichText, Sense, Ui};

use crate::format::link_label;
use crate::icons::{Icon, page_icon};
use crate::layout::Page;
use crate::model::Link;
use crate::theme;
use crate::viewmodel::{discoverability, local_avatar};

use super::kit::Cx;
use super::visibility;
use super::widgets::avatar;
use crate::tr;

/// Height of one navigation entry.
const ENTRY_HEIGHT: f32 = 36.0;
/// Room for an entry icon, and its glyph size.
const ICON_SLOT: f32 = 22.0;
const ICON_SIZE: f32 = 18.0;

pub fn show(ui: &mut Ui, page: &mut Page, logo: &egui::TextureHandle, cx: &mut Cx, incoming: bool) {
	ui.add_space(theme::SPACE_S);
	ui.horizontal(|ui| {
		ui.image((logo.id(), egui::vec2(24.0, 24.0)));
		ui.label(RichText::new("Relay").size(theme::TEXT_HEADING).strong());
	});
	ui.add_space(theme::SPACE_M);
	identity(ui, cx);
	ui.add_space(theme::SPACE_L);
	for p in Page::SIDEBAR {
		let ring = incoming && p == Page::Calls;
		if entry(ui, page_icon(p), p.label(), *page == p, ring) {
			*page = p;
		}
	}
	ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
		ui.add_space(theme::SPACE_S);
		let label = Page::Settings.label().to_owned();
		if entry(
			ui,
			page_icon(Page::Settings),
			&label,
			*page == Page::Settings,
			false,
		) {
			*page = Page::Settings;
		}
		ui.separator();
	});
}

/// Picture, name and connection state of this device.
fn identity(ui: &mut Ui, cx: &mut Cx) {
	let name = cx
		.state
		.settings
		.as_ref()
		.map(|s| s.display_name.as_str())
		.filter(|n| !n.is_empty())
		.unwrap_or(tr!("sidebar-you"));
	ui.horizontal(|ui| {
		let tex = cx.textures.texture(&local_avatar(cx.state), cx.actions);
		avatar(ui, &tex, theme::AVATAR_ROW);
		ui.vertical(|ui| {
			ui.add(
				egui::Label::new(RichText::new(name).size(theme::TEXT_SIDEBAR_NAME).strong())
					.truncate(),
			);
			let (color, text) = link_state(cx.state.link);
			ui.horizontal(|ui| {
				ui.label(RichText::new("\u{25CF}").color(color).small());
				ui.add(egui::Label::new(RichText::new(text).small().weak()).truncate());
			});
			if let Some(d) = discoverability(cx.state) {
				visibility::chip(ui, d);
			}
		});
	});
}

fn link_state(link: Link) -> (egui::Color32, &'static str) {
	let color = match link {
		Link::Connected => theme::LIVE,
		Link::Offline => theme::ERROR,
		Link::Starting | Link::Connecting | Link::Reconnecting => theme::WARN,
	};
	(color, link_label(link))
}

/// A full-width entry; `alert` marks a page with something waiting (a ringing
/// call). Returns whether it was clicked.
fn entry(ui: &mut Ui, icon: Icon, label: &str, selected: bool, alert: bool) -> bool {
	let size = egui::vec2(ui.available_width(), ENTRY_HEIGHT);
	let (rect, response) = ui.allocate_exact_size(size, Sense::click());
	response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
	let visuals = ui.style().interact_selectable(&response, selected);
	if selected || response.hovered() || response.has_focus() {
		ui.painter().rect(
			rect,
			visuals.corner_radius,
			visuals.weak_bg_fill,
			visuals.bg_stroke,
			egui::StrokeKind::Inside,
		);
	}
	ui.painter().text(
		egui::pos2(
			rect.min.x + theme::SPACE_M + ICON_SLOT / 2.0,
			rect.center().y,
		),
		egui::Align2::CENTER_CENTER,
		icon.codepoint(),
		icon.font(ICON_SIZE),
		visuals.text_color(),
	);
	let text_x = rect.min.x + theme::SPACE_M + ICON_SLOT + theme::SPACE_M;
	let galley = ui.painter().layout(
		label.to_owned(),
		egui::TextStyle::Button.resolve(ui.style()),
		visuals.text_color(),
		rect.max.x - text_x - theme::SPACE_M - 14.0,
	);
	let pos = egui::pos2(text_x, rect.center().y - galley.size().y / 2.0);
	ui.painter().galley(pos, galley, visuals.text_color());
	if alert {
		ui.painter().circle_filled(
			egui::pos2(rect.max.x - theme::SPACE_M, rect.center().y),
			5.0,
			theme::LIVE,
		);
	}
	response.on_hover_text(label).clicked()
}
