import { describe, expect, it } from "vitest";
import libraryJson from "./data/componentLibrary.json";
import { canonicalJson, diffSnapshots, modelFingerprint, nameFromChanges, summaryChange } from "./provenance";
import type { ComponentDef, ElementInstance, Project, RunSnapshot } from "./types";

const project = (overrides: Partial<Project> = {}): Project => ({
  id: "p",
  name: "P",
  systems: [{ id: "s", name: "S", parentId: null, elements: [], connections: [] }],
  dataBusConnections: [],
  cases: [{ id: "c", name: "Case", duration: 10, timeStep: 1 }],
  ...overrides,
});

describe("model fingerprints", () => {
  it("canonical JSON sorts keys at every level and keeps array order", () => {
    expect(canonicalJson({ b: 1, a: { d: [3, { f: 1, e: 2 }], c: null } })).toBe(
      '{"a":{"c":null,"d":[3,{"e":2,"f":1}]},"b":1}',
    );
  });

  it("is the SHA-256 of the canonical JSON, whatever the key order", async () => {
    // sha256 of '{"cases":[{"duration":10,"id":"c",…],…,"systems":[…"parentId":null}]}'
    const expected = "f97eb14893469539d561d4c954b33b8613eb8399ea531f5a004fe0bb9b135bdd";
    const reordered = Object.fromEntries(Object.entries(project()).reverse()) as unknown as Project;
    expect(await modelFingerprint(project())).toBe(expected);
    expect(await modelFingerprint(reordered)).toBe(expected);
  });

  it("changes when the model does", async () => {
    const edited = project({ cases: [{ id: "c", name: "Case", duration: 11, timeStep: 1 }] });
    expect(await modelFingerprint(edited)).not.toBe(await modelFingerprint(project()));
  });
});

