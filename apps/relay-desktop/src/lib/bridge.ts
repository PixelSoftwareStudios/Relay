import { createMockBridge } from "./mock";
import { createTauriBridge, insideTauri } from "./tauri";
import type { Bridge } from "./types";

/** Tauri when hosted by the shell; mock data when run in a plain browser. */
export function selectBridge(): Bridge {
	const forced = import.meta.env.VITE_RELAY_MOCK === "1";
	return insideTauri() && !forced ? createTauriBridge() : createMockBridge();
}
