import { describe, expect, it } from "vitest";
import { avatarProblem, hueFromId, initials, isTerminal, levelFraction, MAX_AVATAR_BYTES } from "../src/lib/format";

describe("levelFraction", () => {
	it("maps dBFS onto 0..1 and clamps", () => {
		expect(levelFraction(0)).toBe(1);
		expect(levelFraction(-30)).toBeCloseTo(0.5);
		expect(levelFraction(-90)).toBe(0);
		expect(levelFraction(6)).toBe(1);
	});
	it("renders unknown readings as empty", () => {
		expect(levelFraction(null)).toBe(0);
		expect(levelFraction(Number.NaN)).toBe(0);
	});
});

describe("initials and hue", () => {
	it("takes up to two leading letters", () => {
		expect(initials("ada lovelace byron")).toBe("AL");
		expect(initials("  ")).toBe("?");
		expect(initials("Zoë")).toBe("Z");
	});
	it("is stable and bounded", () => {
		expect(hueFromId("abc")).toBe(hueFromId("abc"));
		expect(hueFromId("x".repeat(500))).toBeLessThan(360);
	});
});

describe("avatarProblem", () => {
	it("accepts small png and jpeg", () => {
		expect(avatarProblem(1000, "image/png")).toBeNull();
		expect(avatarProblem(MAX_AVATAR_BYTES, "image/jpeg")).toBeNull();
	});
	it("rejects other types, empty and oversized files", () => {
		expect(avatarProblem(10, "image/gif")).not.toBeNull();
		expect(avatarProblem(0, "image/png")).not.toBeNull();
		expect(avatarProblem(MAX_AVATAR_BYTES + 1, "image/png")).not.toBeNull();
	});
});

describe("isTerminal", () => {
	it("flags finished phases only", () => {
		expect(isTerminal("declined")).toBe(true);
		expect(isTerminal("connected")).toBe(false);
	});
});
