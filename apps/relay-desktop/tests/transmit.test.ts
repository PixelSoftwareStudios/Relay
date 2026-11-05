import { describe, expect, it } from "vitest";
import { describeTransmit } from "../src/lib/transmit";
import type { Call, Transmit } from "../src/lib/types";

function call(transmit: Partial<Transmit>): Call {
	return {
		callId: "c",
		peerId: "p",
		phase: "connected",
		transmit: { mode: "receiveOnly", consentGranted: false, effectiveTransmitting: false, blockedReason: "", ...transmit },
		userMuted: false,
		userDeafened: false,
		receiveGain: 1,
		profile: "",
		problem: null,
		controlledByMe: true,
	};
}

describe("describeTransmit", () => {
	it("never claims a live microphone for listen-only", () => {
		expect(describeTransmit(call({ effectiveTransmitting: true })).tone).toBe("idle");
	});
	it("requires consent before talking about transmitting", () => {
		const s = describeTransmit(call({ mode: "openMic", effectiveTransmitting: true }));
		expect(s.tone).toBe("warn");
		expect(s.label).toMatch(/not allowed/);
	});
	it("reports transmitting only when the engine says so", () => {
		const live = describeTransmit(call({ mode: "pushToTalk", consentGranted: true, effectiveTransmitting: true }));
		expect(live.tone).toBe("live");
		const armed = describeTransmit(call({ mode: "pushToTalk", consentGranted: true }));
		expect(armed.tone).toBe("idle");
		expect(armed.label).toMatch(/armed/);
	});
	it("surfaces the engine's blocked reason for open mic", () => {
		const s = describeTransmit(call({ mode: "openMic", consentGranted: true, blockedReason: "device lost" }));
		expect(s.tone).toBe("warn");
		expect(s.detail).toContain("device lost");
	});
});
