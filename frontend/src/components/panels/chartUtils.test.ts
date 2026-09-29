import { describe, expect, it, vi } from "vitest";
import type { Channel, SimRun } from "../../types";
import {
  defaultChannelKeys,
  distanceOf,
  distanceUnit,
  fmtNum,
  fmtX,
  headlineRows,
  mergeRows,
  pickSweepMetric,
  timeUnit,
  unitAxis,
  xAxisFor,
  yRange,
} from "./chartUtils";

const ch = (elementId: string, portId: string, label: string, unit: string, ts: [number, number | null][] = []): Channel => ({
  elementId,
  portId,
  label,
  unit,
  timeSeries: ts.map(([t, value]) => ({ t, value })),
});
const runOf = (channels: Channel[]) => ({ id: "r", result: { channels } }) as unknown as SimRun;
/** n + 1 samples from 0 at a step of dt, valued by f(t). */
const grid = (n: number, dt: number, f: (t: number) => number): [number, number][] =>
  Array.from({ length: n + 1 }, (_, i) => [i * dt, f(i * dt)]);

describe("axes that fit the data (RES-18)", () => {
  it("reads time in s up to an hour, min up to 3 h, then h; distance in m below 1 km", () => {
    expect([600, 3600, 3601, 10800, 10801].map(timeUnit)).toEqual(["s", "s", "min", "min", "h"]);
    expect([75, 999, 1000, 23266].map(distanceUnit)).toEqual(["m", "m", "km", "km"]);
  });

  it("reads values out to 4 significant digits, whole numbers from 10,000 up", () => {
    expect([89.787, 365.07613, 12872.994, 0.30000000000000004, -0, -0.0123456].map(fmtNum)).toEqual([
      "89.79",
      "365.1",
      "12,873",
      "0.3",
      "0",
      "-0.01235",
    ]);
  });

  it("reads time and distance to the samples' resolution", () => {
    // at a 0.1 s step t holds float noise (step × 0.1)
    const tenth = xAxisFor("auto", [runOf([ch("b", "sig_soc", "B · SOC", "%", grid(600, 0.1, () => 1))])]);
    expect(fmtX(tenth, 12.300000000000002)).toBe("12.3 s");
    const hundredth = xAxisFor("s", [runOf([ch("b", "sig_soc", "B · SOC", "%", grid(100, 0.01, () => 1))])]);
    expect(fmtX(hundredth, 3.4600000000000004)).toBe("3.46 s");
    // WLTC: 23,266.5 m in 1,800 s
    const wltc = runOf([ch("v", "sig_distance", "Vehicle · Distance", "m", grid(1800, 1, (t) => (t * 23266.5) / 1800))]);
    const km = xAxisFor("distance", [wltc]);
    expect([km.label, km.csv]).toEqual(["Distance [km]", "distance_km"]);
    expect(fmtX(km, 5123.4567, 396)).toBe("5.12 km (t = 396 s)");
    expect(xAxisFor("min", [wltc]).label).toBe("t [min]");
  });

  it("labels ticks with the decimals of their step, in the axis's unit", () => {
    const at = (div: number, splits: number[], incr: number) =>
      (unitAxis(div).values as (u: unknown, s: number[], a: number, sp: number, i: number) => string[])(null, splits, 0, 0, incr);
    expect(at(1000, [5000, 5020], 20)).toEqual(["5.00", "5.02"]);
    expect(at(60, [300, 600], 300)).toEqual(["5", "10"]);
  });

  it("keeps the distance from going back, and keeps samples at the same distance apart", () => {
    const run = runOf([ch("v", "sig_distance", "Vehicle · Distance", "m", [[0, 0], [1, 5], [2, 4], [3, 6]])]);
    expect(distanceOf(run)).toEqual([0, 5, 5, 6]);
    expect(distanceOf(runOf([]))).toBeNull();
    // a stop at 5 m from t = 1 to 2 s, merged with another run's sample at 5 m
    const merged = mergeRows([
      { x: [0, 5, 5, 6], t: [0, 1, 2, 3], y: [10, 11, 12, 13] },
      { x: [5], t: [1.5], y: [20] },
    ]);
    expect(merged.x).toEqual([0, 5, 5, 5, 6]);
    expect(merged.t).toEqual([0, 1, 1.5, 2, 3]);
    expect(merged.cols).toEqual([
      [10, 11, undefined, 12, 13],
      [undefined, undefined, 20, undefined, undefined],
    ]);
  });

  it("fits a y axis to its data plus 5 %, at 0 or at set ends on request", async () => {
    // uPlot reads matchMedia as it loads
    vi.stubGlobal("matchMedia", () => ({ matches: false, addEventListener() {}, removeEventListener() {} }));
    const { default: uPlot } = await import("uplot");
    const range = (lo: number, hi: number, cfg = {}) => uPlot.rangeNum(lo, hi, yRange(cfg)) as [number, number];
    const share = (lo: number, hi: number) => {
      const [a, b] = range(lo, hi);
      return (hi - lo) / (b - a);
    };
    // BEV City Cycle: SOC 88.76-90 %, terminal voltage 362.5-369.7 V
    expect(share(88.764, 90)).toBeGreaterThanOrEqual(0.6);
    expect(share(362.47401, 369.69074)).toBeGreaterThanOrEqual(0.6);
    expect(range(88.764, 90, { zero: true })[0]).toBe(0);
    expect(range(88.764, 90, { min: 80, max: 100 })).toEqual([80, 100]);
    const top = range(88.764, 90, { max: 400 });
    expect(top[1]).toBe(400);
    expect(top[0]).toBeGreaterThan(80); // the other end still fits the data
    expect(range(88.764, 90, { min: 95, max: 90 })).toEqual(range(88.764, 90)); // crossed: both automatic
    vi.unstubAllGlobals();
  });
});

