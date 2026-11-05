//! Responsive layout choice. Pure functions of the available width so it is
//! tested without a window.

/// Below this width (egui points) the single-column phone layout is used.
pub const COMPACT_MAX_WIDTH: f32 = 600.0;

/// Smallest touch target in compact mode (points).
pub const MIN_TOUCH_TARGET: f32 = 44.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutClass {
	/// Phone: one column, bottom navigation, large touch targets.
	Compact,
	/// Desktop or tablet: one scrolling column of all sections.
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

/// Pages of the compact layout, shown in the bottom navigation.
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

	pub fn label(self) -> &'static str {
		match self {
			Self::Calls => "Calls",
			Self::People => "People",
			Self::Channels => "Channels",
			Self::Settings => "Settings",
		}
	}
}
