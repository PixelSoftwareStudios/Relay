//! Rendering. Every function draws from a [`Model`] only.

mod calls;
mod header;
mod lists;
pub mod meter;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};

use super::model::{Model, Tab};

/// Draw the whole screen.
pub fn draw(frame: &mut Frame, model: &Model) {
	let [head, tabs, body, foot] = Layout::vertical([
		Constraint::Length(3),
		Constraint::Length(1),
		Constraint::Min(3),
		Constraint::Length(1),
	])
	.areas(frame.area());
	header::header(frame, head, model);
	header::tabs(frame, tabs, model);
	match model.tab {
		Tab::Peers => lists::peers(frame, body, model),
		Tab::Calls => calls::calls(frame, body, model),
		Tab::Devices => lists::devices(frame, body, model),
		Tab::Settings => lists::settings(frame, body, model),
	}
	header::footer(frame, foot, model);
}