// RES-10: what changed between two runs, the run name built from it, and
// how a summary value changed
describe("comparing two runs", () => {
  const lib = Object.fromEntries(
    (libraryJson as unknown as { components: ComponentDef[] }).components.map((c) => [c.id, c]),
  );
  const part = (id: string, componentDefId: string, label: string, parameterOverrides = {}): ElementInstance => ({
    id,
    componentDefId,
    label,
    position: { x: 0, y: 0 },
    parameterOverrides,
  });
  /** A run's snapshot of a small model: the bev-car's Vehicle as it ships,
   *  a battery wired to a bus, a shaft and a constant. */
  const snap = (edit: (s: RunSnapshot) => void = () => {}): RunSnapshot => {
    const s: RunSnapshot = {
      project: project({
        systems: [
          {
            id: "s",
            name: "S",
            parentId: null,
            elements: [
              part("el-vehicle", "vehicle.body", "Vehicle", { mass_kg: 1927, cd: 0.27, frontal_area_m2: 2.31 }),
              part("el-bat", "battery.generic", "Battery"),
              part("el-node", "electric.node", "Bus"),
              part("el-shaft", "mech.shaft", "Shaft"),
              part("el-const", "signal.constant", "Demand"),
            ],
            connections: [
              { id: "w1", sourceElementId: "el-bat", sourcePortId: "pos", targetElementId: "el-node", targetPortId: "t1" },
            ],
          },
        ],
      }),
      case: { id: "c", name: "City Cycle", duration: 600, timeStep: 1, realtimeFactor: 0 },
      appVersion: "0.3.0",
      liveEdits: [],
    };
    edit(s);
    return s;
  };
  const el = (s: RunSnapshot, id: string) => s.project.systems[0].elements.find((e) => e.id === id)!;
  const texts = (a: RunSnapshot, b: RunSnapshot) => diffSnapshots(a, b, lib).map((c) => c.text);

  it("names a parameter change old → new with its unit, and the part", () => {
    const heavier = snap((s) => (el(s, "el-vehicle").parameterOverrides.mass_kg = 2300));
    const changes = diffSnapshots(snap(), heavier, lib);
    expect(changes).toEqual([
      { text: "Vehicle · Vehicle Mass 1,927 → 2,300 kg", short: "Vehicle Mass 2,300 kg", elementId: "el-vehicle" },
    ]);
    expect(nameFromChanges(changes)).toBe("Vehicle Mass 2,300 kg");
    // a library default counts as the value: the shaft's 100 %
    const lossy = snap((s) => (el(s, "el-shaft").parameterOverrides.efficiency_pct = 90));
    expect(texts(snap(), lossy)).toEqual(["Shaft · Mechanical Efficiency 100 → 90 %"]);
    expect(texts(snap(), snap())).toEqual([]);
    expect(nameFromChanges([])).toBeUndefined();
  });

  it("compares the value the solver used: a case override wins, and one equal to the part's is no change", () => {
    const point = snap((s) => (s.case.parameterOverrides = { "el-vehicle": { mass_kg: 1500 } }));
    expect(texts(snap(), point)).toEqual(["Vehicle · Vehicle Mass 1,927 → 1,500 kg"]);
    const same = snap((s) => (s.case.parameterOverrides = { "el-vehicle": { mass_kg: 1927 } }));
    expect(texts(snap(), same)).toEqual([]);
  });

  it("a table is edited or not whatever the order of its keys; its outside-the-data setting counts", () => {
    const ocv = { 0: 300, 10: 318, 20: 330, 40: 342, 60: 352, 80: 362, 90: 368, 100: 376 };
    const reordered = snap(
      (s) => (el(s, "el-bat").parameterOverrides.ocv_table = Object.fromEntries(Object.entries(ocv).reverse())),
    );
    expect(texts(snap(), reordered)).toEqual([]);
    const cell = snap((s) => (el(s, "el-bat").parameterOverrides.ocv_table = { ...ocv, 50: 347 }));
    expect(texts(snap(), cell)).toEqual(["Battery · Open-Circuit Voltage edited"]);
    const outside = snap((s) => (el(s, "el-bat").tableOutside = { ocv_table: ["linear"] }));
    expect(diffSnapshots(snap(), outside, lib)).toEqual([
      { text: "Battery · Open-Circuit Voltage edited", short: "Open-Circuit Voltage edited", elementId: "el-bat" },
    ]);
  });

  it("wiring either way round is the same; wires, links and parts added or removed are listed", () => {
    const swapped = snap((s) => {
      s.project.systems[0].connections = [
        { id: "w9", sourceElementId: "el-node", sourcePortId: "t1", targetElementId: "el-bat", targetPortId: "pos" },
      ];
    });
    expect(texts(snap(), swapped)).toEqual([]);
    const rewired = snap((s) => {
      s.project.systems[0].connections = [];
      s.project.dataBusConnections = [
        { id: "d1", element1Id: "el-const", port1Id: "sig_out", element2Id: "el-shaft", port2Id: "sig_power" },
      ];
      s.project.systems[0].elements.push(part("el-new", "signal.constant", "Extra Demand"));
    });
    expect(diffSnapshots(snap(), rewired, lib)).toEqual([
      { text: "Added Extra Demand", short: "added Extra Demand", elementId: "el-new" },
      { text: "Wired Demand.Output – Shaft.Transmitted Power", short: "wiring changed" },
      { text: "Unwired Battery.Positive Terminal (+) – Bus.Terminal 1", short: "wiring changed" },
    ]);
    const fewer = snap(
      (s) => (s.project.systems[0].elements = s.project.systems[0].elements.filter((e) => e.id !== "el-const")),
    );
    expect(diffSnapshots(snap(), fewer, lib)).toEqual([{ text: "Removed Demand", short: "removed Demand" }]);
  });

  it("case settings: the form's names, absent equals the default, pacing is no change, new fields show", () => {
    const longer = snap((s) => Object.assign(s.case, { duration: 1800, timeStep: 0.1 }));
    expect(texts(snap(), longer)).toEqual(["Case · Duration 600 → 1,800 s", "Case · Step 1 → 0.1 s"]);
    // (as the engine gives a case back: 0.2.0's runs, and cases made before a
    // reload, have none of these fields)
    const same = snap((s) =>
      Object.assign(s.case, { outputEvery: 1, kind: "cycle", realtimeFactor: 5, endDistance: null, startLine: 0 }),
    );
    expect(texts(snap(), same)).toEqual([]);
    // a setting this code does not know yet shows by its field name
    const later = snap((s) => Object.assign(s.case, { stopAtLap: 3 }));
    expect(texts(snap(), later)).toEqual(["Case · stopAtLap — → 3"]);
    // another case comes first
    const wltc = snap((s) => Object.assign(s.case, { id: "w", name: "WLTC Class 3b", duration: 1800 }));
    expect(texts(snap(), wltc)).toEqual(["Case City Cycle → WLTC Class 3b", "Case · Duration 600 → 1,800 s"]);
    // a renamed case does not name the run
    expect(nameFromChanges(diffSnapshots(snap(), snap((s) => (s.case.name = "WLTC")), lib))).toBeUndefined();
  });

  it("lists both runs' live edits last, and leaves them out of the name", () => {
    const edited = snap((s) => {
      el(s, "el-shaft").parameterOverrides.efficiency_pct = 95;
      s.liveEdits = [{ t: 120.4, elementId: "el-vehicle", key: "mass_kg", value: 2400 }];
    });
    const base = snap((s) => (s.liveEdits = [{ t: 30, elementId: "el-shaft", key: "efficiency_pct", value: 98 }]));
    const changes = diffSnapshots(base, edited, lib);
    expect(changes.map((c) => c.text)).toEqual([
      "Shaft · Mechanical Efficiency 100 → 95 %",
      "Live edit in the baseline at t ≈ 30 s: Shaft · Mechanical Efficiency = 98 %",
      "Live edit in this run at t ≈ 120 s: Vehicle · Vehicle Mass = 2,400 kg",
    ]);
    expect(changes[2].elementId).toBe("el-vehicle");
    expect(nameFromChanges(changes)).toBe("Mechanical Efficiency 95 %");
    const three = [{ text: "a", short: "A" }, { text: "b", short: "B" }, { text: "c", short: "C" }];
    expect(nameFromChanges(three)).toBe("A +2 more");
    // cut to the 120 characters a stored run's name holds
    const long = nameFromChanges([{ text: "x", short: `added ${"L".repeat(115)}` }])!;
    expect(long).toHaveLength(120);
    expect(long.endsWith("L…")).toBe(true);
  });

  it("a summary change within the stored rounding is noise; % change is against the baseline", () => {
    expect(summaryChange(7.292, 7.292).noise).toBe(true);
    expect(summaryChange(88.77, 88.76).noise).toBe(true); // one step of 0.01
    expect(summaryChange(88.78, 88.76).noise).toBe(false);
    const c = summaryChange(0.086, 0.07);
    expect([c.noise, c.digits]).toEqual([false, 3]);
    expect(summaryChange(0.5, 0).pct).toBeNull();
    const up = summaryChange(12.34, 11.12);
    expect(up.pct).toBeCloseTo(10.97, 2);
    expect(up.diff).toBeCloseTo(1.22, 9);
    expect(summaryChange(2e-7, 1e-7).noise).toBe(true); // 7 decimals, from the exponent
  });
});
