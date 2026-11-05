import type { CallPhase, Link } from "./types";

const FLOOR_DBFS = -60;

/** Map a dBFS reading to a 0..1 bar fill; unknown readings render empty. */
export function levelFraction(dbfs: number | null): number {
	if (dbfs === null || Number.isNaN(dbfs)) return 0;
	return Math.min(1, Math.max(0, (dbfs - FLOOR_DBFS) / -FLOOR_DBFS));
}

export function formatDbfs(dbfs: number | null): string {
	return dbfs === null || Number.isNaN(dbfs) ? "no signal data" : `${dbfs.toFixed(0)} dBFS`;
}

const PHASES: Record<CallPhase, string> = {
	preparing: "Preparing",
	outgoingRinging: "Ringing",
	incomingRinging: "Incoming call",
	negotiating: "Connecting",
	connected: "Connected",
	interrupted: "Interrupted",
	ending: "Ending",
	ended: "Ended",
	failed: "Failed",
	declined: "Declined",
	cancelled: "Cancelled",
};

export function phaseLabel(phase: CallPhase): string {
	return PHASES[phase];
}

const LINKS: Record<Link, string> = {
	connecting: "Connecting to the Relay daemon",
	connected: "Connected",
	reconnecting: "Daemon connection lost, reconnecting",
	offline: "Daemon not running",
};

export function linkLabel(link: Link): string {
	return LINKS[link];
}

const TERMINAL: CallPhase[] = ["ended", "failed", "declined", "cancelled"];

export function isTerminal(phase: CallPhase): boolean {
	return TERMINAL.includes(phase);
}

export function initials(name: string): string {
	const parts = name.trim().split(/\s+/).filter(Boolean);
	const letters = parts.slice(0, 2).map((p) => Array.from(p)[0] ?? "");
	return letters.join("").toUpperCase() || "?";
}

/** Stable hue (0..359) derived from an id so each peer keeps its colour. */
export function hueFromId(id: string): number {
	let h = 0;
	for (const ch of id) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
	return h % 360;
}

export function gainPercent(gain: number): string {
	return `${Math.round(gain * 100)}%`;
}

export const MAX_AVATAR_BYTES = 256 * 1024;

const AVATAR_TYPES = ["image/png", "image/jpeg"];

/** Client-side pre-check only; the engine decodes and validates for real. */
export function avatarProblem(size: number, mediaType: string): string | null {
	if (!AVATAR_TYPES.includes(mediaType)) return "Use a PNG or JPEG image.";
	if (size === 0) return "The file is empty.";
	if (size > MAX_AVATAR_BYTES) return "The image must be 256 KiB or smaller.";
	return null;
}
