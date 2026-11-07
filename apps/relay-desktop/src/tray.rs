//! System tray icon (macOS and Windows) with idle, in-call and muted glyphs.
//! On Linux the app builds no tray: `tray-icon` needs a GTK main loop there,
//! which this client does not run, so closing the window quits the app.

use std::sync::Arc;

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use relay_ui::actions::Actions;
use relay_ui::emitter::{TrayState, TraySummary};
use relay_ui::imaging::decode_for_texture;
use relay_ui::tr;

use crate::background::{Background, TrayFacts};

/// Monochrome template glyphs. macOS draws the `@2x` pair on Retina screens
/// and tints template images itself; other platforms use the 1x glyphs.
const GLYPHS: [(TrayState, &[u8], &[u8]); 3] = [
	(
		TrayState::Idle,
		include_bytes!("../../../assets/icon/tray-idle.png"),
		include_bytes!("../../../assets/icon/tray-idle@2x.png"),
	),
	(
		TrayState::InCall,
		include_bytes!("../../../assets/icon/tray-call.png"),
		include_bytes!("../../../assets/icon/tray-call@2x.png"),
	),
	(
		TrayState::Muted,
		include_bytes!("../../../assets/icon/tray-muted.png"),
		include_bytes!("../../../assets/icon/tray-muted@2x.png"),
	),
];

/// The glyph bytes for a state on the current platform.
pub fn glyph(state: TrayState) -> &'static [u8] {
	let (_, one, two) = GLYPHS
		.iter()
		.find(|(s, _, _)| *s == state)
		.copied()
		.unwrap_or(GLYPHS[0]);
	if cfg!(target_os = "macos") { two } else { one }
}

fn icon_for(state: TrayState) -> Option<Icon> {
	let image = decode_for_texture(glyph(state))?;
	Icon::from_rgba(image.rgba, image.size[0] as u32, image.size[1] as u32).ok()
}

pub struct Tray {
	icon: TrayIcon,
	status: MenuItem,
	availability: MenuItem,
	stay: CheckMenuItem,
	facts: TrayFacts,
	show: MenuItem,
	quit: MenuItem,
	shown: TrayState,
}

impl Tray {
	/// Only called when the tray text changed (the emitter bounds the rate).
	pub fn apply(&mut self, summary: &TraySummary) {
		let _ = self.icon.set_tooltip(Some(&summary.tooltip));
		self.status.set_text(&summary.tooltip);
		// The summary differs when the language changed, so the menu follows it.
		self.show.set_text(tr!("tray-show"));
		self.quit.set_text(tr!("tray-quit"));
		self.stay.set_text(tr!("tray-stay-available"));
		self.availability.set_text(availability_text(self.facts));
		if summary.icon != self.shown
			&& let Some(icon) = icon_for(summary.icon)
		{
			let _ = set_tray_icon(&self.icon, icon);
			self.shown = summary.icon;
		}
	}
}

impl Tray {
	/// Redraw the availability line and the check item from the facts.
	pub fn set_facts(&mut self, facts: TrayFacts) {
		self.facts = facts;
		self.availability.set_text(availability_text(facts));
		self.stay.set_checked(facts.background);
	}
}

/// The status line under the tooltip: whether this device can be reached right now.
pub fn availability_text(facts: TrayFacts) -> &'static str {
	if facts.available {
		tr!("tray-status-available")
	} else {
		tr!("tray-status-unavailable")
	}
}

/// macOS tints template images itself; other platforms take the glyph as is.
#[cfg(target_os = "macos")]
fn set_tray_icon(tray: &TrayIcon, icon: Icon) -> Result<(), tray_icon::Error> {
	tray.set_icon_templated(Some(icon))
}

#[cfg(not(target_os = "macos"))]
fn set_tray_icon(tray: &TrayIcon, icon: Icon) -> Result<(), tray_icon::Error> {
	tray.set_icon(Some(icon))
}

#[cfg(target_os = "macos")]
fn with_glyph(builder: TrayIconBuilder, icon: Icon) -> TrayIconBuilder {
	builder.with_icon_templated(icon)
}

#[cfg(not(target_os = "macos"))]
fn with_glyph(builder: TrayIconBuilder, icon: Icon) -> TrayIconBuilder {
	builder.with_icon(icon)
}

/// Build the tray on the UI thread. Failure is non-fatal.
///
/// `set_background` asks the engine to store `background.available`; the check mark follows
/// the setting as it comes back, not the click.
pub fn build(
	actions: &Actions,
	background: Arc<Background>,
	set_background: Arc<dyn Fn(bool) + Send + Sync>,
) -> Option<Tray> {
	if cfg!(not(any(target_os = "macos", target_os = "windows"))) {
		return None;
	}
	let status = MenuItem::with_id("status", "Relay", false, None);
	let show = MenuItem::with_id("show", tr!("tray-show"), true, None);
	let quit = MenuItem::with_id("quit", tr!("tray-quit"), true, None);
	let facts = background.facts();
	let availability = MenuItem::with_id("availability", availability_text(facts), false, None);
	let stay = CheckMenuItem::with_id(
		"stay",
		tr!("tray-stay-available"),
		true,
		facts.background,
		None,
	);
	let menu = Menu::new();
	menu.append_items(&[
		&status,
		&availability,
		&PredefinedMenuItem::separator(),
		&stay,
		&show,
		&quit,
	])
	.ok()?;
	let builder = TrayIconBuilder::new()
		.with_tooltip("Relay")
		.with_menu(Box::new(menu))
		.with_menu_on_left_click(false);
	let icon = with_glyph(builder, icon_for(TrayState::Idle)?)
		.build()
		.ok()?;

	let a = actions.clone();
	MenuEvent::set_event_handler(Some(move |event: MenuEvent| match event.id.0.as_str() {
		"show" => a.show_window(),
		"stay" => {
			set_background(!background.enabled());
			background.mark_stale();
			a.shell.request_repaint();
		}
		// The tray can outlive a hidden window that never repaints, so exit directly.
		"quit" => std::process::exit(0),
		_ => {}
	}));
	let a = actions.clone();
	TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
		if let TrayIconEvent::Click {
			button: MouseButton::Left,
			button_state: MouseButtonState::Up,
			..
		} = event
		{
			a.show_window();
		}
	}));
	Some(Tray {
		icon,
		status,
		availability,
		stay,
		facts,
		show,
		quit,
		shown: TrayState::Idle,
	})
}
