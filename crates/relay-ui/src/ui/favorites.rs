//! The Favorites section of the Calls page: up to six trusted contacts as a grid of tiles,
//! three to a row, each a one-tap call. A tile holds the picture, the name and whether the
//! person is online; a long press (or a right click) opens Open contact and Remove. Below the
//! grid two buttons lead to Contacts and to Connect. With nothing to show it says what to do.

use egui::{self, Align2, Color32, FontId, Sense, Ui, Vec2};

use crate::layout::Page;
use crate::model::{Link, Trust};
use crate::theme;
use crate::viewmodel::{PeerRow, favorites};

use super::kit::{self, Cx};
use super::widgets::button_row;
use crate::tr;

/// Tiles in a row. Three fit a 320 pt phone: the name is cut with an ellipsis if it is long.
const COLUMNS: usize = 3;
/// At most this many favorites exist.
pub const MAX: usize = 6;
/// Height of a tile: picture, name and one status line.
const TILE_HEIGHT: f32 = 104.0;
/// Size of the picture in a tile.
const PICTURE: f32 = 48.0;

pub fn show(ui: &mut Ui, rows: &[PeerRow], cx: &mut Cx) {
	let has_contacts = rows.iter().any(|r| r.known);
	let tiles: Vec<PeerRow> = favorites(cx.state).into_iter().take(MAX).collect();
	if tiles.is_empty() && !has_contacts && cx.state.link != Link::Connected {
		return;
	}
	kit::section_title(ui, tr!("favorites-heading"));
	if tiles.is_empty() {
		empty(ui, has_contacts, cx);
		return;
	}
	grid(ui, &tiles, cx);
	ui.add_space(theme::SPACE_S);
	kit::help(ui, tr!("favorites-hint"));
	ui.add_space(theme::SPACE_S);
	buttons(ui, true, cx);
}

fn empty(ui: &mut Ui, has_contacts: bool, cx: &mut Cx) {
	kit::card(ui, |ui| {
		if has_contacts {
			kit::card_title(ui, tr!("favorites-empty-title"));
			kit::help(ui, tr!("favorites-empty-help"));
		} else {
			kit::card_title(ui, tr!("favorites-no-contacts-title"));
			kit::help(ui, tr!("favorites-no-contacts-help"));
		}
		ui.add_space(theme::SPACE_S);
		buttons(ui, has_contacts, cx);
	});
}

/// Go to Contacts (when there are contacts) and Find nearby users.
fn buttons(ui: &mut Ui, contacts: bool, cx: &mut Cx) {
	button_row(ui, |ui| {
		if contacts && ui.button(tr!("favorites-go-contacts")).clicked() {
			cx.local.goto = Some(Page::Contacts);
		}
		if ui.button(tr!("favorites-find-nearby")).clicked() {
			cx.local.goto = Some(Page::Connect);
		}
	});
}

fn grid(ui: &mut Ui, tiles: &[PeerRow], cx: &mut Cx) {
	let gap = theme::SPACE_M;
	let width = ((ui.available_width() - gap * (COLUMNS as f32 - 1.0)) / COLUMNS as f32).floor();
	for line in tiles.chunks(COLUMNS) {
		ui.horizontal(|ui| {
			ui.spacing_mut().item_spacing.x = gap;
			for row in line {
				tile(ui, row, Vec2::new(width, TILE_HEIGHT), cx);
			}
		});
		ui.add_space(gap);
	}
}

/// Why a tile cannot be called, in words; `None` when it can.
fn reason(row: &PeerRow, cx: &Cx) -> Option<&'static str> {
	if row.can_call {
		return None;
	}
	let trust = cx
		.state
		.peers
		.iter()
		.find(|p| p.peer_id == row.peer_id)
		.map(|p| p.trust);
	Some(
		if matches!(trust, Some(Trust::Blocked | Trust::KeyChanged)) {
			tr!("favorite-blocked")
		} else if !row.reachable {
			tr!("favorite-offline")
		} else {
			tr!("favorite-busy")
		},
	)
}

