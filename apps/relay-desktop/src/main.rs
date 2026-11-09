use std::sync::Arc;

use relay_desktop::app::DesktopApp;
use relay_desktop::background::{Background, BackgroundSink};
use relay_desktop::host::DesktopShell;
use relay_desktop::hotkey::{self, HotkeyHost, HotkeyShared};
use relay_desktop::login::LoginItem;
use relay_desktop::notify::{Dispatcher, NotifySink, Reply, Responder, SystemNotifier};
use relay_desktop::options::DesktopOptions;
use relay_desktop::prefs::DesktopPrefs;
use relay_desktop::ptt::PttControl;
use relay_desktop::tray;
use relay_desktop::updates::UpdateService;
use relay_ui::actions::{Actions, Shared, ShellSink};
use relay_ui::background_settings::KEY_BACKGROUND;
use relay_ui::controller::{Controller, SessionSlot};
use relay_ui::daemon_control;
use relay_ui::emitter::{self, Cadence};
use relay_ui::host_options::HostOptions;
use relay_ui::hub::Hub;
use relay_ui::imaging::decode_for_texture;
use relay_ui::messaging::link::MsgLink;
use relay_ui::relay_link::{RelayLink, resolve_endpoint};
use relay_ui::shell::Shell;
use relay_ui::ui::RelayApp;
use relay_ui::{messaging, mock, supervisor};

/// Embedded from the repo-root assets folder.
/// macOS draws the icon as given in the Dock, so it uses the padded, shadowed
/// artwork that follows the macOS icon grid; other systems use the edge-to-edge mark.
#[cfg(target_os = "macos")]
const WINDOW_ICON_PNG: &[u8] = include_bytes!("../../../assets/icon/relay-app-512.png");
#[cfg(not(target_os = "macos"))]
const WINDOW_ICON_PNG: &[u8] = include_bytes!("../../../assets/icon/relay-mark-256.png");

