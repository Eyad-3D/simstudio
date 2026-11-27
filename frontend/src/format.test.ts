import { describe, expect, it } from "vitest";
import { fixedNumber, formatNumber, signedNumber } from "./format";
import { fmtNum, fmtPct } from "./components/panels/chartUtils";

describe("a rounded zero has no sign (app pass #28)", () => {
  it("formatNumber", () => {
    expect(formatNumber(-0.0004, "en", { maximumFractionDigits: 3 })).toBe("0");
    expect(formatNumber(-0, "en")).toBe("0");
    expect(formatNumber(-0.25, "en", { maximumFractionDigits: 3 })).toBe("-0.25");
    expect(formatNumber(1234.5, "en", { maximumFractionDigits: 1 })).toBe("1,234.5");
  });
  it("fixedNumber, as a part's badge reads a car at rest", () => {
    expect(fixedNumber(-0.04, 1, "en")).toBe("0.0");
    expect(fixedNumber(-0.06, 1, "en")).toBe("-0.1");
  });
  it("signedNumber, as the baseline line reads a change", () => {
    expect(signedNumber(-0.0001, 3, "en")).toBe("0.000");
    expect(signedNumber(-0.004, 2, "en")).toBe("0.00");
    expect(signedNumber(0.5, 3, "en")).toBe("+0.500");
    expect(signedNumber(-0.5, 1, "en")).toBe("-0.5");
  });
  it("the chart read-outs and gaps", () => {
    expect(fmtNum(-1e-10)).toBe("0");
    expect(fmtNum(-0)).toBe("0");
    expect(fmtNum(-0.00004)).toBe("-0.00004");
    expect(fmtPct(-0.0004)).toBe("> −0.001");
  });
});
