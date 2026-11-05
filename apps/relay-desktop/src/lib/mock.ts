import { emptyState } from "./resync";
import type { AppState, Bridge, Call, CallControls, Meters, TransmitMode } from "./types";

const hex = (n: number) => n.toString(16).padStart(32, "0");

function seedState(): AppState {
	const state = emptyState(true);
	state.link = "connected";
	state.engine = { version: "mock", apiMajor: 1, apiMinor: 0 };
	state.revision = 1;
	state.peers = [
		{ peerId: hex(1), displayName: "Ada Lovelace", addresses: ["192.168.1.20"], trust: "verified", reachable: true, hasAvatar: false },
		{ peerId: hex(2), displayName: "Grace Hopper", addresses: ["192.168.1.31"], trust: "tofuUnverified", reachable: true, hasAvatar: false },
		{ peerId: hex(3), displayName: "Alan Turing", addresses: [], trust: "keyChanged", reachable: false, hasAvatar: false },
	];
	state.devices = [
		{ deviceId: "in-default", name: "Built-in Microphone", direction: "input", available: true, systemDefault: true },
		{ deviceId: "in-usb", name: "USB Headset Mic", direction: "input", available: true, systemDefault: false },
		{ deviceId: "out-default", name: "Built-in Speakers", direction: "output", available: true, systemDefault: true },
		{ deviceId: "out-usb", name: "USB Headset", direction: "output", available: true, systemDefault: false },
	];
	state.settings = {
		inputDeviceId: "in-default",
		outputDeviceId: "out-default",
		globalMuted: false,
		globalDeafened: false,
		microphoneGain: 1,
		playbackGain: 1,
	};
	state.warnings = [];
	state.calls = [mockCall(hex(100), hex(1), "incomingRinging")];
	return state;
}

function mockCall(callId: string, peerId: string, phase: Call["phase"]): Call {
	return {
		callId,
		peerId,
		phase,
		transmit: { mode: "receiveOnly", consentGranted: false, effectiveTransmitting: false, blockedReason: "" },
		userMuted: false,
		userDeafened: false,
		receiveGain: 1,
		profile: "mock",
		problem: null,
		controlledByMe: true,
	};
}

/** In-memory bridge so the UI runs without a daemon (dev and screenshots). */
export function createMockBridge(): Bridge {
	let state = seedState();
	let nextCall = 101;
	const stateHandlers = new Set<(s: AppState) => void>();
	const meterHandlers = new Set<(m: Meters) => void>();
	let meterSeq = 0;

	const publish = (patch: Partial<AppState>) => {
		state = { ...state, ...patch, seq: state.seq + 1 };
		stateHandlers.forEach((h) => h(state));
	};
	const updateCall = (callId: string, fn: (c: Call) => Call) =>
		publish({ calls: state.calls.map((c) => (c.callId === callId ? fn(c) : c)) });

	const meters = (): Meters => {
		const t = Date.now() / 700;
		const live = state.calls.filter((c) => c.phase === "connected");
		return {
			seq: ++meterSeq,
			microphoneDbfs: state.settings?.globalMuted ? null : -30 + 12 * Math.sin(t),
			calls: live.map((c, i) => {
				const level = -28 + 14 * Math.sin(t + i);
				return { callId: c.callId, speaking: level > -26, receiveDbfs: level, rttMs: 4 };
			}),
		};
	};
	setInterval(() => {
		if (meterHandlers.size) {
			const m = meters();
			meterHandlers.forEach((h) => h(m));
		}
	}, 150);

	const done = () => Promise.resolve();
	return {
		getState: async () => state,
		getMeters: async () => meters(),
		onState: async (h) => (stateHandlers.add(h), () => stateHandlers.delete(h)),
		onMeters: async (h) => (meterHandlers.add(h), () => meterHandlers.delete(h)),
		startCall: async (peerId) => {
			publish({ calls: [...state.calls, mockCall(hex(nextCall++), peerId, "outgoingRinging")] });
		},
		acceptCall: async (id) => (updateCall(id, (c) => ({ ...c, phase: "connected" })), done()),
		declineCall: async (id) => (updateCall(id, (c) => ({ ...c, phase: "declined" })), done()),
		endCall: async (id) => (updateCall(id, (c) => ({ ...c, phase: "ended" })), done()),
		setTransmit: async (id, mode: TransmitMode, consent) =>
			updateCall(id, (c) => {
				const sending = consent && mode === "openMic";
				return { ...c, transmit: { mode, consentGranted: consent, effectiveTransmitting: sending, blockedReason: "" } };
			}),
		setCallControls: async (id, controls: CallControls) =>
			updateCall(id, (c) => ({
				...c,
				userMuted: controls.muted ?? c.userMuted,
				userDeafened: controls.deafened ?? c.userDeafened,
				receiveGain: controls.receiveGain ?? c.receiveGain,
			})),
		setGlobalControls: async (controls) => {
			if (state.settings) publish({ settings: { ...state.settings, ...controls } });
		},
		selectDevices: async (inputDeviceId, outputDeviceId) => {
			if (state.settings) publish({ settings: { ...state.settings, inputDeviceId, outputDeviceId } });
		},
		setProfile: async () => publish({ notice: "Mock: profile name is not stored." }),
		uploadAvatar: async () => publish({ notice: "Mock: avatar is only previewed locally." }),
		hideWindow: done,
		quitApp: done,
	};
}
