import { describe, expect, it } from "vitest";
import { emptyState, SeqGuard } from "../src/lib/resync";

describe("SeqGuard", () => {
	it("accepts the first value and contiguous successors", () => {
		const g = new SeqGuard();
		expect(g.observe(5)).toBe("ok");
		expect(g.observe(6)).toBe("ok");
	});
	it("drops stale and duplicate emissions", () => {
		const g = new SeqGuard();
		g.observe(5);
		expect(g.observe(5)).toBe("stale");
		expect(g.observe(3)).toBe("stale");
	});
	it("reports gaps but still moves forward", () => {
		const g = new SeqGuard();
		g.observe(1);
		expect(g.observe(4)).toBe("gap");
		expect(g.observe(5)).toBe("ok");
	});
	it("adopts only fetched state that is not older", () => {
		const g = new SeqGuard();
		g.observe(10);
		expect(g.adopt(9)).toBe(false);
		expect(g.adopt(10)).toBe(true);
		expect(g.adopt(12)).toBe(true);
		expect(g.observe(13)).toBe("ok");
	});
});

describe("emptyState", () => {
	it("starts connecting with no data", () => {
		const s = emptyState(false);
		expect(s.link).toBe("connecting");
		expect(s.peers).toEqual([]);
	});
});
