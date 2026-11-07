//! The icon set: Phosphor glyphs from the bundled icon font (`assets/font`),
//! drawn through their own egui families so they never fall back through the
//! text fonts, plus the button helpers every page uses.
//!
//! Rules of use: an icon stands alone only where its meaning is universal
//! (mute, send, close, back, search, copy, more, star). Anywhere else it sits
//! beside a visible label ([`icon_label`]). Every icon-only control carries a
//! tooltip that doubles as its accessible name, so screen readers read it.

use egui::{
	Align2, Color32, FontFamily, FontId, Response, RichText, Sense, TextFormat, Ui, WidgetInfo,
	text::LayoutJob, vec2,
};

use crate::fonts::{ICON_FAMILY, ICON_FILL_FAMILY};
use crate::layout::MIN_TOUCH_TARGET;
use crate::ui::icon_style::{self, Tone};

/// Side of an icon-only button with a pointer (points).
pub const DESKTOP_HIT: f32 = 32.0;

/// One icon. The codepoints are Phosphor's; `assets/font/icon-glyphs.txt` lists
/// the glyphs kept in the subset and the `icon_font` test checks them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
	Call,
	HangUp,
	Mic,
	MicOff,
	Headphones,
	HeadphonesOff,
	PushToTalk,
	Volume,
	Chat,
	Settings,
	Contacts,
	Connect,
	Channels,
	Nearby,
	Copy,
	Pin,
	More,
	Send,
	Attach,
	Search,
	Scan,
	Qr,
	Star,
	StarFilled,
	Trash,
	Check,
	Close,
	Back,
	ChevronRight,
	ChevronDown,
	ChevronLeft,
	Info,
	Warning,
	Refresh,
	Share,
	Lock,
	Unlock,
	Wifi,
	WifiOff,
	Trust,
	Eye,
	EyeOff,
	Add,
	Edit,
	Image,
	Camera,
	Globe,
	Success,
	Error,
	Clock,
	Bell,
	AddContact,
	SignOut,
	Devices,
	/// A call that came in (arrow down-left).
	CallIncoming,
	/// A call placed from this device (arrow up-right).
	CallOutgoing,
	/// A call nobody answered: missed, declined, cancelled or not reached.
	CallMissed,
	/// The infinity sign (the latin text font has none): a silent level, `-∞ dB`.
	Infinity,
}

impl Icon {
	/// Every icon, for tests and galleries.
	pub const ALL: [Icon; 58] = [
		Icon::Call,
		Icon::HangUp,
		Icon::Mic,
		Icon::MicOff,
		Icon::Headphones,
		Icon::HeadphonesOff,
		Icon::PushToTalk,
		Icon::Volume,
		Icon::Chat,
		Icon::Settings,
		Icon::Contacts,
		Icon::Connect,
		Icon::Channels,
		Icon::Nearby,
		Icon::Copy,
		Icon::Pin,
		Icon::More,
		Icon::Send,
		Icon::Attach,
		Icon::Search,
		Icon::Scan,
		Icon::Qr,
		Icon::Star,
		Icon::StarFilled,
		Icon::Trash,
		Icon::Check,
		Icon::Close,
		Icon::Back,
		Icon::ChevronRight,
		Icon::ChevronDown,
		Icon::ChevronLeft,
		Icon::Info,
		Icon::Warning,
		Icon::Refresh,
		Icon::Share,
		Icon::Lock,
		Icon::Unlock,
		Icon::Wifi,
		Icon::WifiOff,
		Icon::Trust,
		Icon::Eye,
		Icon::EyeOff,
		Icon::Add,
		Icon::Edit,
		Icon::Image,
		Icon::Camera,
		Icon::Globe,
		Icon::Success,
		Icon::Error,
		Icon::Clock,
		Icon::Bell,
		Icon::AddContact,
		Icon::SignOut,
		Icon::Devices,
		Icon::CallIncoming,
		Icon::CallOutgoing,
		Icon::CallMissed,
		Icon::Infinity,
	];

