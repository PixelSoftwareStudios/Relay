//! The hidden "Connection details" panel of a call card: live latency and quality numbers for
//! someone chasing a bad call. It opens from the card's menu or with Ctrl/Cmd+Shift+D. The
//! engine's statistics stream is open only while the panel is drawn (see `stats_watch`).

use egui::{self, RichText, Ui};

use crate::model::{CallStats, MediaPath, Phase};
use crate::theme;
use crate::viewmodel::CallCard;

use super::kit::{self, Cx};
use super::local::Local;
use crate::tr;

/// The panel of `card`, when the user opened it for this call.
pub fn show(ui: &mut Ui, card: &CallCard, cx: &mut Cx) {
	if cx.local.details_call.as_deref() != Some(card.call_id.as_str())
		|| !matches!(card.phase, Phase::Connected | Phase::Interrupted)
	{
		return;
	}
	cx.local.details_drawn = true;
	cx.actions.watch_stats(&card.call_id);
	let stats = cx.actions.shared.stats.latest(&card.call_id);
	ui.add_space(theme::SPACE_S);
	ui.separator();
	kit::card_title(ui, tr!("details-title"));
	let Some(s) = stats else {
		kit::help(ui, tr!("details-waiting"));
		return;
	};
	egui::Grid::new(("call-details", &card.call_id))
		.num_columns(2)
		.spacing([theme::SPACE_L, theme::SPACE_S])
		.show(ui, |ui| {
			for (label, value) in rows(&s) {
				ui.label(RichText::new(label).weak());
				ui.add(egui::Label::new(value).wrap());
				ui.end_row();
			}
		});
	kit::details(ui, ("call-details-more", &card.call_id), |ui| {
		egui::Grid::new(("call-details-more-grid", &card.call_id))
			.num_columns(2)
			.spacing([theme::SPACE_L, theme::SPACE_S])
			.show(ui, |ui| {
				for (label, value) in more_rows(&s) {
					ui.label(RichText::new(label).weak());
					ui.add(egui::Label::new(value).wrap());
					ui.end_row();
				}
			});
	});
}

/// Ctrl/Cmd+Shift+D: open the panel of the first live call, or close it.
pub fn toggle_shortcut(local: &mut Local, state: &crate::model::AppState) {
	if local.details_call.is_some() {
		local.details_call = None;
		return;
	}
	local.details_call = state
		.calls
		.iter()
		.find(|c| matches!(c.phase, Phase::Connected | Phase::Interrupted))
		.map(|c| c.call_id.clone());
}

fn ms(value: f64) -> String {
	tr!("stat-ms", value = format!("{value:.0}"))
}

fn ms_or_none(value: Option<f64>) -> String {
	value.map_or_else(|| tr!("stat-none").to_owned(), ms)
}

fn percent(value: f64) -> String {
	tr!("stat-percent", value = format!("{value:.1}"))
}

fn yes_no(value: bool) -> &'static str {
	if value {
		tr!("stat-yes")
	} else {
		tr!("stat-no")
	}
}

/// The path state in words a person can act on.
fn path_text(s: &CallStats) -> String {
	match s.path {
		MediaPath::Connected => tr!("details-path-ok").to_owned(),
		MediaPath::Waiting if s.path_reason.is_empty() => tr!("details-path-waiting").to_owned(),
		MediaPath::Waiting => tr!("details-path-waiting-why", reason = reason(&s.path_reason)),
		MediaPath::Failed if s.path_reason.is_empty() => tr!("details-path-failed").to_owned(),
		MediaPath::Failed => tr!("details-path-failed-why", reason = reason(&s.path_reason)),
	}
}

fn reason(text: &str) -> &str {
	crate::engine::plain_refusal(text).trim_end_matches('.')
}

fn rows(s: &CallStats) -> Vec<(&'static str, String)> {
	let rtt = match (s.rtt_ms, s.rtt_min_ms, s.rtt_max_ms) {
		(Some(now), Some(min), Some(max)) => {
			tr!("details-range", value = ms(now), min = ms(min), max = ms(max))
		}
		(now, ..) => ms_or_none(now),
	};
	let delay = s.mouth_to_ear_ms.map_or_else(
		|| tr!("stat-none").to_owned(),
		|v| tr!("details-estimate", value = ms(v)),
	);
	vec![
		(tr!("details-path"), path_text(s)),
		(
			tr!("details-traffic"),
			tr!(
				"details-traffic-value",
				sending = yes_no(s.sending),
				receiving = yes_no(s.receiving)
			),
		),
		(tr!("details-rtt"), rtt),
		(tr!("details-jitter"), ms_or_none(s.jitter_ms)),
		(
			tr!("details-loss"),
			tr!(
				"details-loss-value",
				now = percent(s.loss_percent_5s),
				total = percent(s.loss_percent_total)
			),
		),
		(
			tr!("details-buffer"),
			tr!(
				"details-buffer-value",
				depth = ms(s.jitter_buffer_depth_ms),
				target = ms(s.jitter_buffer_target_ms)
			),
		),
		(tr!("details-delay"), delay),
		(
			tr!("details-bitrate"),
			tr!(
				"stat-kbps",
				value = format!("{:.0}", f64::from(s.codec_bitrate_bps) / 1000.0)
			),
		),
	]
}

fn more_rows(s: &CallStats) -> Vec<(&'static str, String)> {
	let route = match s.transport.as_str() {
		"lan" => tr!("details-route-lan").to_owned(),
		"overlay" => tr!("details-route-overlay").to_owned(),
		other => other.to_owned(),
	};
	let addresses = if s.local_addr.is_empty() && s.remote_addr.is_empty() {
		tr!("stat-none").to_owned()
	} else {
		format!("{} \u{2192} {}", s.local_addr, s.remote_addr)
	};
	vec![
		(tr!("details-route"), route),
		(tr!("details-addresses"), addresses),
		(
			tr!("details-packets"),
			tr!(
				"details-packets-value",
				sent = s.packets_sent.to_string(),
				received = s.packets_received.to_string()
			),
		),
		(
			tr!("details-drift"),
			s.clock_drift_ppm.map_or_else(
				|| tr!("stat-none").to_owned(),
				|v| tr!("stat-ppm", value = format!("{v:+.0}")),
			),
		),
		(
			tr!("details-device-buffers"),
			tr!(
				"details-device-buffers-value",
				capture = ms(s.capture_buffer_ms),
				playback = ms(s.playback_buffer_ms)
			),
		),
	]
}
