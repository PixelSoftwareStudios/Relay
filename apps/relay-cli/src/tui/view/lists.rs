//! Peers, devices and settings screens.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use super::meter;
use crate::commands::names;
use crate::tui::model::{Model, Tab};

fn render_list(frame: &mut Frame, area: Rect, title: &str, items: Vec<ListItem>, selected: usize) {
	let mut state = ListState::default().with_selected(Some(selected));
	let list = List::new(items)
		.block(
			Block::default()
				.borders(Borders::ALL)
				.title(title.to_owned()),
		)
		.highlight_style(Style::new().add_modifier(Modifier::REVERSED))
		.highlight_symbol("> ");
	frame.render_stateful_widget(list, area, &mut state);
}

pub fn peers(frame: &mut Frame, area: Rect, model: &Model) {
	let items: Vec<ListItem> = model
		.snapshot
		.peers
		.iter()
		.map(|p| {
			let (dot, color) = if p.reachable {
				("*", Color::Green)
			} else {
				("o", Color::DarkGray)
			};
			ListItem::new(Line::from(vec![
				Span::styled(dot, Style::new().fg(color)),
				Span::raw(format!(
					" {:<24} {:<11} {}",
					p.display_name,
					names::trust_state(p.trust),
					p.local_addresses.join(", ")
				)),
			]))
		})
		.collect();
	let title = if items.is_empty() {
		"Peers (none found yet)"
	} else {
		"Peers (enter: call)"
	};
	render_list(frame, area, title, items, model.selection(Tab::Peers));
}

pub fn devices(frame: &mut Frame, area: Rect, model: &Model) {
	let settings = model.snapshot.settings.clone().unwrap_or_default();
	let items: Vec<ListItem> = model
		.snapshot
		.devices
		.iter()
		.map(|d| {
			let chosen =
				d.device_id == settings.input_device_id || d.device_id == settings.output_device_id;
			let style = if d.available {
				Style::new()
			} else {
				Style::new().fg(Color::DarkGray)
			};
			ListItem::new(format!(
				"{} {:<7} {}{}",
				if chosen { "*" } else { " " },
				names::direction(d.direction),
				d.name,
				if d.system_default { " (default)" } else { "" },
			))
			.style(style)
		})
		.collect();
	render_list(
		frame,
		area,
		"Devices (enter: select)",
		items,
		model.selection(Tab::Devices),
	);
}

pub fn settings(frame: &mut Frame, area: Rect, model: &Model) {
	let s = model.snapshot.settings.clone().unwrap_or_default();
	let gain = |v: f32| {
		format!(
			"[{}] x{v:.2}",
			meter::bar(Some(-60.0 + 60.0 * (v / 4.0)), 16)
		)
	};
	let rows = [
		format!("Microphone gain   {}   (+/-)", gain(s.microphone_gain)),
		format!("Playback gain     {}   (+/-)", gain(s.playback_gain)),
		format!("Mute microphone   {}", on_off(s.global_muted)),
		format!("Deafen playback   {}", on_off(s.global_deafened)),
	];
	let items = rows.into_iter().map(ListItem::new).collect();
	render_list(
		frame,
		area,
		"Settings",
		items,
		model.selection(Tab::Settings),
	);
}

fn on_off(v: bool) -> &'static str {
	if v { "on" } else { "off" }
}