	/// The glyph's codepoint in the icon font.
	pub fn codepoint(self) -> char {
		match self {
			Icon::Call => '\u{E3B8}',
			Icon::HangUp => '\u{E3BC}',
			Icon::Mic => '\u{E326}',
			Icon::MicOff => '\u{E328}',
			Icon::Headphones => '\u{E2A6}',
			Icon::HeadphonesOff => '\u{E45A}',
			Icon::PushToTalk => '\u{EC90}',
			Icon::Volume => '\u{E44A}',
			Icon::Chat => '\u{E168}',
			Icon::Settings => '\u{E270}',
			Icon::Contacts => '\u{E4D6}',
			Icon::Connect => '\u{E2E2}',
			Icon::Channels => '\u{E2A2}',
			Icon::Nearby => '\u{E0F2}',
			Icon::Copy => '\u{E1CA}',
			Icon::Pin => '\u{E3E2}',
			Icon::More => '\u{E208}',
			Icon::Send => '\u{E396}',
			Icon::Attach => '\u{E39A}',
			Icon::Search => '\u{E30C}',
			Icon::Scan => '\u{EBB6}',
			Icon::Qr => '\u{E3E6}',
			Icon::Star | Icon::StarFilled => '\u{E46A}',
			Icon::Trash => '\u{E4A6}',
			Icon::Check => '\u{E182}',
			Icon::Close => '\u{E4F6}',
			Icon::Back => '\u{E058}',
			Icon::ChevronRight => '\u{E13A}',
			Icon::ChevronDown => '\u{E136}',
			Icon::ChevronLeft => '\u{E138}',
			Icon::Info => '\u{E2CE}',
			Icon::Warning => '\u{E4E0}',
			Icon::Refresh => '\u{E094}',
			Icon::Share => '\u{E408}',
			Icon::Lock => '\u{E2FA}',
			Icon::Unlock => '\u{E306}',
			Icon::Wifi => '\u{E4EA}',
			Icon::WifiOff => '\u{E4F2}',
			Icon::Trust => '\u{E40C}',
			Icon::Eye => '\u{E220}',
			Icon::EyeOff => '\u{E224}',
			Icon::Add => '\u{E3D4}',
			Icon::Edit => '\u{E3B4}',
			Icon::Image => '\u{E2CA}',
			Icon::Camera => '\u{E10E}',
			Icon::Globe => '\u{E288}',
			Icon::Success => '\u{E184}',
			Icon::Error => '\u{E4F8}',
			Icon::Clock => '\u{E19A}',
			Icon::Bell => '\u{E0CE}',
			Icon::AddContact => '\u{E4D0}',
			Icon::SignOut => '\u{E42A}',
			Icon::Devices => '\u{EBA4}',
			Icon::CallIncoming => '\u{E040}',
			Icon::CallOutgoing => '\u{E092}',
			Icon::CallMissed => '\u{E3C4}',
			Icon::Infinity => '\u{E634}',
		}
	}

	/// The egui family that holds this glyph: the filled face for
	/// [`Icon::StarFilled`], the regular one for everything else.
	pub fn family(self) -> FontFamily {
		let name = if self == Icon::StarFilled {
			ICON_FILL_FAMILY
		} else {
			ICON_FAMILY
		};
		FontFamily::Name(name.into())
	}

	/// The font to draw this icon at `size` points.
	pub fn font(self, size: f32) -> FontId {
		FontId::new(size, self.family())
	}

	/// The icon as styled text, for labels and buttons.
	pub fn text(self, size: f32) -> RichText {
		RichText::new(self.codepoint().to_string()).font(self.font(size))
	}
}

/// The icon of a page, for the sidebar and the bottom tabs.
pub fn page_icon(page: crate::layout::Page) -> Icon {
	use crate::layout::Page;
	match page {
		Page::Calls => Icon::Call,
		Page::Contacts => Icon::Contacts,
		Page::Connect => Icon::Connect,
		Page::Channels => Icon::Channels,
		Page::Settings => Icon::Settings,
	}
}

/// Side of an icon-only button: 44 points under touch rules, 32 with a pointer.
pub fn hit_size(ui: &Ui) -> f32 {
	if crate::layout::is_touch(ui) {
		MIN_TOUCH_TARGET
	} else {
		DESKTOP_HIT
	}
}

/// Glyph size inside an icon-only button of side `hit`.
fn glyph_size(hit: f32) -> f32 {
	(hit * 0.55).round()
}

/// An icon-only button as a widget, so it works with `ui.add_enabled`.
pub struct IconButton<'a> {
	icon: Icon,
	tooltip: &'a str,
	tone: Tone,
	on: bool,
}

impl<'a> IconButton<'a> {
	/// A plain button; the tooltip is also its accessible name.
	pub fn new(icon: Icon, tooltip: &'a str) -> Self {
		Self {
			icon,
			tooltip,
			tone: Tone::Plain,
			on: false,
		}
	}

	/// Shown pressed when `on` (a toggle).
	pub fn toggled(mut self, on: bool) -> Self {
		self.on = on;
		self
	}

	/// Red, for hard-to-undo acts.
	pub fn danger(mut self) -> Self {
		self.tone = Tone::Danger;
		self
	}

	/// Round and filled in the accent colour (send).
	pub fn accent(mut self) -> Self {
		self.tone = Tone::Accent;
		self
	}
}

impl egui::Widget for IconButton<'_> {
	fn ui(self, ui: &mut Ui) -> Response {
		draw_button(ui, self.icon, self.tooltip, self.tone, self.on)
	}
}

/// An icon-only button. The tooltip is shown on hover and is the control's
/// accessible name, so it must say what the button does ("Copy address").
pub fn icon_button(ui: &mut Ui, icon: Icon, tooltip: &str) -> Response {
	ui.add(IconButton::new(icon, tooltip))
}

/// A toggle button: `on` shows it pressed. The tooltip names the action a
/// press would do ("Add to favorites"), and the accessible state says on or off.
pub fn icon_toggle(ui: &mut Ui, on: bool, icon: Icon, tooltip: &str) -> Response {
	ui.add(IconButton::new(icon, tooltip).toggled(on))
}

