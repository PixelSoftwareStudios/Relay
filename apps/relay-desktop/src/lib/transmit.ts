import type { Call } from "./types";

export type Tone = "idle" | "live" | "warn";

export interface TransmitStatus {
	label: string;
	detail: string;
	tone: Tone;
}

/**
 * Honest transmit text. Only the engine's `effectiveTransmitting` ever claims
 * the microphone is live; a requested mode alone never does.
 */
export function describeTransmit(call: Call): TransmitStatus {
	const { mode, consentGranted, effectiveTransmitting, blockedReason } = call.transmit;
	const blocked = blockedReason ? ` Engine reason: ${blockedReason}.` : "";
	if (mode === "receiveOnly") {
		return { label: "Listening only", detail: "Your microphone is not sent.", tone: "idle" };
	}
	if (!consentGranted) {
		return {
			label: "Microphone not allowed",
			detail: `Transmit consent has not been given for this call.${blocked}`,
			tone: "warn",
		};
	}
	if (effectiveTransmitting) {
		const how = mode === "openMic" ? "open microphone" : "push-to-talk key held";
		return { label: "Transmitting", detail: `Microphone is live (${how}).`, tone: "live" };
	}
	if (mode === "openMic") {
		return {
			label: "Open mic armed, not transmitting",
			detail: `The engine is not sending audio right now.${blocked}`,
			tone: "warn",
		};
	}
	return {
		label: "Push-to-talk armed",
		detail: `Waiting for a key press from an engine-side input source.${blocked}`,
		tone: "idle",
	};
}

/** What this client can and cannot do about push-to-talk. */
export const HOTKEY_NOTE =
	"This app registers no global hotkey and has no on-screen talk button yet. " +
	"Push-to-talk only transmits while an engine-side input source (for example the CLI) holds the key.";
