//! Whether the device is discoverable, private or not advertising: the model,
//! the sidebar chip, the Connect card and the sidebar's width.

mod phone;

use phone::{Drawn, Phone};
use relay_ui::i18n::Language;
use relay_ui::layout::Page;
use relay_ui::model::{Advert, AppState, Link};
use relay_ui::theme;
use relay_ui::viewmodel::{Discoverability, discoverability};

fn state(link: Link, advert: Option<Advert>) -> AppState {
	let mut s = phone::busy_state();
	s.link = link;
	match advert {
		Some(a) => s.settings.as_mut().unwrap().advert = a,
		None => s.settings = None,
	}
	s
}

fn has(drawn: &Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l.contains(text))
}

#[test]
fn the_model_combines_the_preference_with_the_link() {
	use Discoverability::*;
	let d = |link, advert| discoverability(&state(link, advert));
	assert_eq!(
		d(Link::Connected, Some(Advert::Visible)),
		Some(Discoverable)
	);
	assert_eq!(d(Link::Connected, Some(Advert::Hidden)), Some(Private));
	assert_eq!(d(Link::Connected, None), None);
	for link in [
		Link::Starting,
		Link::Connecting,
		Link::Reconnecting,
		Link::Offline,
	] {
		assert_eq!(d(link, Some(Advert::Visible)), Some(NotAdvertising(link)));
		assert_eq!(d(link, None), Some(NotAdvertising(link)));
		// Hidden is private whatever the link does.
		assert_eq!(d(link, Some(Advert::Hidden)), Some(Private));
	}
}

#[tokio::test]
async fn the_sidebar_chip_and_the_people_card_follow_the_state() {
	let cases = [
		(
			Link::Connected,
			Some(Advert::Visible),
			"Discoverable",
			"Private",
		),
		(
			Link::Connected,
			Some(Advert::Hidden),
			"Private",
			"Discoverable",
		),
		(
			Link::Offline,
			Some(Advert::Visible),
			"Not advertising",
			"Discoverable",
		),
	];
	for (link, advert, shown, absent) in cases {
		let mut phone = Phone::desktop(720.0, 800.0, Language::En);
		phone.show_state(&state(link, advert));
		let drawn = phone.page(Page::Connect);
		let sidebar = drawn
			.named
			.iter()
			.filter(|(_, r)| r.max.x <= theme::SIDEBAR_WIDTH)
			.map(|(t, _)| t.as_str())
			.collect::<Vec<_>>();
		assert!(sidebar.contains(&shown), "{link:?}: {sidebar:?}");
		assert!(!sidebar.contains(&absent), "{link:?}: {sidebar:?}");
		assert!(has(&drawn, shown));
	}
}

#[tokio::test]
async fn the_people_card_offers_the_switch_and_names_the_reason() {
	let mut phone = Phone::desktop(720.0, 800.0, Language::En);
	phone.show_state(&state(Link::Connected, Some(Advert::Visible)));
	let drawn = phone.page(Page::Connect);
	assert!(has(&drawn, "Go private"), "{:?}", drawn.labels);
	phone.show_state(&state(Link::Connected, Some(Advert::Hidden)));
	let drawn = phone.settle();
	assert!(has(&drawn, "Make discoverable"));
	assert!(has(&drawn, "Only someone with your address"));
	phone.show_state(&state(Link::Offline, Some(Advert::Visible)));
	let drawn = phone.settle();
	assert!(
		has(&drawn, "Not advertising: Daemon not running"),
		"{:?}",
		drawn.labels
	);
}

#[tokio::test]
async fn the_sidebar_names_this_device_even_when_the_daemon_is_down() {
	let host = gethostname::gethostname()
		.to_string_lossy()
		.trim()
		.to_owned();
	assert!(!host.is_empty());
	let mut phone = Phone::desktop(720.0, 800.0, Language::En);
	phone.show_state(&state(Link::Offline, None));
	let drawn = phone.page(Page::Connect);
	assert!(has(&drawn, &host), "{:?}", drawn.labels);
	assert!(
		!drawn.labels.iter().any(|l| l == "You"),
		"{:?}",
		drawn.labels
	);
}
