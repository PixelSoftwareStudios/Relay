//! Drawing of the safe markdown AST from `relay-messages`. There is no node
//! for HTML, images or arbitrary links, so nothing here can load or run
//! anything. A link click only asks the confirmation gate.

use egui::{Color32, RichText, Ui};
use relay_messages::markdown::{Block, CodeBlock, Document, Inline, Link};

use crate::messaging::actions::Intent;
use crate::messaging::gates::LinkGate;
use crate::theme;
use crate::tr;

#[derive(Clone, Copy, Default)]
struct Style {
	bold: bool,
	italic: bool,
}

/// Draw a document. Copy requests go to `intents`; link clicks to `gate`.
pub fn document(ui: &mut Ui, doc: &Document, gate: &mut LinkGate, intents: &mut Vec<Intent>) {
	for block in &doc.blocks {
		match block {
			Block::Paragraph(inlines) => run(ui, inlines, gate),
			Block::Code(code) => code_block(ui, code, intents),
			Block::List {
				ordered,
				start,
				items,
			} => {
				for (n, item) in items.iter().enumerate() {
					ui.horizontal_top(|ui| {
						let marker = if *ordered {
							format!("{}.", u64::from(*start) + n as u64)
						} else {
							"•".to_owned()
						};
						ui.label(marker);
						run(ui, item, gate);
					});
				}
			}
		}
	}
}

fn run(ui: &mut Ui, inlines: &[Inline], gate: &mut LinkGate) {
	ui.horizontal_wrapped(|ui| {
		ui.spacing_mut().item_spacing.x = 0.0;
		inline(ui, inlines, Style::default(), gate);
	});
}

fn inline(ui: &mut Ui, inlines: &[Inline], style: Style, gate: &mut LinkGate) {
	for node in inlines {
		match node {
			Inline::Text(t) => {
				ui.label(styled(RichText::new(t), style));
			}
			Inline::Bold(c) => inline(
				ui,
				c,
				Style {
					bold: true,
					..style
				},
				gate,
			),
			Inline::Italic(c) => inline(
				ui,
				c,
				Style {
					italic: true,
					..style
				},
				gate,
			),
			Inline::Code(c) => {
				ui.label(styled(RichText::new(c).code(), style));
			}
			Inline::LineBreak => ui.end_row(),
			Inline::Link(link) => link_widget(ui, link, gate),
		}
	}
}

fn styled(mut text: RichText, style: Style) -> RichText {
	if style.bold {
		text = text.strong();
	}
	if style.italic {
		text = text.italics();
	}
	text
}

/// A link: the URL text, the real host and a warning badge when the host is
/// internationalised or mixes scripts. Clicking only opens the confirmation.
fn link_widget(ui: &mut Ui, link: &Link, gate: &mut LinkGate) {
	let shown = if link.url.len() > 60 {
		let cut = (0..=60)
			.rev()
			.find(|i| link.url.is_char_boundary(*i))
			.unwrap_or(0);
		format!("{}…", &link.url[..cut])
	} else {
		link.url.clone()
	};
	let response = ui
		.add(
			egui::Label::new(
				RichText::new(shown)
					.underline()
					.color(ui.visuals().hyperlink_color),
			)
			.sense(egui::Sense::click()),
		)
		.on_hover_text(tr!("md-link-hover", host = link.host.ascii.as_str()));
	if let Some(w) = link.host.warning {
		let text = match w {
			relay_messages::markdown::HostWarning::Idn => tr!("md-badge-idn"),
			relay_messages::markdown::HostWarning::MixedScript => tr!("md-badge-mixed"),
			relay_messages::markdown::HostWarning::InvalidPunycode => tr!("md-badge-bad-host"),
		};
		ui.label(
			RichText::new(text)
				.color(Color32::BLACK)
				.background_color(theme::WARN)
				.small(),
		);
	}
	if response.clicked() {
		gate.ask(link);
	}
}

fn code_block(ui: &mut Ui, code: &CodeBlock, intents: &mut Vec<Intent>) {
	egui::Frame::group(ui.style())
		.fill(ui.visuals().extreme_bg_color)
		.show(ui, |ui| {
			ui.set_width(ui.available_width());
			ui.horizontal(|ui| {
				let label = code.language.as_deref().unwrap_or("code");
				ui.label(RichText::new(label).small().weak());
				if !code.closed {
					ui.label(RichText::new(tr!("md-code-unclosed")).small().weak());
				}
				ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
					if ui
						.button(tr!("action-copy"))
						.on_hover_text(tr!("md-code-copy-hover"))
						.clicked()
					{
						intents.push(Intent::Copy(code.copy_text().to_owned()));
					}
				});
			});
			ui.add(egui::Label::new(RichText::new(&code.text).monospace()).selectable(true));
		});
}