fn tile(ui: &mut Ui, row: &PeerRow, size: Vec2, cx: &mut Cx) {
	let reason = reason(row, cx);
	let (rect, response) = ui.allocate_exact_size(size, Sense::click());
	let name = row.name.clone();
	response.widget_info(|| {
		egui::WidgetInfo::labeled(egui::Role::Button, reason.is_none(), name.as_str())
	});
	let visuals = ui.style().interact(&response);
	let fill = if response.hovered() || response.has_focus() {
		visuals.weak_bg_fill
	} else {
		ui.visuals().faint_bg_color
	};
	let text_color = if reason.is_some() {
		ui.visuals().weak_text_color()
	} else {
		ui.visuals().text_color()
	};
	let painter = ui.painter();
	painter.rect(
		rect,
		theme::CARD_RADIUS,
		fill,
		ui.visuals().widgets.noninteractive.bg_stroke,
		egui::StrokeKind::Inside,
	);
	let picture = egui::Rect::from_center_size(
		egui::pos2(rect.center().x, rect.min.y + theme::SPACE_M + PICTURE / 2.0),
		Vec2::splat(PICTURE),
	);
	let tex = cx.textures.texture(&row.avatar, cx.actions);
	let tint = if reason.is_some() {
		Color32::from_white_alpha(140)
	} else {
		Color32::WHITE
	};
	egui::Image::new(&tex)
		.fit_to_exact_size(picture.size())
		.corner_radius(PICTURE / 2.0)
		.tint(tint)
		.paint_at(ui, picture);
	status_dot(ui, picture, row.reachable, fill);
	let inner = size.x - 2.0 * theme::SPACE_S;
	let name = clipped(
		ui,
		&row.name,
		inner,
		FontId::proportional(theme::TEXT_SMALL + 2.0),
		text_color,
	);
	let y = picture.max.y + theme::SPACE_S;
	let name_height = name.size().y;
	ui.painter().galley(
		egui::pos2(rect.center().x - name.size().x / 2.0, y),
		name,
		text_color,
	);
	if let Some(reason) = reason {
		let weak = ui.visuals().weak_text_color();
		let line = clipped(
			ui,
			reason,
			inner,
			FontId::proportional(theme::TEXT_SMALL),
			weak,
		);
		ui.painter().galley(
			egui::pos2(rect.center().x - line.size().x / 2.0, y + name_height),
			line,
			weak,
		);
	}
	let response = match reason {
		Some(reason) => response.on_hover_text(reason),
		None => response.on_hover_text(tr!("favorite-call-hover", name = row.name.as_str())),
	};
	if reason.is_none() && response.clicked() {
		let id = row.peer_id.clone();
		cx.actions.run(|c| async move { c.start_call(&id).await });
	}
	response.context_menu(|ui| menu(ui, row, cx));
}

/// The press-and-hold menu of a tile.
fn menu(ui: &mut Ui, row: &PeerRow, cx: &mut Cx) {
	if ui.button(tr!("history-open-contact")).clicked() {
		cx.local.focus_contact = Some(row.peer_id.clone());
		cx.local.goto = Some(Page::Contacts);
		ui.close();
	}
	if ui.button(tr!("favorite-remove")).clicked() {
		let id = row.peer_id.clone();
		cx.actions
			.run(|c| async move { c.set_favorite(&id, false).await });
		ui.close();
	}
}

/// The online dot on the lower right of the picture, ringed in the tile's own colour.
fn status_dot(ui: &Ui, picture: egui::Rect, online: bool, ring: Color32) {
	let center = picture.right_bottom() - Vec2::splat(6.0);
	let painter = ui.painter();
	painter.circle_filled(center, 7.0, ring);
	let color = if online {
		theme::LIVE
	} else {
		ui.visuals().weak_text_color()
	};
	painter.circle_filled(center, 5.0, color);
}

/// One line of text cut to `width` with an ellipsis.
fn clipped(
	ui: &Ui,
	text: &str,
	width: f32,
	font: FontId,
	color: Color32,
) -> std::sync::Arc<egui::Galley> {
	let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
	job.wrap = egui::text::TextWrapping {
		max_width: width,
		max_rows: 1,
		break_anywhere: true,
		overflow_character: Some('\u{2026}'),
	};
	job.halign = Align2::LEFT_TOP.x();
	ui.painter().layout_job(job)
}
