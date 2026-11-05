use tauri_build::{AppManifest, Attributes};

// The command list is the webview's whole privilege; see capabilities/.
const COMMANDS: &[&str] = &[
	"get_state",
	"get_meters",
	"start_call",
	"accept_call",
	"decline_call",
	"end_call",
	"set_transmit",
	"set_call_controls",
	"set_global_controls",
	"select_devices",
	"set_profile",
	"upload_avatar",
	"hide_window",
	"quit_app",
];

fn main() {
	let manifest = AppManifest::new().commands(COMMANDS);
	tauri_build::try_build(Attributes::new().app_manifest(manifest)).expect("tauri build script");
}
