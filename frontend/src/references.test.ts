import { describe, expect, it } from "vitest";
import { checkReference, checkReferences, gapText, gradeGap } from "./references";

describe("expected values (VAL-35)", () => {
  // backend/tests/test_references.py holds the same table
  it.each([
    [0, 1, "within"],
    [1, 1, "within"],
    [-1, 1, "within"],
    [1.5, 1, "near"],
    [-2, 1, "near"],
    [2.01, 1, "outside"],
    [0.1, 0, "outside"],
  ])("grades a gap of %s with a tolerance of %s as %s", (diff, tol, grade) => {
    expect(gradeGap(diff, tol)).toBe(grade);
  });

  it("takes the tolerance in % of the reference by default, or in its unit", () => {
    const pct = checkReference({ kpi: "Time to 100 km/h", value: 7.3, tolerance: 5 }, 7.1, "s");
    expect(pct.tolerance).toBeCloseTo(0.365);
    expect(pct.grade).toBe("within");
    const abs = checkReference({ kpi: "Time to 100 km/h", value: 7.3, tolerance: 0.1, tolerancePct: false }, 7.1, "s");
    expect(abs.grade).toBe("near");
    expect(gapText(abs)).toBe("−0.2 s (−2.74 %)");
  });

  it("says when the run has no such value, or rules it out", () => {
    const [missing] = checkReferences([{ kpi: "Nothing", value: 1, tolerance: 5 }], []);
    expect(missing.grade).toBe("missing");
    const [bad] = checkReferences([{ kpi: "Consumption", value: 14, tolerance: 5 }], [
      { label: "Consumption", value: 14, unit: "kWh/100km", notValid: "cycle not followed" },
    ]);
    expect(bad.grade).toBe("not valid");
  });
});
