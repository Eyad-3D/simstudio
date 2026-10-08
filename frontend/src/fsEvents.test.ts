import { describe, expect, it } from "vitest";
import { fsPointsTable, packKwh, packOverrides, tableMean } from "./fsEvents";
import type { Project, SimRun, SummaryValue } from "./types";

const project: Project = {
  id: "p",
  name: "P",
  systems: [{ id: "s", name: "S", parentId: null, elements: [], connections: [] }],
  dataBusConnections: [],
  cases: [
    {
      id: "a",
      name: "Acc",
      duration: 25,
      timeStep: 0.01,
      kind: "acceleration",
      fsEvent: "acceleration",
    },
    {
      id: "e",
      name: "End",
      duration: 600,
      timeStep: 1,
      kind: "lap",
      fsEvent: "endurance",
    },
  ],
};

const run = (id: string, caseId: string, summary: SummaryValue[], startedAt = 1): SimRun => ({
  id,
  caseId,
  caseName: caseId,
  startedAt,
  status: "success",
  result: { caseId, status: "success", messages: [], channels: [], summary },
});

describe("Formula Student points table", () => {
  it("takes each event's newest run and adds up the points", () => {
    const runs = [
      run("r3", "e", [
        {
          label: "Endurance time (FS Rules 2026 v1.1 (FSG))",
          value: 1305,
          unit: "s",
        },
        { label: "Endurance points (estimate)", value: 200.5, unit: "points" },
        { label: "Efficiency points (estimate)", value: 50, unit: "points" },
      ]),
      run("r2", "a", [
        {
          label: "Acceleration time (FS Rules 2026 v1.1 (FSG))",
          value: 3.8,
          unit: "s",
        },
        { label: "Acceleration points (estimate)", value: 40, unit: "points" },
      ]),
      run("r1", "a", [{ label: "Acceleration points (estimate)", value: 10, unit: "points" }]),
    ];
    const { rows, total } = fsPointsTable(project, runs);
    expect(rows.map((r) => r.event)).toEqual(["acceleration", "skidpad", "autocross", "endurance", "efficiency"]);
    expect(rows[0]).toMatchObject({ time: 3.8, points: 40, maxPoints: 50 });
    expect(rows[1].note).toMatch(/No case/);
    expect(rows[4]).toMatchObject({ points: 50, maxPoints: 75 });
    expect(total).toBeCloseTo(290.5);
  });

  it("names a broken rule and a missing reference", () => {
    const runs = [
      run("r1", "a", [
        {
          label: "Acceleration time (FS Rules 2026 v1.1 (FSG))",
          value: 3.8,
          unit: "s",
        },
        {
          label: "Rule check: power, 500 ms average (EV 2.2.1)",
          value: 92,
          unit: "kW",
          limit: 80,
          passed: false,
        },
        { label: "Acceleration points (estimate)", value: 0, unit: "points" },
      ]),
      run("r2", "e", [
        {
          label: "Endurance time (FS Rules 2026 v1.1 (FSG))",
          value: 1305,
          unit: "s",
        },
      ]),
    ];
    const { rows } = fsPointsTable(project, runs);
    expect(rows[0].breach).toBe("power, 500 ms average (EV 2.2.1): 92 kW over 80 kW");
    expect(rows[3].note).toMatch(/Reference time/);
    expect(rows[4].note).toMatch(/Reference energy/);
  });
});

describe("the endurance study's pack axis (STU-38)", () => {
  it("scales the charge capacity with the kWh when one is set", () => {
    // the FS example: 14 Ah and 7.21 kWh; the engine reads the charge from the Ah
    const fs = { pack_model: "Pack values", capacity_kWh: 7.21, capacity_Ah: 14 };
    expect(packKwh(fs)).toBe(7.21);
    const small = packOverrides(fs, 3.0);
    expect(small.capacity_kWh).toBe(3.0);
    expect(small.capacity_Ah).toBeCloseTo((14 * 3.0) / 7.21, 12);
    expect(packOverrides(fs, 7.21).capacity_Ah).toBeCloseTo(14, 12);
    // no Charge Capacity: the amp-hours already follow the kWh
    expect(packOverrides({ capacity_kWh: 60, capacity_Ah: 0 }, 45)).toEqual({ capacity_kWh: 45 });
  });

  it("scales the cell's charge for a pack built from cells", () => {
    const cells = {
      pack_model: "Cells",
      series_cells: 100,
      parallel_cells: 2,
      cell_capacity_Ah: 5,
      cell_ocv_table: { "0": 3.0, "100": 4.0 },
    };
    expect(tableMean(cells.cell_ocv_table)).toBeCloseTo(3.5, 12);
    expect(packKwh(cells)).toBeCloseTo(3.5, 12); // 100 × 2 × 5 Ah × 3.5 V
    expect(packOverrides(cells, 7)).toEqual({ cell_capacity_Ah: 10 });
  });

  it("reads a table's mean with its ends held", () => {
    expect(tableMean({ "20": 2, "80": 4 })).toBeCloseTo((20 * 2 + 60 * 3 + 20 * 4) / 100, 12);
    expect(Number.isNaN(tableMean("x"))).toBe(true);
  });
});
