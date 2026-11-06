//! A setting saved on its own (`SetSettings`) and the engine's answer under it: "Saved", or what
//! went wrong and what to do next.

use egui::{RichText, Ui};

use crate::engine::SettingOutcome;
use crate::theme;

use super::super::kit::Cx;
use super::super::widgets::banner;
use crate::tr;

/// Sends `value` for `key`; the answer is shown by [`answer`] once it arrives.
pub fn save(cx: &mut Cx, key: &'static str, value: String) {
	cx.local.setting_saving.insert(key);
	cx.local.setting_reports.remove(key);
	cx.actions.save_setting(key, value);
}

/// Whether a save of `key` is still on its way.
pub fn busy(cx: &Cx, key: &'static str) -> bool {
	cx.local.setting_saving.contains(key)
}

/// The engine's last answer for `key`.
pub fn answer(ui: &mut Ui, cx: &Cx, key: &'static str) {
	match cx.local.setting_reports.get(key) {
		None => {}
		Some(Err(message)) => banner(ui, message, true),
		Some(Ok(results)) => {
			for result in results.iter().filter(|r| r.key == key) {
				let reason = crate::setting_support::reason(&result.reason);
				let text = match (result.outcome, reason.is_empty()) {
					(SettingOutcome::AppliedLive, _) => RichText::new(tr!("setting-saved")).color(theme::LIVE),
					(SettingOutcome::SavedNeedsRestart, true) => RichText::new(tr!("setting-saved-restart")),
					(SettingOutcome::SavedNeedsRestart, false) => {
						RichText::new(tr!("setting-saved-restart-why", reason = reason))
					}
					(SettingOutcome::Rejected, true) => {
						RichText::new(tr!("setting-rejected")).color(theme::WARN)
					}
					(SettingOutcome::Rejected, false) => {
						RichText::new(tr!("setting-rejected-why", reason = reason)).color(theme::WARN)
					}
				};
				ui.label(text);
			}
		}
	}
}
