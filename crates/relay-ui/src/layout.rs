//! Responsive layout choice. Pure functions of the available width so it is
//! tested without a window.

use crate::tr;

/// Below this width (egui points) the single-column phone layout is used.
pub const COMPACT_MAX_WIDTH: f32 = 600.0;

/// Smallest touch target in compact mode (points).
pub const MIN_TOUCH_TARGET: f32 = 44.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutClass {
	/// Phone: one column, bottom navigation, large touch targets.
	Compact,
	/// Desktop or tablet: a sidebar and one page at a time.
	Regular,
}

impl LayoutClass {
	/// Choose from the available width; non-finite or non-positive widths
	/// (a window not yet laid out) count as regular.
	pub fn for_width(width: f32) -> Self {
		if width.is_finite() && width > 0.0 && width < COMPACT_MAX_WIDTH {
			Self::Compact
		} else {
			Self::Regular
		}
	}

	pub fn is_compact(self) -> bool {
		self == Self::Compact
	}

	/// Minimum height of an interactive widget in this layout.
	pub fn min_touch(self) -> f32 {
		match self {
			Self::Compact => MIN_TOUCH_TARGET,
			Self::Regular => 18.0,
		}
	}
}

/// Whether widgets are sized for touch (the compact style is installed), so
/// information must not depend on hovering.
pub fn is_touch(ui: &egui::Ui) -> bool {
	ui.spacing().interact_size.y >= MIN_TOUCH_TARGET
}

/// Pages of the app. The compact layout shows all four as bottom tabs; the
/// regular layout lists the first three in the sidebar and opens Settings from
/// the gear button. Session messages are not a page: they belong to a call and
/// open from its card or its history row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
	#[default]
	Calls,
	People,
	Channels,
	Settings,
}

impl Page {
	pub const ALL: [Page; 4] = [Page::Calls, Page::People, Page::Channels, Page::Settings];

	/// The pages of the regular layout's sidebar (Settings has its own button).
	pub const SIDEBAR: [Page; 3] = [Page::Calls, Page::People, Page::Channels];

	pub fn label(self) -> &'static str {
		match self {
			Self::Calls => tr!("calls-heading"),
			Self::People => tr!("nav-people"),
			Self::Channels => tr!("channels-heading"),
			Self::Settings => tr!("nav-settings"),
		}
	}
}
