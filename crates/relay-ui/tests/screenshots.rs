//! Optional screenshots: a small CPU rasteriser draws egui's tessellated
//! meshes into PNG files so a person can look at the screens. Off unless
//! `RELAY_SHOTS_DIR` names a directory; no GPU or window is involved.
//!
//! `RELAY_SHOTS_DIR=/tmp/shots cargo test -p relay-ui --test screenshots`

mod chatkit;
mod phone;

use std::collections::HashMap;
use std::path::PathBuf;

use egui::epaint::{ImageData, Primitive};
use egui::{Color32, TextureId};
use phone::Phone;
use relay_ui::capabilities::SettingsCategory;
use relay_ui::i18n::Language;
use relay_ui::layout::Page;

struct Texture {
	size: [usize; 2],
	pixels: Vec<Color32>,
}

#[derive(Default)]
struct Textures(HashMap<TextureId, Texture>);

impl Textures {
	fn apply(&mut self, delta: &egui::TexturesDelta) {
		for (id, patches) in &delta.set {
			for patch in patches {
				let ImageData::Color(image) = &patch.image;
				match patch.pos {
					None => {
						self.0.insert(
							*id,
							Texture {
								size: image.size,
								pixels: image.pixels.clone(),
							},
						);
					}
					Some([x0, y0]) => {
						let Some(tex) = self.0.get_mut(id) else {
							continue;
						};
						for row in 0..image.size[1] {
							let from = row * image.size[0];
							let to = (y0 + row) * tex.size[0] + x0;
							tex.pixels[to..to + image.size[0]]
								.copy_from_slice(&image.pixels[from..from + image.size[0]]);
						}
					}
				}
			}
		}
	}
}

