//! Tauri commands: the complete surface the webview may call. Each is a thin
//! wrapper; validation and engine mapping live in [`crate::controller`].

use std::sync::Arc;

use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, State, WebviewWindow};

use crate::controller::{ControlError, Controller};
use crate::hub::Hub;
use crate::model::{AppState, Meters, Mode};

type Ctl<'a> = State<'a, Arc<Controller>>;

#[tauri::command]
pub fn get_state(hub: State<'_, Arc<Hub>>) -> AppState {
	hub.state()
}

#[tauri::command]
pub fn get_meters(hub: State<'_, Arc<Hub>>) -> Meters {
	hub.meters()
}

#[tauri::command]
pub async fn start_call(ctl: Ctl<'_>, peer_id: String) -> Result<(), ControlError> {
	ctl.start_call(&peer_id).await
}

#[tauri::command]
pub async fn accept_call(ctl: Ctl<'_>, call_id: String) -> Result<(), ControlError> {
	ctl.accept_call(&call_id).await
}

#[tauri::command]
pub async fn decline_call(ctl: Ctl<'_>, call_id: String) -> Result<(), ControlError> {
	ctl.decline_call(&call_id).await
}

#[tauri::command]
pub async fn end_call(ctl: Ctl<'_>, call_id: String) -> Result<(), ControlError> {
	ctl.end_call(&call_id).await
}

#[tauri::command]
pub async fn set_transmit(
	ctl: Ctl<'_>,
	call_id: String,
	mode: Mode,
	consent: bool,
) -> Result<(), ControlError> {
	ctl.set_transmit(&call_id, mode, consent).await
}

#[tauri::command]
pub async fn set_call_controls(
	ctl: Ctl<'_>,
	call_id: String,
	muted: Option<bool>,
	deafened: Option<bool>,
	receive_gain: Option<f32>,
) -> Result<(), ControlError> {
	ctl.set_call_controls(&call_id, muted, deafened, receive_gain)
		.await
}

#[tauri::command]
pub async fn set_global_controls(
	ctl: Ctl<'_>,
	muted: Option<bool>,
	deafened: Option<bool>,
	microphone_gain: Option<f32>,
	playback_gain: Option<f32>,
) -> Result<(), ControlError> {
	ctl.set_global_controls(muted, deafened, microphone_gain, playback_gain)
		.await
}

#[tauri::command]
pub async fn select_devices(
	ctl: Ctl<'_>,
	input_id: String,
	output_id: String,
) -> Result<(), ControlError> {
	ctl.select_devices(&input_id, &output_id).await
}

#[tauri::command]
pub async fn set_profile(ctl: Ctl<'_>, display_name: String) -> Result<(), ControlError> {
	ctl.set_profile(&display_name).await
}

/// Raw body is the encoded image; the media type travels in a header.
#[tauri::command]
pub async fn upload_avatar(ctl: Ctl<'_>, request: Request<'_>) -> Result<(), ControlError> {
	let InvokeBody::Raw(bytes) = request.body() else {
		return Err(ControlError::Invalid("avatar must be sent as raw bytes"));
	};
	let media_type = request
		.headers()
		.get("x-media-type")
		.and_then(|v| v.to_str().ok())
		.unwrap_or_default();
	ctl.upload_avatar(bytes.clone(), media_type).await
}

/// Hides the window; the daemon and any call are untouched.
#[tauri::command]
pub fn hide_window(window: WebviewWindow) -> Result<(), ControlError> {
	window
		.hide()
		.map_err(|e| ControlError::Engine(e.to_string()))
}

/// Quits this app only. The daemon is a separate process and keeps running.
#[tauri::command]
pub fn quit_app(app: AppHandle) {
	app.exit(0);
}