fn main() -> eframe::Result {
	// A daemon started by this binary (autostart, "Start daemon") runs here, first.
	relay_daemon::exit_if_requested();
	tracing_subscriber::fmt()
		.with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
		.init();
	let mock_mode =
		std::env::args().any(|a| a == "--mock") || std::env::var_os("RELAY_DESKTOP_MOCK").is_some();
	let runtime = tokio::runtime::Builder::new_multi_thread()
		.worker_threads(2)
		.thread_name("relay-desktop-io")
		.enable_all()
		.build()
		.expect("tokio runtime");
	let window_icon = decode_for_texture(WINDOW_ICON_PNG).expect("embedded window icon decodes");

	let viewport = egui::ViewportBuilder::default()
		.with_title("Relay")
		.with_inner_size([720.0, 800.0])
		.with_min_inner_size([380.0, 480.0])
		.with_icon(egui::IconData {
			rgba: window_icon.rgba,
			width: window_icon.size[0] as u32,
			height: window_icon.size[1] as u32,
		});
	let options = eframe::NativeOptions {
		viewport,
		..Default::default()
	};

	eframe::run_native(
		"Relay",
		options,
		Box::new(move |cc| {
			let ctx = cc.egui_ctx.clone();
			let dispatcher = Arc::new(Dispatcher::new(Arc::new(SystemNotifier)));
			let options = Arc::new(DesktopOptions::new(
				DesktopPrefs::default_path(),
				dispatcher.clone(),
				LoginItem::system(),
				Box::new(|| sys_locale::get_locales().collect()),
			));
			// Before the tray and the first frame, so everything starts in the right language.
			relay_ui::i18n::set_active(options.language());
			let hub = Arc::new(Hub::default());
			let ptt = Arc::new(PttControl::new(
				hub.clone(),
				runtime.handle().clone(),
				Box::new(|| resolve_endpoint().map_err(|e| e.to_string())),
			));
			let hotkey_state = Arc::new(HotkeyShared::default());
			let background = Arc::new(Background::default());
			let host = Arc::new(
				DesktopShell::new(ctx.clone(), options.clone())
					.with_background(background.clone())
					.with_push_to_talk(ptt.clone(), hotkey_state.clone())
					.with_updates(updates(runtime.handle().clone(), mock_mode)),
			);
			// The system prompt appears now, not at the first call.
			host.start_microphone();
			let shell: Arc<dyn Shell> = host.clone();
			let slot = Arc::new(SessionSlot::default());
			let shared = Arc::new(Shared::default());
			let controller = Arc::new(Controller::new(hub.clone(), slot.clone()));
			let actions = Actions {
				controller,
				hub: hub.clone(),
				shared: shared.clone(),
				runtime: runtime.handle().clone(),
				shell: shell.clone(),
			};
			let shell_sink = Arc::new(ShellSink {
				shared: shared.clone(),
				shell,
			});
			let (switch, front, repaint) = (options, host.clone(), host.clone());
			let respond = notification_replies(&actions);
			let notify = Arc::new(
				NotifySink::new(
					shell_sink,
					dispatcher,
					Arc::new(move || switch.notify_incoming()),
					Arc::new(move || front.in_front()),
				)
				.with_responder(respond),
			);
			let sink = Arc::new(BackgroundSink::new(
				notify,
				background.clone(),
				Arc::new(move || repaint.request_repaint()),
			));
			runtime.spawn(emitter::run(hub.clone(), sink, Cadence::default()));
			if mock_mode {
				runtime.spawn(mock::run(hub, slot));
				runtime.spawn(messaging::mock::run(
					shared.messages.clone(),
					actions.shell.clone(),
				));
			} else {
				let msg_engine = Arc::new(MsgLink::new(resolve_endpoint));
				runtime.spawn(messaging::supervisor::supervise(
					msg_engine,
					shared.messages.clone(),
					actions.shell.clone(),
				));
				runtime.spawn(hotkey::follow_settings(
					hub.clone(),
					actions.controller.clone(),
					actions.shell.clone(),
				));
				let engine = Arc::new(RelayLink::new(resolve_endpoint));
				let (shell, autostart, shared) =
					(actions.shell.clone(), shared.autostart.clone(), shared);
				runtime.spawn(async move {
					// Once per launch, before the first connect: an idle daemon left over from
					// an earlier build is replaced by one from this build.
					if shell.can_manage_daemon()
						&& let Ok(endpoint) = resolve_endpoint()
					{
						let first = daemon_control::restart_idle_at_launch(
							&endpoint, &autostart, &hub, &shared,
						)
						.await;
						if first == daemon_control::LaunchRefresh::KeptBusy {
							// A call was active: replace the daemon as soon as it ends.
							let (hub, autostart, shared) =
								(hub.clone(), autostart.clone(), shared.clone());
							tokio::spawn(async move {
								daemon_control::restart_when_idle(
									&endpoint, &autostart, &hub, &shared,
								)
								.await;
							});
						}
					}
					supervisor::supervise(engine, hub, slot, shell, autostart).await;
				});
			}
			let tray = tray::build(&actions, background.clone(), set_background(&actions));
			host.set_tray(tray.is_some());
			let app = RelayApp::new(actions.clone(), &ctx);
			// Created here, on the main thread, where the OS key must be registered.
			let hotkeys = HotkeyHost::new(
				relay_platform::detect_app_backend(),
				hotkey_state,
				ptt,
				runtime.handle().clone(),
			);
			Ok(Box::new(DesktopApp::new(
				app, actions, tray, background, hotkeys, runtime,
			)))
		}),
	)
}

/// Self-update for this build. Not in mock mode (a demo never touches the network). Cleans up
/// what an earlier update left beside the app, once, here. On Windows the daemon this app talks
/// to is stopped right before the files are replaced, because it holds them.
fn updates(runtime: tokio::runtime::Handle, mock_mode: bool) -> UpdateService {
	if mock_mode {
		return UpdateService::none();
	}
	UpdateService::system(Arc::new(move || {
		if cfg!(windows)
			&& let Ok(endpoint) = resolve_endpoint()
			&& let Err(error) = runtime.block_on(daemon_control::stop(&endpoint))
		{
			tracing::warn!(%error, "the background service did not stop before the update");
		}
	}))
}

/// Accept, Decline and click-to-open from a notification, run on the runtime.
fn notification_replies(actions: &Actions) -> Responder {
	let actions = actions.clone();
	Arc::new(move |call_id: &str, reply: Reply| {
		let (controller, id) = (actions.controller.clone(), call_id.to_owned());
		match reply {
			Reply::Open => actions.show_window(),
			Reply::Accept => {
				actions.show_window();
				actions.runtime.spawn(async move {
					let _ = controller.accept_call(&id).await;
				});
			}
			Reply::Decline => {
				actions.runtime.spawn(async move {
					let _ = controller.decline_call(&id).await;
				});
			}
		}
	})
}

/// The tray's "Stay available in the background" item stores `background.available`.
fn set_background(actions: &Actions) -> Arc<dyn Fn(bool) + Send + Sync> {
	let actions = actions.clone();
	Arc::new(move |on| actions.save_setting(KEY_BACKGROUND, on.to_string()))
}
