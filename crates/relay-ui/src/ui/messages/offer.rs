//! A file offer as a card. Only metadata is shown; the file itself never
//! travels in chat, is never opened and is never previewed automatically.

use egui::{RichText, Ui};

use crate::messaging::actions::Intent;
use crate::messaging::model::{OfferStatus, OfferView, SessionView, SettingsView};
use crate::messaging::rules::{
	format_size, hash_short, offer_actions, offer_warnings, progress, status_label, type_guess,
};
use crate::theme;
use crate::tr;

pub fn card(
	ui: &mut Ui,
	offer: &OfferView,
	session: &SessionView,
	settings: Option<SettingsView>,
	intents: &mut Vec<Intent>,
) {
	let key = session.key.clone();
	egui::Frame::group(ui.style()).show(ui, |ui| {
		ui.set_width(ui.available_width());
		ui.label(RichText::new(&offer.name).strong());
		ui.label(
			RichText::new(tr!(
				"offer-summary",
				size = format_size(offer.size),
				type = type_guess(&offer.name, &offer.media_type, offer.executable_warning),
				hash = hash_short(&offer.blake3)
			))
			.small()
			.weak(),
		)
		.on_hover_text(tr!("offer-hash-hover"));
		let failed = matches!(offer.status, OfferStatus::Failed | OfferStatus::Rejected);
		let status = RichText::new(status_label(offer));
		ui.label(if failed {
			status.color(theme::ERROR)
		} else {
			status
		});
		if offer.status == OfferStatus::InProgress {
			ui.add(
				egui::ProgressBar::new(progress(offer))
					.animate(false)
					.desired_width(ui.available_width().min(260.0)),
			);
		}
		if offer.status == OfferStatus::Completed && !offer.outgoing && !offer.saved_as.is_empty() {
			ui.label(RichText::new(tr!("offer-saved-as", name = offer.saved_as.as_str())).small());
		}
		for warning in offer_warnings(offer, settings.map(|s| s.max_offer_size_mib)) {
			ui.label(RichText::new(warning).small().color(theme::WARN));
		}
		if !offer.preview.is_empty() {
			ui.label(RichText::new(tr!("offer-preview-approved")).small().weak());
			ui.add(
				egui::Label::new(RichText::new(&offer.preview).monospace().small())
					.selectable(true),
			);
		} else if offer.preview_requested {
			ui.label(RichText::new(tr!("offer-preview-waiting")).small().weak());
		}
		if offer.preview_approval_pending {
			ui.label(
				RichText::new(tr!("offer-preview-asked"))
					.small()
					.color(theme::WARN),
			);
		}
		let a = offer_actions(offer, session);
		ui.horizontal_wrapped(|ui| {
			let id = offer.offer_id.clone();
			if a.download
				&& ui
					.button(tr!("offer-download"))
					.on_hover_text(tr!("offer-download-hover"))
					.clicked()
			{
				intents.push(Intent::Download {
					session: key.clone(),
					offer: id.clone(),
				});
			}
			if a.reject && ui.button(tr!("offer-reject")).clicked() {
				intents.push(Intent::Reject {
					session: key.clone(),
					offer: id.clone(),
				});
			}
			if a.request_preview
				&& ui
					.button(tr!("offer-request-preview"))
					.on_hover_text(tr!("offer-request-preview-hover"))
					.clicked()
			{
				intents.push(Intent::RequestPreview {
					session: key.clone(),
					offer: id.clone(),
				});
			}
			if a.approve_preview {
				if ui
					.button(tr!("offer-approve-preview"))
					.on_hover_text(tr!("offer-approve-preview-hover"))
					.clicked()
				{
					intents.push(Intent::ApprovePreview {
						session: key.clone(),
						offer: id.clone(),
						approve: true,
					});
				}
				if ui.button(tr!("offer-refuse-preview")).clicked() {
					intents.push(Intent::ApprovePreview {
						session: key.clone(),
						offer: id.clone(),
						approve: false,
					});
				}
			}
			if a.cancel && ui.button(tr!("action-cancel")).clicked() {
				intents.push(Intent::Cancel {
					session: key.clone(),
					offer: id,
				});
			}
		});
	});
}
