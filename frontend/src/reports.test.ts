import { describe, expect, it } from "vitest";
import libraryJson from "./data/componentLibrary.json";
import { dutyKpis, energyCsv, limitSegments, limitShares, sankeyLayout, timeAbove } from "./reports";
import { changesSince } from "./staleness";
import type { ComponentDef, EnergyReport, Project, SimRun } from "./types";

const report: EnergyReport = {
  parts: [
    { elementId: "bat", label: "Battery", kind: "battery.generic", inKWh: 0.1, outKWh: 1.0, lostKWh: 0.02, storedKWh: -0.92, lostPct: 2 },
  ],
  sources: [{ label: "Battery", kWh: 1.02, group: "source", elementId: "bat" }],
  sinks: [
    { label: "Air drag", kWh: 0.4, group: "road" },
    { label: "Rolling resistance", kWh: 0.3, group: "road" },
    { label: "E-Motor — losses", kWh: 0.1, group: "losses" },
    { label: "Battery — internal losses", kWh: 0.02, group: "losses" },
    { label: "Wheel FL", kWh: 0.004, group: "losses" },
    { label: "Wheel FR", kWh: 0.004, group: "losses" },
    { label: "Battery — charged back", kWh: 0.1, group: "recovered" },
  ],
  sourceKWh: 1.02,
  remainderKWh: 0.092,
  remainderPct: 9.02,
  balanceErrorPct: 0,
};

describe("the energy Sankey", () => {
  it("draws bands in proportion and the remainder as its own band", () => {
    const { nodes, links, total } = sankeyLayout(report, false, 300);
    expect(total).toBeCloseTo(1.02);
    const into = links.filter((l) => l.to === "in");
    expect(into.reduce((a, l) => a + l.kWh, 0)).toBeCloseTo(1.02);
    const groups = nodes.filter((n) => n.column === 2);
    expect(groups.map((n) => n.label)).toEqual([
      "Driving: air and rolling",
      "Losses in parts",
      "Charged back",
      "Not accounted for",
    ]);
    // the groups and their items carry every kWh the sources gave
    expect(groups.reduce((a, n) => a + n.kWh, 0)).toBeCloseTo(1.02);
    // a band's thickness is its energy at the chart's scale
    const drag = nodes.find((n) => n.label === "Air drag")!;
    expect(drag.h).toBeCloseTo((0.4 / 1.02) * 300);
    // sinks under 1 % of the sources fold into one node
    expect(nodes.some((n) => n.label.startsWith("2 more"))).toBe(true);
  });

  it("puts sinks that exceed the sources on the left", () => {
    const { nodes } = sankeyLayout({ ...report, remainderKWh: -0.05 }, true);
    expect(nodes.find((n) => n.column === 0 && n.label === "Not accounted for")?.kWh).toBeCloseTo(0.05);
  });

  it("exports the table and the bands as CSV", () => {
    const text = energyCsv(report);
    expect(text.split("\n")[0]).toBe("part,kind,in_kWh,out_kWh,lost_kWh,stored_change_kWh,lost_pct_of_sources");
    expect(text).toContain("Not accounted for,remainder,0.092");
  });
});

describe("limits and duty", () => {
  const lane = { label: "E-Motor", elementIds: ["m"], changes: [[0, 1], [1.9, 2], [3.95, 4]] as [number, number][], seconds: { grip: 1.9, set_limit: 2.05, machine: 0.02 } };
  const states = ["braking", "grip", "set_limit", "supply", "machine", "coasting", "demand"];

  it("turns changes into segments that cover the run", () => {
    expect(limitSegments(lane, states, 3.97)).toEqual([
      [0, 1.9, "grip"],
      [1.9, 3.95, "set_limit"],
      [3.95, 3.97, "machine"],
    ]);
    expect(limitShares(lane)[0]).toEqual(["set_limit", 2.05]);
  });

  it("counts the time above a threshold from the stored points", () => {
    const ts = [0, 10, 70, 90, 50].map((value, t) => ({ t, value }));
    expect(timeAbove(ts, 60)).toBe(2);
  });

  it("offers RMS and peak power and current as study KPIs", () => {
    const kpis = dutyKpis({
      caseId: "c",
      status: "success",
      messages: [],
      channels: [],
      summary: [],
      duty: [
        {
          elementId: "m",
          label: "E-Motor",
          kind: "motor.emotor",
          rows: [
            { quantity: "Shaft power", unit: "kW", max: 80, min: -30, mean: 20, rms: 35 },
            { quantity: "Torque", unit: "N·m", max: 230, min: -100, mean: 50, rms: 90 },
            { quantity: "DC current", unit: "A", max: 150, min: -60, mean: 40, rms: 70 },
          ],
        },
      ],
    });
    expect(kpis.map((k) => k.label)).toEqual(["E-Motor — RMS Shaft power", "E-Motor — peak Shaft power", "E-Motor — RMS DC current"]);
    expect(kpis[1].value).toBe(80);
  });
});

describe("changes since the results shown (UX-41)", () => {
  const lib = Object.fromEntries((libraryJson as unknown as { components: ComponentDef[] }).components.map((d) => [d.id, d]));
  const base: Project = {
    id: "p",
    name: "P",
    systems: [
      {
        id: "s",
        name: "S",
        parentId: null,
        elements: [
          { id: "veh", componentDefId: "vehicle.body", label: "Vehicle", position: { x: 0, y: 0 }, parameterOverrides: {} },
          { id: "w", componentDefId: "propulsion.wheel", label: "Wheel", position: { x: 0, y: 0 }, parameterOverrides: {} },
        ],
        connections: [],
      },
    ],
    dataBusConnections: [],
    cases: [{ id: "c", name: "Case", duration: 10, timeStep: 1 }],
  };
  const run = { id: "r", caseId: "c", caseName: "Case", startedAt: 0, status: "success", result: {} as SimRun["result"], snapshot: { project: base, case: base.cases[0], appVersion: "0.3.0", liveEdits: [] } } as SimRun;

  it("finds nothing for the model the run was made from", () => {
    expect(changesSince(run, base, lib).changes).toEqual([]);
  });

  it("marks the parts, wires and cases changed since", () => {
    const now: Project = structuredClone(base);
    now.systems[0].elements[0].parameterOverrides = { mass_kg: 2300 };
    now.systems[0].connections.push({ id: "c1", sourceElementId: "veh", sourcePortId: "a", targetElementId: "w", targetPortId: "b" });
    now.cases[0].duration = 20;
    now.cases[0].name = "Renamed"; // a name changes no result
    const s = changesSince(run, now, lib);
    expect([...s.elementIds]).toEqual(["veh"]);
    expect([...s.wireIds]).toEqual(["c1"]);
    expect([...s.caseIds]).toEqual(["c"]);
    expect(s.changes.map((c) => c.text)).toContain("Vehicle · Vehicle Mass 1,800 → 2,300 kg");
  });
});