fn edge(a: egui::Pos2, b: egui::Pos2, p: egui::Pos2) -> f32 {
	(b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

/// Draw the primitives over an opaque `background`, premultiplied source-over
/// in gamma space as egui's own backends do.
fn rasterise(
	prims: &[egui::ClippedPrimitive],
	textures: &Textures,
	ppp: f32,
	size: egui::Vec2,
	background: Color32,
) -> image::RgbaImage {
	let (w, h) = ((size.x * ppp) as usize, (size.y * ppp) as usize);
	let mut buf = vec![
		[
			background.r() as f32 / 255.0,
			background.g() as f32 / 255.0,
			background.b() as f32 / 255.0,
		];
		w * h
	];
	for prim in prims {
		let Primitive::Mesh(mesh) = &prim.primitive else {
			continue;
		};
		let Some(tex) = textures.0.get(&mesh.texture_id) else {
			continue;
		};
		let clip = prim.clip_rect;
		let (cx0, cy0) = ((clip.min.x * ppp).max(0.0), (clip.min.y * ppp).max(0.0));
		let (cx1, cy1) = (
			(clip.max.x * ppp).min(w as f32),
			(clip.max.y * ppp).min(h as f32),
		);
		for tri in mesh.indices.as_chunks::<3>().0.iter() {
			let v = [
				mesh.vertices[tri[0] as usize],
				mesh.vertices[tri[1] as usize],
				mesh.vertices[tri[2] as usize],
			];
			let p = v.map(|v| egui::pos2(v.pos.x * ppp, v.pos.y * ppp));
			let area = edge(p[0], p[1], p[2]);
			if area.abs() < 1e-6 {
				continue;
			}
			let x0 = p
				.iter()
				.map(|p| p.x)
				.fold(f32::MAX, f32::min)
				.max(cx0)
				.floor();
			let x1 = p
				.iter()
				.map(|p| p.x)
				.fold(f32::MIN, f32::max)
				.min(cx1)
				.ceil();
			let y0 = p
				.iter()
				.map(|p| p.y)
				.fold(f32::MAX, f32::min)
				.max(cy0)
				.floor();
			let y1 = p
				.iter()
				.map(|p| p.y)
				.fold(f32::MIN, f32::max)
				.min(cy1)
				.ceil();
			let mut y = y0;
			while y < y1 {
				let mut x = x0;
				while x < x1 {
					let c = egui::pos2(x + 0.5, y + 0.5);
					let w0 = edge(p[1], p[2], c) / area;
					let w1 = edge(p[2], p[0], c) / area;
					let w2 = edge(p[0], p[1], c) / area;
					if w0 >= -0.001 && w1 >= -0.001 && w2 >= -0.001 {
						let u = w0 * v[0].uv.x + w1 * v[1].uv.x + w2 * v[2].uv.x;
						let t = w0 * v[0].uv.y + w1 * v[1].uv.y + w2 * v[2].uv.y;
						let tx = ((u * tex.size[0] as f32) as usize).min(tex.size[0] - 1);
						let ty = ((t * tex.size[1] as f32) as usize).min(tex.size[1] - 1);
						let texel = tex.pixels[ty * tex.size[0] + tx];
						let col = |i: usize| {
							let f = |c: Color32| [c.r(), c.g(), c.b(), c.a()][i] as f32 / 255.0;
							w0 * f(v[0].color) + w1 * f(v[1].color) + w2 * f(v[2].color)
						};
						let tx = |i: usize| {
							[texel.r(), texel.g(), texel.b(), texel.a()][i] as f32 / 255.0
						};
						let a = col(3) * tx(3);
						let dst = &mut buf[y as usize * w + x as usize];
						for (k, d) in dst.iter_mut().enumerate() {
							*d = col(k) * tx(k) + *d * (1.0 - a);
						}
					}
					x += 1.0;
				}
				y += 1.0;
			}
		}
	}
	let mut img = image::RgbaImage::new(w as u32, h as u32);
	for (i, px) in buf.iter().enumerate() {
		let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
		img.put_pixel(
			(i % w) as u32,
			(i / w) as u32,
			image::Rgba([c(px[0]), c(px[1]), c(px[2]), 255]),
		);
	}
	img
}

/// Run a few frames, keeping every texture update, and draw the last one.
fn shoot(phone: &mut Phone, textures: &mut Textures, path: &PathBuf) {
	let mut last = None;
	for _ in 0..5 {
		let input = egui::RawInput {
			screen_rect: Some(phone.screen()),
			..Default::default()
		};
		let app = &mut phone.app;
		let mut out = phone.ctx.run_ui(input, |ui| {
			app.begin_frame(ui.ctx());
			app.ui(ui);
		});
		textures.apply(&out.textures_delta);
		out.textures_delta.clear();
		last = Some(out);
	}
	let out = last.unwrap();
	let ppp = out.pixels_per_point;
	let prims = phone.ctx.tessellate(out.shapes, ppp);
	let bg = phone.ctx.global_style().visuals.panel_fill;
	rasterise(&prims, textures, ppp, phone.size, bg)
		.save(path)
		.unwrap();
}

#[tokio::test]
async fn screenshots() {
	let Some(dir) = std::env::var_os("RELAY_SHOTS_DIR") else {
		return;
	};
	let dir = PathBuf::from(dir);
	std::fs::create_dir_all(&dir).unwrap();
	for (name, desktop, w, h) in [
		("desktop", true, 720.0, 800.0),
		("phone", false, 360.0, 800.0),
		("desktop-tall", true, 720.0, 1700.0),
		("phone-tall", false, 360.0, 1900.0),
	] {
		for language in [Language::En, Language::De, Language::Ru] {
			let lang = format!("{language:?}").to_lowercase();
			let mut phone = if desktop {
				Phone::desktop(w, h, language)
			} else {
				Phone::new(w, h, language)
			};
			if desktop {
				phone.ctx.set_pixels_per_point(1.5);
			}
			let mut textures = Textures::default();
			for page in Page::ALL {
				phone.app.set_page(page);
				shoot(
					&mut phone,
					&mut textures,
					&dir.join(format!("{name}-{lang}-{page:?}.png")),
				);
			}
			connect_shots(&mut phone, &mut textures, &dir, &format!("{name}-{lang}"));
			for category in SettingsCategory::ALL {
				phone.app.open_settings(Some(category));
				shoot(
					&mut phone,
					&mut textures,
					&dir.join(format!("{name}-{lang}-settings-{category:?}.png")),
				);
			}
		}
	}
}

/// Connect and Contacts with nobody known (connect section open), with contacts (collapsed),
/// and the sidebar chip for a private and an offline device.
fn connect_shots(phone: &mut Phone, textures: &mut Textures, dir: &std::path::Path, tag: &str) {
	let base = phone::busy_state();
	let mut empty = base.clone();
	empty.peers.clear();
	let mut private = base.clone();
	if let Some(s) = private.settings.as_mut() {
		s.advert = relay_ui::model::Advert::Hidden;
	}
	let mut offline = base.clone();
	offline.link = relay_ui::model::Link::Offline;
	for (name, state) in [
		("connect-empty", empty),
		("connect-peers", base.clone()),
		("connect-private", private),
		("connect-offline", offline),
	] {
		phone.show_state(&state);
		phone
			.actions
			.shared
			.inbox
			.set_connection(Ok(relay_ui::model::ConnectionView {
				addresses: vec!["192.168.1.40".into()],
				port: 7000,
				fingerprint: "ab".repeat(32),
				connection_string: format!("relay://192.168.1.40:7000#{}", "ab".repeat(32)),
			}));
		phone.app.set_page(Page::Connect);
		shoot(phone, textures, &dir.join(format!("{tag}-{name}.png")));
		phone.app.set_page(Page::Contacts);
		let name = name.replace("connect", "contacts");
		shoot(phone, textures, &dir.join(format!("{tag}-{name}.png")));
	}
	phone.show_state(&base);
}

/// The chat of a call: below the call card (desktop) and full screen (phone),
/// and a history row with its Messages button, closed and open.
#[tokio::test]
async fn chat_screenshots() {
	let Some(dir) = std::env::var_os("RELAY_SHOTS_DIR") else {
		return;
	};
	let dir = PathBuf::from(dir);
	std::fs::create_dir_all(&dir).unwrap();
	for (name, desktop, w, h) in [
		("desktop", true, 720.0, 1500.0),
		("phone", false, 360.0, 800.0),
	] {
		let mut phone = if desktop {
			Phone::desktop(w, h, Language::En)
		} else {
			Phone::new(w, h, Language::En)
		};
		if desktop {
			phone.ctx.set_pixels_per_point(1.5);
		}
		let mut textures = Textures::default();
		phone.show_state(&chatkit::state(false));
		chatkit::live_messages(&phone);
		chatkit::incoming(&phone, 500);
		shoot(
			&mut phone,
			&mut textures,
			&dir.join(format!("{name}-call-badge.png")),
		);
		phone.app.open_chat(&chatkit::ada_call());
		shoot(
			&mut phone,
			&mut textures,
			&dir.join(format!("{name}-call-chat.png")),
		);
		phone.app.close_chat();
		phone.show_state(&chatkit::history_state());
		phone.size.y = 1700.0;
		shoot(
			&mut phone,
			&mut textures,
			&dir.join(format!("{name}-history-row.png")),
		);
		phone.app.open_chat(&chatkit::alan_call());
		shoot(
			&mut phone,
			&mut textures,
			&dir.join(format!("{name}-history-chat.png")),
		);
	}
}

/// Run one frame with `events`, keeping its texture updates.
fn pump(phone: &mut Phone, textures: &mut Textures, events: Vec<egui::Event>) {
	let input = egui::RawInput {
		screen_rect: Some(phone.screen()),
		events,
		..Default::default()
	};
	let app = &mut phone.app;
	let mut out = phone.ctx.run_ui(input, |ui| {
		app.begin_frame(ui.ctx());
		app.ui(ui);
	});
	textures.apply(&out.textures_delta);
	out.textures_delta.clear();
}

/// The redesigned chat: desktop and phone transcript, the open message menu
/// and a history row with its new time.
#[tokio::test]
async fn chat_design_screenshots() {
	let Some(dir) = std::env::var_os("RELAY_SHOTS_DIR") else {
		return;
	};
	let dir = PathBuf::from(dir);
	std::fs::create_dir_all(&dir).unwrap();
	for (name, desktop, w, h) in [
		("desktop", true, 720.0, 800.0),
		("phone", false, 360.0, 800.0),
	] {
		let mut phone = if desktop {
			Phone::desktop(w, h, Language::En)
		} else {
			Phone::new(w, h, Language::En)
		};
		if desktop {
			phone.ctx.set_pixels_per_point(1.5);
		}
		let mut textures = Textures::default();
		phone.show_state(&chatkit::state(false));
		chatkit::live_messages(&phone);
		phone
			.shell
			.now_ms
			.store(chatkit::NOW_MS, std::sync::atomic::Ordering::SeqCst);
		chatkit::talk(&phone);
		phone.app.open_chat(&chatkit::ada_call());
		for _ in 0..6 {
			pump(&mut phone, &mut textures, vec![]);
		}
		shoot(
			&mut phone,
			&mut textures,
			&dir.join(format!("{name}-chat.png")),
		);
		let side = if desktop { 26.0 } else { 44.0 };
		let drawn = phone.frame_with(vec![]);
		let dots = chatkit::squares(&drawn, side);
		if let Some(r) = dots.get(dots.len() / 2) {
			let pos = r.center();
			let press = |pressed| egui::Event::PointerButton {
				pos,
				button: egui::PointerButton::Primary,
				pressed,
				modifiers: Default::default(),
			};
			pump(
				&mut phone,
				&mut textures,
				vec![egui::Event::PointerMoved(pos)],
			);
			pump(&mut phone, &mut textures, vec![press(true)]);
			pump(&mut phone, &mut textures, vec![press(false)]);
			shoot(
				&mut phone,
				&mut textures,
				&dir.join(format!("{name}-chat-menu.png")),
			);
		}
		phone.app.close_chat();
		phone.show_state(&chatkit::history_state());
		phone.size.y = 1100.0;
		shoot(
			&mut phone,
			&mut textures,
			&dir.join(format!("{name}-chat-history.png")),
		);
	}
}

/// The Calls page in its states (favorites full and empty, history collapsed and open, a call in
/// each mode, the connection details) and the Settings categories of the calls area.
#[tokio::test]
async fn calls_screenshots() {
	use relay_ui::model::{HistoryDirection, HistoryEntry, HistoryOutcome, Mode};
	let Some(dir) = std::env::var_os("RELAY_SHOTS_DIR") else {
		return;
	};
	let dir = PathBuf::from(dir);
	std::fs::create_dir_all(&dir).unwrap();
	let now = 1_793_880_000_000u64;
	let history = (1..=3u64)
		.map(|i| HistoryEntry {
			id: i,
			call_id: hex::encode([i as u8; 16]),
			peer_id: hex::encode([1u8; 32]),
			peer_name: ["Ada Lovelace", "Grace Hopper", "Alan Turing"][i as usize - 1].into(),
			fingerprint_short: String::new(),
			direction: if i == 2 {
				HistoryDirection::Outgoing
			} else {
				HistoryDirection::Incoming
			},
			outcome: if i == 3 {
				HistoryOutcome::Missed
			} else {
				HistoryOutcome::Completed
			},
			started_at_ms: now - i * 3_600_000,
			duration_ms: 125_000,
		})
		.collect::<Vec<_>>();
	for (name, desktop, w, h) in [
		("desktop", true, 760.0, 900.0),
		("phone", false, 360.0, 820.0),
	] {
		let mut phone = if desktop {
			Phone::desktop(w, h, Language::En)
		} else {
			Phone::new(w, h, Language::En)
		};
		phone
			.shell
			.now_ms
			.store(now, std::sync::atomic::Ordering::SeqCst);
		if desktop {
			phone.ctx.set_pixels_per_point(1.5);
		}
		let mut textures = Textures::default();
		let mut idle = phone::busy_state();
		idle.calls.clear();
		idle.history = history.clone();
		let mut none = idle.clone();
		for p in &mut none.peers {
			p.favorite = false;
		}
		let mut nobody = idle.clone();
		nobody.peers.clear();
		let mut full = idle.clone();
		for (i, p) in full.peers.iter_mut().enumerate() {
			p.favorite = true;
			p.favorite_position = i as u32;
		}
		let mut call = phone::busy_state();
		call.calls.retain(|c| c.call_id == "02".repeat(16));
		call.history = history.clone();
		call.calls[0].connected_at_unix_ms = now - 754_000;
		let mut muted = call.clone();
		muted.calls[0].transmit.mode = Mode::Muted;
		let mut open = call.clone();
		open.calls[0].transmit.mode = Mode::Unmuted;
		open.calls[0].transmit.effective_transmitting = true;
		for (tag, state) in [
			("favorites", idle),
			("favorites-full", full),
			("favorites-empty", none),
			("favorites-nobody", nobody),
			("call-ptt", call),
			("call-muted", muted),
			("call-open", open),
			("incoming", phone::busy_state()),
		] {
			phone.show_state(&state);
			phone.app.set_page(Page::Calls);
			shoot(
				&mut phone,
				&mut textures,
				&dir.join(format!("calls-{name}-{tag}.png")),
			);
		}
		// History open.
		let mut state = phone::busy_state();
		state.calls.clear();
		state.history = history.clone();
		phone.show_state(&state);
		phone.app.set_page(Page::Calls);
		phone.size.y = 1500.0;
		phone.settle();
		phone.click_label("History");
		shoot(
			&mut phone,
			&mut textures,
			&dir.join(format!("calls-{name}-history-open.png")),
		);
		// The connection details with live numbers.
		let mut state = phone::busy_state();
		state.calls.retain(|c| c.call_id == "02".repeat(16));
		state.calls[0].connected_at_unix_ms = now - 754_000;
		phone.show_state(&state);
		phone
			.slot
			.set(std::sync::Arc::new(phone::stats::Ops::default()));
		phone.app.set_page(Page::Calls);
		phone.settle();
		phone.frame_with(vec![egui::Event::Key {
			key: egui::Key::D,
			physical_key: None,
			pressed: true,
			repeat: false,
			modifiers: egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
		}]);
		let id = "02".repeat(16);
		for _ in 0..200 {
			if phone.actions.shared.stats.latest(&id).is_some() {
				break;
			}
			tokio::time::sleep(std::time::Duration::from_millis(10)).await;
		}
		shoot(
			&mut phone,
			&mut textures,
			&dir.join(format!("calls-{name}-details.png")),
		);
		phone.click_label(relay_ui::i18n::t("action-details"));
		shoot(
			&mut phone,
			&mut textures,
			&dir.join(format!("calls-{name}-details-more.png")),
		);
		phone.size.y = h;
		let mut off = phone::busy_state();
		off.settings.as_mut().unwrap().ptt_enabled = false;
		off.settings
			.as_mut()
			.unwrap()
			.effective_default_transmit_mode = Mode::Muted;
		for (tag, state) in [("on", phone::busy_state()), ("off", off)] {
			phone.show_state(&state);
			for category in [
				SettingsCategory::Profile,
				SettingsCategory::PushToTalk,
				SettingsCategory::Calls,
			] {
				phone.app.open_settings(Some(category));
				shoot(
					&mut phone,
					&mut textures,
					&dir.join(format!("calls-{name}-settings-{category:?}-{tag}.png")),
				);
			}
		}
	}
}