/// An icon-only button for a hard-to-undo act (hang up, delete): red.
pub fn icon_button_danger(ui: &mut Ui, icon: Icon, tooltip: &str) -> Response {
	ui.add(IconButton::new(icon, tooltip).danger())
}

/// A round filled icon button in the accent colour (send). Disabled, it fades.
pub fn icon_button_accent(ui: &mut Ui, icon: Icon, tooltip: &str) -> Response {
	ui.add(IconButton::new(icon, tooltip).accent())
}

fn draw_button(ui: &mut Ui, icon: Icon, tooltip: &str, tone: Tone, on: bool) -> Response {
	debug_assert!(!tooltip.is_empty(), "an icon button needs a tooltip");
	let side = hit_size(ui);
	let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
	response.widget_info(|| {
		let mut info = WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), tooltip);
		info.selected = Some(on);
		info
	});
	if ui.is_rect_visible(rect) {
		let paint = icon_style::paint(ui, &response, tone, on);
		let painter = ui.painter();
		if paint.fill != Color32::TRANSPARENT || paint.stroke.width > 0.0 {
			painter.rect(
				rect,
				if tone == Tone::Accent {
					egui::CornerRadius::same((side / 2.0) as u8)
				} else {
					theme_radius()
				},
				paint.fill,
				paint.stroke,
				egui::StrokeKind::Inside,
			);
		}
		painter.text(
			rect.center(),
			Align2::CENTER_CENTER,
			icon.codepoint(),
			icon.font(glyph_size(side)),
			paint.glyph,
		);
	}
	response.on_hover_text(tooltip)
}

fn theme_radius() -> egui::CornerRadius {
	egui::CornerRadius::same(crate::theme::CARD_RADIUS)
}

/// The layout of `icon  text` for a button: the icon in its family, the text in
/// the body font, both in the button's text colour.
fn icon_text_job(ui: &Ui, icon: Icon, text: &str) -> LayoutJob {
	let body = egui::TextStyle::Button.resolve(ui.style());
	let mut job = LayoutJob::default();
	job.append(
		&icon.codepoint().to_string(),
		0.0,
		TextFormat::simple(icon.font(body.size + 2.0), Color32::PLACEHOLDER),
	);
	job.append(
		text,
		theme_gap(),
		TextFormat::simple(body, Color32::PLACEHOLDER),
	);
	job
}

fn theme_gap() -> f32 {
	crate::theme::SPACE_M
}

/// A button with an icon and a visible label, for actions whose meaning an
/// icon alone does not carry (Call, Trust this device).
pub fn icon_label(ui: &mut Ui, icon: Icon, text: &str) -> Response {
	ui.add(icon_label_widget(ui, icon, text))
}

/// [`icon_label`], disabled unless `enabled`.
pub fn icon_label_enabled(ui: &mut Ui, enabled: bool, icon: Icon, text: &str) -> Response {
	let widget = icon_label_widget(ui, icon, text);
	ui.add_enabled(enabled, widget)
}

/// [`icon_label`] across the whole width, at least touch height: the main action of a card.
pub fn icon_label_full(ui: &mut Ui, enabled: bool, icon: Icon, text: &str) -> Response {
	let height = ui.spacing().interact_size.y.max(MIN_TOUCH_TARGET);
	let widget = icon_label_widget(ui, icon, text).min_size(vec2(ui.available_width(), height));
	ui.add_enabled(enabled, widget)
}

/// [`icon_label`] as a widget, for `ui.add_enabled`.
pub fn icon_label_widget(ui: &Ui, icon: Icon, text: &str) -> egui::Button<'static> {
	egui::Button::new(icon_text_job(ui, icon, text))
}

/// [`icon_label`] in the primary (accept) colour.
pub fn icon_label_primary(ui: &mut Ui, icon: Icon, text: &str) -> Response {
	let job = icon_text_job(ui, icon, text);
	ui.add(egui::Button::new(job).fill(icon_style::PRIMARY_FILL))
}

/// [`icon_label`] in red, for hang up and other hard-to-undo acts.
pub fn icon_label_danger(ui: &mut Ui, icon: Icon, text: &str) -> Response {
	let job = icon_text_job(ui, icon, text);
	ui.add(egui::Button::new(job).fill(icon_style::DANGER_FILL))
}

/// A flat menu entry with an icon, its text in the error colour, for
/// destructive choices (forget a device, clear a history).
pub fn icon_label_red(ui: &mut Ui, icon: Icon, text: &str) -> Response {
	let red = crate::theme::ERROR;
	let body = egui::TextStyle::Button.resolve(ui.style());
	let mut job = LayoutJob::default();
	job.append(
		&icon.codepoint().to_string(),
		0.0,
		TextFormat::simple(icon.font(body.size + 2.0), red),
	);
	job.append(text, theme_gap(), TextFormat::simple(body, red));
	ui.add(egui::Button::new(job))
}

/// An icon drawn inline, before a line of text (status rows, banners).
pub fn icon_inline(ui: &mut Ui, icon: Icon, color: Color32) -> Response {
	let size = egui::TextStyle::Body.resolve(ui.style()).size + 2.0;
	ui.label(icon.text(size).color(color))
}
