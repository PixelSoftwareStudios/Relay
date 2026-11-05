import type { AppState } from "./types";

export type SeqVerdict = "ok" | "stale" | "gap";

/**
 * Orders host emissions. State events carry the full structural state, so a
 * gap is repaired by one `getState` call rather than by replaying events.
 */
export class SeqGuard {
	private last: number | null = null;

	observe(seq: number): SeqVerdict {
		if (this.last !== null && seq <= this.last) return "stale";
		const verdict = this.last !== null && seq !== this.last + 1 ? "gap" : "ok";
		this.last = seq;
		return verdict;
	}

	/** Accept a state fetched out of band if it is newer than what we hold. */
	adopt(seq: number): boolean {
		if (this.last !== null && seq < this.last) return false;
		this.last = seq;
		return true;
	}
}

/** Placeholder shown before the first host state arrives. */
export function emptyState(mock: boolean): AppState {
	return {
		seq: 0,
		link: "connecting",
		mock,
		engine: null,
		revision: 0,
		peers: [],
		calls: [],
		devices: [],
		settings: null,
		warnings: [],
		linkError: null,
		notice: null,
	};
}