// summary labels in the order the engine writes them (core.py, verdict.py)
const labels = (rows: string[]) => headlineRows(rows.map((label) => ({ label }))).map((r) => r.label);
const BATTERY = ["HV Battery — final SOC", "HV Battery — energy delivered", "HV Battery — energy recuperated", "HV Battery — internal losses"];

describe("headline numbers and the first plot (RES-30)", () => {
  it("leads an electric car's headline with consumption and distance, which the engine lists 5th and 6th", () => {
    expect(labels([...BATTERY, "Distance driven", "Consumption", "Electrical energy balance error", "Simulated duration"])).toEqual([
      "Consumption",
      "Distance driven",
      "HV Battery — final SOC",
      "HV Battery — energy delivered",
      "HV Battery — energy recuperated",
    ]);
  });

  it("leads a hybrid's headline with fuel consumption and keeps at most six numbers", () => {
    const hybrid = [...BATTERY, "Engine — fuel used", "Distance driven", "Fuel consumption", "CO₂ emissions", "Simulated duration"];
    expect(labels(hybrid)).toEqual([
      "Fuel consumption",
      "Distance driven",
      "HV Battery — final SOC",
      "HV Battery — energy delivered",
      "HV Battery — energy recuperated",
      "CO₂ emissions",
    ]);
    expect(labels(["Consumption", ...hybrid])).toHaveLength(6);
  });

  it("leads a test's headline with its time, and an acceleration test's with its speed at the line", () => {
    const perf = [...BATTERY, "Distance driven", "Consumption", "Maximum speed", "Time to 100 km/h", "Simulated duration"];
    expect(labels(perf).slice(0, 3)).toEqual(["Time to 100 km/h", "Maximum speed", "Consumption"]);
    const accel = ["Time to 75 m", "Speed at 75 m", "Time to 100 km/h", ...BATTERY, "Distance driven"];
    expect(labels(accel).slice(0, 3)).toEqual(["Time to 75 m", "Time to 100 km/h", "Speed at 75 m"]);
  });

  it("puts a failed check first, finds a lap time, and falls back to a bench model's first rows", () => {
    const rows = [
      { label: "Lap time" },
      { label: "Speed at the finish" },
      { label: "Energy per lap" },
      { label: "Accumulator — usable energy left", passed: false },
      { label: "Distance driven" },
    ];
    expect(headlineRows(rows).map((r) => r.label)).toEqual(["Accumulator — usable energy left", "Lap time", "Distance driven"]);
    expect(labels(["Voltage Source — energy supplied", "Simulated duration"])).toEqual([
      "Voltage Source — energy supplied",
      "Simulated duration",
    ]);
  });

  it("opens a sweep on the first headline number: the test's time, the fuel or the energy used", () => {
    expect(pickSweepMetric([...BATTERY, "Distance driven", "Consumption"])).toBe("Consumption");
    expect(pickSweepMetric([...BATTERY, "Distance driven", "Fuel consumption", "CO₂ emissions"])).toBe("Fuel consumption");
    expect(pickSweepMetric([...BATTERY, "Consumption", "Maximum speed", "Time to 100 km/h"])).toBe("Time to 100 km/h");
    expect(pickSweepMetric(["Voltage Source — energy supplied"])).toBe("Voltage Source — energy supplied");
    expect(pickSweepMetric([])).toBe("");
  });

  it("opens the plot on target against actual speed, then SOC and battery power", () => {
    const channels = [
      ch("el-battery", "sig_power", "HV Battery · Discharge Power", "kW"),
      ch("el-battery", "sig_soc", "HV Battery · SOC", "%"),
      ch("el-motor", "sig_speed", "E-Motor · Shaft Speed", "1/min"),
      ch("el-task", "sig_demand", "Vehicle Task · Target Speed", "km/h"),
      ch("el-vehicle", "sig_speed", "Vehicle · Vehicle Speed", "km/h"),
    ];
    expect(defaultChannelKeys(channels)).toEqual([
      "el-task:sig_demand",
      "el-vehicle:sig_speed",
      "el-battery:sig_soc",
      "el-battery:sig_power",
    ]);
    // no speed to follow (a lap or an acceleration test): actual speed first
    expect(defaultChannelKeys(channels.filter((c) => c.portId !== "sig_demand"))[0]).toBe("el-vehicle:sig_speed");
    // nothing known: the first two channels
    expect(defaultChannelKeys([channels[2], channels[2]])).toHaveLength(2);
  });
});
