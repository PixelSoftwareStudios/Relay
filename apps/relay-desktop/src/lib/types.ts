// View model shared with the Rust host (camelCase JSON). Ids are lowercase hex.

export type Link = "connecting" | "connected" | "reconnecting" | "offline";
export type Trust = "unknown" | "tofuUnverified" | "verified" | "keyChanged" | "blocked";
export type CallPhase =
	| "preparing"
	| "outgoingRinging"
	| "incomingRinging"
	| "negotiating"
	| "connected"
	| "interrupted"
	| "ending"
	| "ended"
	| "failed"
	| "declined"
	| "cancelled";
export type TransmitMode = "receiveOnly" | "pushToTalk" | "openMic";

export interface Problem {
	code: string;
	message: string;
	retryable: boolean;
	recoveryAction: string;
}

export interface Peer {
	peerId: string;
	displayName: string;
	addresses: string[];
	trust: Trust;
	reachable: boolean;
	hasAvatar: boolean;
}

export interface Transmit {
	mode: TransmitMode;
	consentGranted: boolean;
	effectiveTransmitting: boolean;
	blockedReason: string;
}

export interface Call {
	callId: string;
	peerId: string;
	phase: CallPhase;
	transmit: Transmit;
	userMuted: boolean;
	userDeafened: boolean;
	receiveGain: number;
	profile: string;
	problem: Problem | null;
	controlledByMe: boolean;
}

export interface Device {
	deviceId: string;
	name: string;
	direction: "input" | "output";
	available: boolean;
	systemDefault: boolean;
}

export interface Settings {
	inputDeviceId: string;
	outputDeviceId: string;
	globalMuted: boolean;
	globalDeafened: boolean;
	microphoneGain: number;
	playbackGain: number;
}

export interface EngineInfo {
	version: string;
	apiMajor: number;
	apiMinor: number;
}

export interface AppState {
	/** Monotonic emit counter; a jump means events were missed. */
	seq: number;
	link: Link;
	/** True when the data is synthetic (no daemon). */
	mock: boolean;
	engine: EngineInfo | null;
	revision: number;
	peers: Peer[];
	calls: Call[];
	devices: Device[];
	settings: Settings | null;
	warnings: Problem[];
	/** Last connection failure, sanitized. */
	linkError: string | null;
	/** Transient result of the last user action. */
	notice: string | null;
}

export interface CallMeter {
	callId: string;
	speaking: boolean;
	receiveDbfs: number | null;
	rttMs: number | null;
}

export interface Meters {
	seq: number;
	microphoneDbfs: number | null;
	calls: CallMeter[];
}

export type CallControls = { muted?: boolean; deafened?: boolean; receiveGain?: number };
export type GlobalControls = {
	muted?: boolean;
	deafened?: boolean;
	microphoneGain?: number;
	playbackGain?: number;
};

/** Everything the UI may ask of the host. Mirrors the Tauri command set. */
export interface Bridge {
	getState(): Promise<AppState>;
	getMeters(): Promise<Meters>;
	onState(handler: (state: AppState) => void): Promise<() => void>;
	onMeters(handler: (meters: Meters) => void): Promise<() => void>;
	startCall(peerId: string): Promise<void>;
	acceptCall(callId: string): Promise<void>;
	declineCall(callId: string): Promise<void>;
	endCall(callId: string): Promise<void>;
	setTransmit(callId: string, mode: TransmitMode, consent: boolean): Promise<void>;
	setCallControls(callId: string, controls: CallControls): Promise<void>;
	setGlobalControls(controls: GlobalControls): Promise<void>;
	selectDevices(inputId: string, outputId: string): Promise<void>;
	setProfile(displayName: string): Promise<void>;
	uploadAvatar(bytes: Uint8Array, mediaType: string): Promise<void>;
	hideWindow(): Promise<void>;
	quitApp(): Promise<void>;
}
