import { describe, expect, it } from "vitest";
import {
  MAX_SWEEP_POINTS,
  linearValues,
  logValues,
  looksLogSpaced,
  parseValueList,
  progressText,
  sweepValues,
  timeLeft,
  timeLeftText,
} from "./sweep";

describe("sweep values (STU-17)", () => {
  it("even steps include both ends, without float noise", () => {
    expect(linearValues(963.5, 2890.5, 3)).toEqual([963.5, 1927, 2890.5]);
    expect(linearValues(0.1, 0.3, 3)).toEqual([0.1, 0.2, 0.3]);
    expect(linearValues(1e-7, 3e-7, 3)).toEqual([1e-7, 2e-7, 3e-7]); // small values are not rounded away
    expect(linearValues(80, 40, 5)).toEqual([80, 70, 60, 50, 40]); // downwards too
    expect(linearValues(5, 9, 1)).toEqual([5]);
  });

  it("are capped at 200 points", () => {
    expect(linearValues(0, 1, 500)).toHaveLength(MAX_SWEEP_POINTS);
    expect(logValues(1, 1000, 500).values).toHaveLength(MAX_SWEEP_POINTS);
  });

  it("log steps grow by the same factor each step", () => {
    expect(logValues(10, 1000, 3).values).toEqual([10, 100, 1000]);
    expect(logValues(1, 100, 5).values).toEqual([1, 3.16228, 10, 31.6228, 100]);
    expect(logValues(1000, 10, 3).values).toEqual([1000, 100, 10]);
    expect(logValues(0, 100, 3)).toEqual({ values: [], problem: "Log steps need both ends above 0." });
    expect(logValues(-1, 100, 3).problem).not.toBeNull();
  });

  it("a typed list runs in the order typed, a repeated value once", () => {
    expect(parseValueList("1200, 1350;1500\n900  1.5e3")).toEqual({
      values: [1200, 1350, 1500, 900],
      problem: null,
      note: "1 repeated value left out.",
    });
    expect(parseValueList("-5, 0, 2.5").values).toEqual([-5, 0, 2.5]);
    expect(parseValueList("  ").problem).toMatch(/^Type the values/);
    expect(parseValueList("1, 2,5a").problem).toBe("'5a' is not a number: use a point for decimals."); // a decimal comma splits
    expect(parseValueList("1, abc").problem).toBe("'abc' is not a number: use a point for decimals.");
    const many = Array.from({ length: 201 }, (_, i) => i).join(",");
    expect(parseValueList(many)).toEqual({ values: [], problem: "At most 200 values: the list has 201." });
  });

  it("the form gives the values of the spacing picked", () => {
    const range = { from: 10, to: 1000, steps: 3 };
    expect(sweepValues("linear", range, "").values).toEqual([10, 505, 1000]);
    expect(sweepValues("log", range, "").values).toEqual([10, 100, 1000]);
    expect(sweepValues("list", range, "7 8").values).toEqual([7, 8]);
    // the same value at both ends runs once
    expect(sweepValues("linear", { from: 5, to: 5, steps: 4 }, "")).toEqual({
      values: [5],
      problem: null,
      note: "3 repeated values left out.",
    });
  });

  it("log-spaced values over a decade or more are told apart from even ones", () => {
    expect(looksLogSpaced([1, 3.16228, 10, 31.6228, 100])).toBe(true);
    expect(looksLogSpaced([100, 10, 1000])).toBe(true); // in any order
    expect(looksLogSpaced([10, 505, 1000])).toBe(false);
    expect(looksLogSpaced([1000, 1189.21, 1414.21, 1681.79, 2000])).toBe(false); // under a decade
    expect(looksLogSpaced([1, 10])).toBe(false);
    expect(looksLogSpaced([0, 1, 10, 100])).toBe(false);
  });
});

describe("a running sweep's time left (STU-17)", () => {
  const start = 1_000_000;
  it("follows the pace of the points that ended", () => {
    const p = { total: 10, done: 0, startedAt: start, lastAt: null };
    expect(timeLeft(p, start + 5000)).toBeNull();
    // 3 points in 30 s: 10 s a point, so the 10 end at 100 s
    const after3 = { ...p, done: 3, lastAt: start + 30_000 };
    expect(timeLeft(after3, start + 30_000)).toBe(70);
    expect(timeLeft(after3, start + 40_000)).toBe(60); // it counts down between points
    expect(timeLeft(after3, start + 200_000)).toBe(0);
    expect(timeLeft({ ...p, done: 10, lastAt: start + 90_000 }, start + 90_000)).toBe(0);
  });

  it("is said in round numbers", () => {
    expect(timeLeftText(null)).toBe("working out the time left");
    expect(timeLeftText(3)).toBe("almost done");
    expect(timeLeftText(41)).toBe("about 40 s left");
    expect(timeLeftText(57)).toBe("about 55 s left");
    expect(timeLeftText(58)).toBe("about 1 min left");
    expect(timeLeftText(185)).toBe("about 3 min left");
    expect(timeLeftText(3569)).toBe("about 59 min left");
    expect(timeLeftText(3600)).toBe("about 1 h left");
    expect(timeLeftText(4800)).toBe("about 1 h 20 min left");
    expect(progressText({ total: 10, done: 3, startedAt: start, lastAt: start + 30_000 }, start + 30_000)).toBe(
      "3 of 10 points done · about 1 min left",
    );
  });
});
