import { describe, expect, it, vi } from "vitest";
import { createMockBridge } from "../src/lib/mock";
import { RelayStore } from "../src/lib/store.svelte";
import type { AppState, Bridge, Meters } from "../src/lib/types";
import { emptyState } from "../src/lib/resync";

function harness(initial: AppState) {
	let onState: (s: AppState) => void = () => {};
	let onMeters: (m: Meters) => void = () => {};
	let current = initial;
	const base = createMockBridge();
	const bridge: Bridge = {
		...base,
		getState: vi.fn(async () => current),
		onState: async (h) => ((onState = h), () => {}),
		onMeters: async (h) => ((onMeters = h), () => {}),
	};
	return {
		bridge,
		push: (s: AppState) => ((current = s), onState(s)),
		emitOnly: (s: AppState) => onState(s),
		meters: (m: Meters) => onMeters(m),
		setCurrent: (s: AppState) => (current = s),
	};
}

const at = (seq: number, patch: Partial<AppState> = {}): AppState => ({ ...emptyState(false), link: "connected", seq, ...patch });

describe("RelayStore", () => {
	it("loads authoritative state on start", async () => {
		const h = harness(at(3, { revision: 9 }));
		const store = new RelayStore(h.bridge);
		await store.start();
		expect(store.state.revision).toBe(9);
		store.stop();
	});

	it("ignores stale emissions", async () => {
		const h = harness(at(5, { revision: 5 }));
		const store = new RelayStore(h.bridge);
		await store.start();
		h.emitOnly(at(4, { revision: 4 }));
		expect(store.state.revision).toBe(5);
	});

	it("refetches after a sequence gap", async () => {
		const h = harness(at(1));
		const store = new RelayStore(h.bridge);
		await store.start();
		h.setCurrent(at(8, { revision: 80 }));
		h.emitOnly(at(7, { revision: 70 }));
		await vi.waitFor(() => expect(store.state.revision).toBe(80));
		expect(h.bridge.getState).toHaveBeenCalledTimes(2);
	});

	it("clears meters when the link drops and ignores them while down", async () => {
		const h = harness(at(1));
		const store = new RelayStore(h.bridge);
		await store.start();
		h.meters({ seq: 1, microphoneDbfs: -20, calls: [] });
		expect(store.meters.microphoneDbfs).toBe(-20);
		h.push(at(2, { link: "reconnecting" }));
		expect(store.meters.microphoneDbfs).toBeNull();
		h.meters({ seq: 2, microphoneDbfs: -10, calls: [] });
		expect(store.meters.microphoneDbfs).toBeNull();
	});

	it("shows action failures instead of swallowing them", async () => {
		const h = harness(at(1));
		const store = new RelayStore(h.bridge);
		await store.run(async () => {
			throw "engine said no";
		});
		expect(store.error).toBe("engine said no");
		expect(store.busy).toBe(false);
	});
});

describe("mock bridge", () => {
	it("runs a call through accept and end without a daemon", async () => {
		const bridge = createMockBridge();
		const first = (await bridge.getState()).calls[0]!;
		await bridge.acceptCall(first.callId);
		expect((await bridge.getState()).calls[0]!.phase).toBe("connected");
		await bridge.endCall(first.callId);
		expect((await bridge.getState()).calls[0]!.phase).toBe("ended");
	});
	it("does not report transmitting without consent", async () => {
		const bridge = createMockBridge();
		const id = (await bridge.getState()).calls[0]!.callId;
		await bridge.setTransmit(id, "openMic", false);
		expect((await bridge.getState()).calls[0]!.transmit.effectiveTransmitting).toBe(false);
	});
});
