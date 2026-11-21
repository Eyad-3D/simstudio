import { describe, expect, it } from "vitest";
import type { SimRun, StoredRunInfo, SummaryValue } from "../../types";
import { comparedPair, differenceOf } from "./chartUtils";
import { figureColumns, ranText, runRows, sortRows, type RunRow } from "./runRows";

describe("the difference between two runs", () => {
  it("on one time grid: the second less the first, sample by sample, nothing interpolated", () => {
    const t = [0, 0.1, 0.2, 0.30000000000000004];
    expect(differenceOf(t, [1, 2, 3, 4], t, [1.5, 2, 2.5, 7])).toEqual([0.5, 0, -0.5, 3]);
  });

  it("on another grid: the second read linearly at the first's times", () => {
    // the second sampled every 2 s, the first every 1 s
    expect(differenceOf([0, 1, 2, 3, 4], [0, 0, 0, 0, 0], [0, 2, 4], [10, 20, 40])).toEqual([10, 15, 20, 30, 40]);
    // and the other way: a finer second run is read at the first's samples
    expect(differenceOf([0, 2], [1, 1], [0, 0.5, 1, 1.5, 2], [1, 9, 9, 9, 3])).toEqual([0, 2]);
  });

  it("only where both runs have data: runs of different length leave gaps", () => {
    // the second covers 1 s to 3 s only
    expect(differenceOf([0, 1, 2, 3, 4], [1, 1, 1, 1, 1], [1, 3], [2, 4])).toEqual([null, 1, 2, 3, null]);
    // a shorter first run: the difference ends with it
    expect(differenceOf([0, 1], [5, 5], [0, 1, 2, 3], [6, 7, 8, 9])).toEqual([1, 2]);
    // a gap in either run is a gap in the difference
    expect(differenceOf([0, 1, 2], [1, null, 1], [0, 2], [1, null])).toEqual([0, null, null]);
    expect(differenceOf([0, 1], [1, 1], [], [])).toEqual([null, null]);
  });

  it("standing still on a distance axis: the second's last sample at that distance", () => {
    expect(differenceOf([0, 5, 5, 10], [0, 1, 2, 3], [0, 5, 5, 10], [0, 4, 6, 3])).toEqual([0, 5, 4, 0]);
  });

  it("is drawn for exactly two runs, on the time chart only", () => {
    expect(comparedPair(["a"], "chart")).toBeNull();
    expect(comparedPair(["a", "b"], "chart")).toEqual(["a", "b"]);
    expect(comparedPair(["a", "b", "c"], "chart")).toBeNull();
    expect(comparedPair(["a", "b"], "xy")).toBeNull();
    expect(comparedPair(["a", "b"], "table")).toBeNull();
  });
});

const sv = (key: string, label: string, value: number, unit: string): SummaryValue => ({ key, label, value, unit });
const figures = (consumption: number, distance: number, soc?: number) => [
  sv("simulated_duration_s", "Simulated duration", 600, "s"),
  sv("distance_km", "Distance driven", distance, "km"),
  sv("consumption_kwh_per_100km", "Consumption", consumption, "kWh/100km"),
  ...(soc === undefined ? [] : [sv("el-bat.final_soc_pct", "HV Battery — final SOC", soc, "%")]),
];
function run(id: string, startedAt: number, summary: SummaryValue[], extra: Partial<SimRun> = {}): SimRun {
  return {
    id,
    caseId: "c",
    caseName: "City Cycle",
    startedAt,
    status: "success",
    result: { caseId: "c", status: "success", messages: [], channels: [], summary },
    ...extra,
  };
}
const listed = (r: SimRun): StoredRunInfo => {
  const { result, snapshot: _snapshot, ...rest } = r;
  return { ...rest, status: result.status, summary: result.summary, bytes: 1 };
};

describe("the table of runs", () => {
  const a = run("a", 3_000, figures(14.2, 7.3, 80), { name: "Vehicle Mass 2,300 kg" });
  const b = run("b", 2_000, figures(13.1, 7.3, 82));
  const c = run("c", 1_000, figures(15.0, 7.2));

  it("lists the runs in memory and the stored runs that are not, newest first", () => {
    const rows = runRows([b, a], [listed(a), listed(b), listed(c)]);
    expect(rows.map((r) => [r.id, r.loaded])).toEqual([
      ["a", true],
      ["b", true],
      ["c", false],
    ]);
    expect(rows[2].summary).toEqual(c.result.summary);
  });

  it("has a column for each headline figure the runs have, at most four, in the headline order", () => {
    const cols = figureColumns(runRows([a, b, c], []));
    expect(cols.map((col) => col.label)).toEqual(["Consumption", "Distance driven", "HV Battery — final SOC"]);
    expect(figureColumns(runRows([a], []), 2).map((col) => col.key)).toEqual(["consumption_kwh_per_100km", "distance_km"]);
  });

  it("sorts by a column either way; a run without the figure stays last", () => {
    const rows = runRows([a, b, c], []);
    const cols = figureColumns(rows);
    const ids = (key: Parameters<typeof sortRows>[1]["key"], desc: boolean) =>
      sortRows(rows, { key, desc }, cols).map((r) => r.id);
    expect(ids("ran", true)).toEqual(["a", "b", "c"]);
    expect(ids("ran", false)).toEqual(["c", "b", "a"]);
    expect(ids(0, false)).toEqual(["b", "a", "c"]); // consumption, low to high
    expect(ids(0, true)).toEqual(["c", "a", "b"]);
    expect(ids(2, false)).toEqual(["a", "b", "c"]); // final SOC: c has none
    expect(ids(2, true)).toEqual(["b", "a", "c"]);
    expect(ids("name", false)).toEqual(["a", "b", "c"]); // unnamed runs last, newest first
  });

  it("sorts names as a person reads them", () => {
    const sweep = (id: string, v: number) =>
      runRows([run(id, v, [], { sweepParam: "Vehicle Mass", sweepValue: v, sweepUnit: "kg" })], [])[0];
    const rows: RunRow[] = [sweep("x", 900), sweep("y", 1800), sweep("z", 1200)];
    expect(sortRows(rows, { key: "name", desc: false }, []).map((r) => r.id)).toEqual(["x", "z", "y"]);
  });

  it("says when a run started: its time today, the day before that", () => {
    const now = new Date(2026, 10, 21, 15, 0).getTime();
    expect(ranText(new Date(2026, 10, 21, 9, 12, 5).getTime(), now)).not.toMatch(/Nov/);
    expect(ranText(new Date(2026, 10, 20, 9, 12, 5).getTime(), now)).toMatch(/20/);
  });
});
