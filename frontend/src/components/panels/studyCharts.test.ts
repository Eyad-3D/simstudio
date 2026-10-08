import { describe, expect, it } from "vitest";
import type { Study, StudyPoint } from "../../types";
import { MAX_FIGURES, chartData, figuresOf, studyToShow } from "./studyCharts";

const point = (values: number[], kpis: Record<string, number>, extra: Partial<StudyPoint> = {}): StudyPoint => ({
  values,
  status: "success",
  kpis,
  ...extra,
});

function study(id: string, caseId: string, points: StudyPoint[], extra: Partial<Study> = {}): Study {
  return {
    id,
    startedAt: 1,
    caseId,
    caseName: caseId,
    factors: [
      { elementId: "veh", paramKey: "mass_kg", elementLabel: "Vehicle", paramLabel: "Vehicle Mass", unit: "kg", values: points.map((p) => p.values[0]) },
    ],
    kpis: [
      { label: "Distance driven", unit: "km" },
      { label: "Consumption", unit: "kWh/100 km" },
      { label: "Battery — final SOC", unit: "%" },
      { label: "Peak power", unit: "kW" },
    ],
    points,
    ...extra,
  };
}

describe("the Study view (STU-16)", () => {
  const a = study("a", "city", []);
  const b = study("b", "wltc", []);
  const c = study("c", "city", []);

  it("shows the study picked, else the run's, else the case's newest, else the newest", () => {
    const all = [a, b, c]; // oldest first
    expect(studyToShow(all, "b", "a", "city")?.id).toBe("b");
    expect(studyToShow(all, "gone", "a", "city")?.id).toBe("a");
    expect(studyToShow(all, undefined, undefined, "city")?.id).toBe("c");
    expect(studyToShow(all, undefined, undefined, "wltc")?.id).toBe("b");
    expect(studyToShow(all, undefined, undefined, "other")?.id).toBe("c");
    expect(studyToShow([], "a")).toBeUndefined();
  });

  it("opens on the headline figures, then keeps those picked that the study has", () => {
    expect(figuresOf(a)).toEqual(["Consumption", "Distance driven", "Battery — final SOC"]);
    expect(figuresOf(a, ["Peak power", "Gone", "Consumption"])).toEqual(["Peak power", "Consumption"]);
    expect(figuresOf(a, ["Gone"])).toEqual(figuresOf(a)); // none of them: the defaults
    const many = study("m", "city", [], { kpis: Array.from({ length: 20 }, (_, i) => ({ label: `k${i}`, unit: "" })) });
    expect(figuresOf(many, many.kpis.map((k) => k.label))).toHaveLength(MAX_FIGURES);
    expect(figuresOf(many)).toEqual(["k0", "k1", "k2", "k3"]); // no headline number: the first 4
  });

  it("charts complete points against the swept value; values not valid apart, the rest left out", () => {
    const s = study("s", "city", [
      point([1500], { Consumption: 14 }),
      point([1200], { Consumption: 13 }),
      point([1800], { Consumption: 15.5 }, { status: "warning", notValid: { Consumption: "cycle not followed" } }),
      point([2100], { Consumption: 1 }, { status: "cancelled", incomplete: "stopped at t = 3 s" }),
      point([2400], {}, { status: "not run" }),
      point([900], { "Peak power": 50 }), // no Consumption
    ]);
    expect(chartData(s, "Consumption")).toEqual({
      x: [900, 1200, 1500, 1800],
      lines: [{ label: "Consumption", y: [null, 13, 14, null], notValid: [null, null, null, 15.5] }],
      left: 2,
      notValid: 1,
      logX: false,
    });
  });

  it("draws a two-factor study as a line for each value of the second factor", () => {
    const s = study(
      "e",
      "endurance",
      [
        point([6, 30], { "Total time": 1400 }),
        point([6, 40], { "Total time": 1350 }),
        point([7, 30], { "Total time": 1390 }),
        point([7, 40], { "Total time": 1340 }),
      ],
      {
        factors: [
          { elementId: "bat", paramKey: "capacity_kWh", elementLabel: "Accumulator", paramLabel: "Usable Capacity", unit: "kWh", values: [6, 7] },
          { elementId: "bat", paramKey: "output_power_limit_kW", elementLabel: "Accumulator", paramLabel: "Output Power Limit", unit: "kW", values: [30, 40] },
        ],
      },
    );
    const d = chartData(s, "Total time");
    expect(d.x).toEqual([6, 7]);
    expect(d.lines).toEqual([
      { label: "Output Power Limit 30 kW", y: [1400, 1390], notValid: [null, null] },
      { label: "Output Power Limit 40 kW", y: [1350, 1340], notValid: [null, null] },
    ]);
  });

  it("puts log-spaced swept values on a log axis", () => {
    const s = study("l", "city", [point([1], { Consumption: 1 }), point([10], { Consumption: 2 }), point([100], { Consumption: 3 })]);
    expect(chartData(s, "Consumption").logX).toBe(true);
  });
});
