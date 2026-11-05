import { emptyState, SeqGuard } from "./resync";
import type { AppState, Bridge, CallMeter, Meters } from "./types";

const NO_METERS: Meters = { seq: 0, microphoneDbfs: null, calls: [] };

/** Reactive UI store over a [`Bridge`]; all engine truth comes from the host. */
export class RelayStore {
	state = $state<AppState>(emptyState(false));
	meters = $state<Meters>(NO_METERS);
	busy = $state(false);
	error = $state<string | null>(null);

	private guard = new SeqGuard();
	private unlisten: Array<() => void> = [];

	constructor(private bridge: Bridge) {}

	async start(): Promise<void> {
		this.unlisten.push(
			await this.bridge.onState((s) => this.receive(s)),
			await this.bridge.onMeters((m) => this.receiveMeters(m)),
		);
		await this.resync();
	}

	stop(): void {
		this.unlisten.forEach((u) => u());
		this.unlisten = [];
	}

	/** Fetch the authoritative state; used at start and after a detected gap. */
	async resync(): Promise<void> {
		try {
			const fresh = await this.bridge.getState();
			if (this.guard.adopt(fresh.seq)) this.apply(fresh);
		} catch (e) {
			this.error = describe(e);
		}
	}

	private receive(next: AppState): void {
		const verdict = this.guard.observe(next.seq);
		if (verdict === "stale") return;
		this.apply(next);
		if (verdict === "gap") void this.resync();
	}

	private apply(next: AppState): void {
		this.state = next;
		if (next.link !== "connected") this.meters = NO_METERS;
	}

	private receiveMeters(next: Meters): void {
		if (next.seq > this.meters.seq && this.state.link === "connected") this.meters = next;
	}

	callMeter(callId: string): CallMeter | undefined {
		return this.meters.calls.find((c) => c.callId === callId);
	}

	/** Run a user action; failures surface as a visible error, never silently. */
	async run(action: () => Promise<void>): Promise<void> {
		this.busy = true;
		this.error = null;
		try {
			await action();
		} catch (e) {
			this.error = describe(e);
		} finally {
			this.busy = false;
		}
	}

	get bridgeApi(): Bridge {
		return this.bridge;
	}
}

export function describe(e: unknown): string {
	if (typeof e === "string") return e;
	if (e instanceof Error) return e.message;
	try {
		return JSON.stringify(e);
	} catch {
		return "Unknown error";
	}
}
