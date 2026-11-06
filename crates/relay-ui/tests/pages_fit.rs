//! The Connect page and the sidebar's identity block fit in English, German and
//! Russian, at a desktop and a phone width. One test, because the language is
//! process-wide state.

mod phone;

use phone::{LANGUAGES, Phone};
use relay_ui::layout::Page;
use relay_ui::model::{Advert, Link};
use relay_ui::theme;

#[tokio::test]
async fn connect_contacts_and_the_sidebar_fit_in_every_language() {
	let mut bad = Vec::new();
	for language in LANGUAGES {
		for (desktop, w, h) in [(true, 720.0, 800.0), (false, 360.0, 800.0)] {
			for (peers, page) in [
				(false, Page::Connect),
				(true, Page::Connect),
				(false, Page::Contacts),
				(true, Page::Contacts),
			] {
				for (link, advert) in [
					(Link::Connected, Advert::Visible),
					(Link::Connected, Advert::Hidden),
					(Link::Reconnecting, Advert::Visible),
				] {
					let mut phone = if desktop {
						Phone::desktop(w, h, language)
					} else {
						Phone::new(w, h, language)
					};
					let mut state = phone::busy_state();
					if !peers {
						state.peers.clear();
						state.calls.clear();
					}
					state.link = link;
					if let Some(s) = state.settings.as_mut() {
						s.advert = advert;
						s.display_name = "A very long display name indeed".into();
					}
					phone.show_state(&state);
					let drawn = phone.page(page);
					let screen = phone.screen().expand(1.0);
					let tag =
						format!("{language:?} {w}x{h} peers={peers} {page:?} {link:?} {advert:?}");
					for r in drawn.widgets.iter().chain(&drawn.painted) {
						if r.min.x < screen.min.x || r.max.x > screen.max.x {
							bad.push(format!("{tag}: {r:?}"));
						}
					}
					if desktop {
						for (t, r) in &drawn.named {
							if r.min.x < theme::SIDEBAR_WIDTH - 4.0
								&& r.max.x > theme::SIDEBAR_WIDTH
							{
								bad.push(format!("{tag}: sidebar text {t:?} {r:?}"));
							}
						}
					}
				}
			}
		}
	}
	bad.dedup();
	assert!(
		bad.is_empty(),
		"{} overflows: {:#?}",
		bad.len(),
		&bad[..bad.len().min(8)]
	);
}
