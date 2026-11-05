import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AppState, Bridge, Meters } from "./types";

export const STATE_EVENT = "relay://state";
export const METERS_EVENT = "relay://meters";

/** Real bridge: thin wrappers over the restricted Tauri command set. */
export function createTauriBridge(): Bridge {
	return {
		getState: () => invoke<AppState>("get_state"),
		getMeters: () => invoke<Meters>("get_meters"),
		onState: (handler) => listen<AppState>(STATE_EVENT, (e) => handler(e.payload)),
		onMeters: (handler) => listen<Meters>(METERS_EVENT, (e) => handler(e.payload)),
		startCall: (peerId) => invoke("start_call", { peerId }),
		acceptCall: (callId) => invoke("accept_call", { callId }),
		declineCall: (callId) => invoke("decline_call", { callId }),
		endCall: (callId) => invoke("end_call", { callId }),
		setTransmit: (callId, mode, consent) => invoke("set_transmit", { callId, mode, consent }),
		setCallControls: (callId, controls) => invoke("set_call_controls", { callId, ...controls }),
		setGlobalControls: (controls) => invoke("set_global_controls", { ...controls }),
		selectDevices: (inputId, outputId) => invoke("select_devices", { inputId, outputId }),
		setProfile: (displayName) => invoke("set_profile", { displayName }),
		uploadAvatar: (bytes, mediaType) =>
			invoke("upload_avatar", bytes, { headers: { "x-media-type": mediaType } }),
		hideWindow: () => invoke("hide_window"),
		quitApp: () => invoke("quit_app"),
	};
}

export function insideTauri(): boolean {
	return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
