//! Settings, Audio: "Microphone Test". A button opens the microphone in the engine for a
//! level-only test and a row of dots follows what it hears, with the raw dB beside it: green
//! for normal levels, yellow and red near clipping, and a held peak. Nothing is sent or
//! played. The test ends when the button is pressed again, when this card stops being drawn
//! (another category, another page, the window hidden) and, at the latest, when the engine's
//! limit passes; the screen repaints only while it runs.

use std::time::Duration;

use egui::{Color32, RichText, Sense, Ui, vec2};

use crate::icons::{Icon, icon_inline, icon_label_widget};
use crate::mic_test::{DOTS, Ended, MicTest, Zone};
use crate::microphone::MicrophoneAccess;
use crate::theme;
use crate::ui::local::Local;

use super::super::kit::{self, Cx};
use crate::tr;

/// Repaint interval while the test runs (the engine reports about 16 times a second).
const FRAME: Duration = Duration::from_millis(60);

pub fn card(ui: &mut Ui, cx: &mut Cx) {
	kit::card(ui, |ui| {
		kit::card_title(ui, tr!("audio-mic-test"));
		kit::help(ui, tr!("mic-test-help"));
		let now = ui.input(|i| i.time);
		if let Some(ended) = cx.local.mic_test.tick(now, cx.meters.microphone_dbfs) {
			// Abandoned: the engine is told; timed out: it closed the microphone itself.
			if ended == Ended::Abandoned {
				cx.actions.run(|c| async move { c.stop_mic_test().await });
			}
		}
		let access = cx.actions.shell.microphone_access();
		dots(ui, &cx.local.mic_test);
		let running = cx.local.mic_test.running();
		if running {
			ui.ctx().request_repaint_after(FRAME);
			kit::help(ui, tr!("mic-test-running"));
		}
		match access {
			MicrophoneAccess::Ready => {
				let (icon, text) = if running {
					(Icon::Close, tr!("mic-test-stop"))
				} else {
					(Icon::Mic, tr!("mic-test-start"))
				};
				if ui.add(icon_label_widget(ui, icon, text)).clicked() {
					if running {
						stop(cx.local, cx.actions);
					} else {
						cx.local.mic_test.start(now);
						cx.actions.run(|c| async move { c.start_mic_test().await });
					}
				}
			}
			MicrophoneAccess::NotAsked | MicrophoneAccess::Denied => {
				kit::help(ui, tr!("mic-ask-body"));
				if ui.button(tr!("mic-allow")).clicked() {
					cx.actions.shell.request_microphone();
				}
			}
			MicrophoneAccess::Blocked => {
				kit::help(ui, tr!("mic-off-body"));
				if ui.button(tr!("mic-open-settings")).clicked() {
					cx.actions.shell.open_microphone_settings();
				}
			}
		}
	});
}

/// Ends the test now (button, category left, window hidden).
pub fn stop(local: &mut Local, actions: &crate::actions::Actions) {
	if local.mic_test.running() {
		local.mic_test.stop();
		actions.run(|c| async move { c.stop_mic_test().await });
	}
}

/// The colour of a lit dot.
pub fn zone_color(zone: Zone) -> Color32 {
	match zone {
		Zone::Green => theme::LIVE,
		Zone::Yellow => theme::WARN,
		Zone::Red => theme::ERROR,
	}
}

/// The row of dots and the number beside it.
fn dots(ui: &mut Ui, test: &MicTest) {
	let touch = crate::layout::is_touch(ui);
	let (radius, step) = if touch { (6.0, 17.0) } else { (5.0, 15.0) };
	ui.horizontal(|ui| {
		let size = vec2(step * DOTS as f32, radius * 2.0 + 8.0);
		let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
		response.widget_info(|| {
			egui::WidgetInfo::labeled(egui::Role::ProgressIndicator, true, level_text(test))
		});
		let off = ui.visuals().widgets.inactive.bg_fill;
		let peak = test.peak_dot();
		for i in 0..DOTS {
			let center = egui::pos2(rect.min.x + step * (i as f32 + 0.5), rect.center().y);
			let color = zone_color(MicTest::zone(i));
			if test.lit(i) {
				ui.painter().circle_filled(center, radius, color);
			} else if peak == Some(i) {
				ui.painter()
					.circle_stroke(center, radius - 1.0, egui::Stroke::new(2.0, color));
			} else {
				ui.painter().circle_filled(center, radius - 1.5, off);
			}
		}
		readout(ui, test);
	});
}

/// "-23 dB", or "-∞ dB" for silence (drawn with the icon font: the text font has no ∞).
fn readout(ui: &mut Ui, test: &MicTest) {
	let body = egui::TextStyle::Monospace.resolve(ui.style());
	match (test.running(), test.level()) {
		(false, _) => {
			ui.label(RichText::new("—").weak().monospace());
		}
		(true, Some(level)) => {
			ui.label(RichText::new(tr!("mic-test-db", value = format!("{level:.0}"))).monospace());
		}
		(true, None) => {
			ui.spacing_mut().item_spacing.x = 0.0;
			ui.label(RichText::new("-").font(body.clone()));
			icon_inline(ui, Icon::Infinity, ui.visuals().text_color());
			ui.label(RichText::new(" dB").font(body));
		}
	}
}

/// The reading as words, for screen readers.
fn level_text(test: &MicTest) -> String {
	match test.level() {
		Some(level) => tr!("mic-test-db", value = format!("{level:.0}")),
		None => tr!("mic-test-silent-words").to_owned(),
	}
}
