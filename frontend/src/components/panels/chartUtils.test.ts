import { describe, expect, it, vi } from "vitest";
import type { Channel, SimRun } from "../../types";
import {
  INTEGRAL_UNITS,
  defaultChannelKeys,
  distanceOf,
  distanceUnit,
  firstReach,
  fmtNum,
  fmtX,
  headlineRows,
  mergeRows,
  nearestIndex,
  pickSweepMetric,
  timeUnit,
  unitAxis,
  windowStats,
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
    const range = (lo: number, hi: number, cfg = {}) => yRange(cfg, uPlot.rangeNum)(null as never, lo, hi, "%") as [number, number];
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
    // one end set past all the data: kept, and the axis does not turn upside down
    for (const one of [{ max: 50 }, { min: 95 }, { max: -50 }] as { min?: number; max?: number }[]) {
      const [lo, hi] = range(88.764, 90, one);
      expect(lo).toBeLessThan(hi);
      expect(lo === one.min || hi === one.max).toBe(true);
    }
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
    // a pack of another name (Formula Student's Accumulator): its power still
    const fs = channels.map((c) => ({ ...c, label: c.label.replace("HV Battery", "Accumulator") }));
    expect(defaultChannelKeys(fs)).toContain("el-battery:sig_power");
    // nothing known: the first two channels
    expect(defaultChannelKeys([channels[2], channels[2]])).toHaveLength(2);
  });
});

/** Samples at t = 0, 1, 2, … with these values. */
const series = (values: (number | null)[]) => values.map((value, t) => ({ t, value }));

describe("measurement cursors (RES-06)", () => {
  it("finds the sample nearest a time, the earlier one on a tie", () => {
    const at = (t: number, ts = [0, 1, 2]) => nearestIndex(ts.length, t, (i) => ts[i]);
    expect([1.4, 1.5, 1.6, -5, 99].map((t) => at(t))).toEqual([1, 1, 2, 0, 2]);
    expect(at(1, [])).toBe(-1);
    // standing still on a distance axis: the first sample at that distance
    expect(at(5, [0, 5, 5, 5, 6])).toBe(1);
  });

  it("gives a window's values and its statistics, weighted by time", () => {
    const ramp = series(Array.from({ length: 11 }, (_, t) => t)); // v = t over 0-10 s
    const w = windowStats(ramp, 0, 10)!;
    expect([w.a, w.b, w.min, w.max, w.integral, w.mean]).toEqual([0, 10, 0, 10, 50, 5]);
    expect(w.rms).toBeCloseTo(Math.sqrt(33.5), 12); // the trapezoid rule on v²
    // B before A: the same window, each value at its own cursor
    const back = windowStats(ramp, 10, 0)!;
    expect([back.a, back.b, back.integral, back.mean, back.rms]).toEqual([10, 0, w.integral, w.mean, w.rms]);
    // a gap drops the pairs on either side of it: 0-2 and 3-4 s are left
    const gap = windowStats(series([1, 1, 1, null, 1, 1]), 0, 5)!;
    expect([gap.integral, gap.mean]).toEqual([3, 1]);
    expect(windowStats(series([null, null]), 0, 1)).toBeNull();
    // one sample: its own value
    const one = windowStats(series([3, -4]), 1, 1)!;
    expect([one.a, one.mean, one.rms, one.integral]).toEqual([-4, -4, 4, 0]);
  });

  it("finds where a signal first reaches a value, rising, falling or exactly", () => {
    const speed = series([0, 20, 40, 60, 80, 60, 30, null, 10]);
    expect(firstReach(speed, 50)).toBe(3); // the sample after it crossed
    expect(firstReach(speed, 40)).toBe(2); // on the sample
    expect(firstReach(speed, 50, 4)).toBe(6); // falling (60 to 30), from index 4 on
    expect(firstReach(speed, 20, 6)).toBe(8); // across the gap
    expect(firstReach(speed, 100)).toBe(-1);
  });

  it("integrates rates into energy, charge, distance, mass and turns", () => {
    const [kWh, unit] = INTEGRAL_UNITS.kW;
    expect([windowStats(series(Array(3601).fill(1)), 0, 3600)!.integral * kWh, unit]).toEqual([1, "kWh"]);
    const [m] = INTEGRAL_UNITS["km/h"];
    expect(windowStats(series(Array(11).fill(36)), 0, 10)!.integral * m).toBeCloseTo(100, 12);
    expect(Object.values(INTEGRAL_UNITS).map(([, u]) => u)).toEqual(["kWh", "Ah", "m", "kg", "rev"]);
    for (const none of ["V", "%", "°C", "N·m", "g", "-"]) expect(INTEGRAL_UNITS[none]).toBeUndefined();
  });

  it("measures a 1-hour run at 0.1 s (36,001 samples) in 10 series well inside a frame", () => {
    const runs = Array.from({ length: 10 }, (_, k) =>
      Array.from({ length: 36001 }, (_, i) => ({ t: i / 10, value: Math.sin(i / 100 + k) * 50 })),
    );
    const times: number[] = [];
    for (let r = 0; r < 20; r++) {
      const t0 = performance.now();
      for (const ts of runs) {
        const at = (t: number) => nearestIndex(ts.length, t, (i) => ts[i].t);
        windowStats(ts, at(10 + r), at(3590 - r));
      }
      if (r >= 10) times.push(performance.now() - t0); // after 10 moves to warm up
    }
    times.sort((a, b) => a - b);
    // the item's budget for a cursor move is 8 ms, drawing included (about
    // 1.4 ms here on a desktop PC)
    expect(times[5]).toBeLessThan(8);
  });
});

describe("numbers and legends a person reads", () => {
  it("a channel is named with its part where another has the same port name (app pass #7)", async () => {
    const { channelName } = await import("./chartUtils");
    const rl = ch("brake-rl", "sig_cmd", "Brake RL · Brake Command", "-");
    const fl = ch("brake-fl", "sig_cmd", "Brake FL · Brake Command", "-");
    const v = ch("veh", "sig_speed", "Vehicle · Vehicle Speed", "km/h");
    expect(channelName(rl, [rl, fl, v])).toBe("Brake RL · Brake Command");
    expect(channelName(v, [rl, fl, v])).toBe("Vehicle Speed");
    // the same channel of another run is not another channel
    expect(channelName(rl, [rl, { ...rl }, v])).toBe("Brake Command");
  });

  it("a read-out of the solver's rounding noise reads 0", async () => {
    const { fmtNum } = await import("./chartUtils");
    expect(fmtNum(-1.773e-14)).toBe("0");
    expect(fmtNum(-0)).toBe("0");
    expect(fmtNum(0.0001234)).toBe("0.0001234");
    expect(fmtNum(88.7612)).toBe("88.76");
  });

  it("a tiny percentage reads as one, never in exponent form", async () => {
    const { fmtPct } = await import("./chartUtils");
    expect(fmtPct(1.2740478785764536e-7)).toBe("< 0.001");
    expect(fmtPct(-3e-9)).toBe("> −0.001");
    expect(fmtPct(0)).toBe("0");
    expect(fmtPct(0.0123456)).toBe("0.0123");
    expect(fmtPct(12.3456)).toBe("12.3");
    for (const v of [1e-7, 2.5e-12, 123456.7]) expect(fmtPct(v)).not.toMatch(/e[-+]?\d/i);
  });

  it("the legend reads each line's last sample with the pointer off the chart, not a dash", async () => {
    const { legendIdx } = await import("./chartUtils");
    const xs = [0, 1, 2, 3, 4];
    expect(legendIdx([1, 2, 3, null, undefined], xs, null)).toBe(3); // a null sample is the line's; a gap is not
    expect(legendIdx([5, undefined, 7, undefined, undefined], xs, null)).toBe(2);
    expect(legendIdx([undefined, undefined], [0, 1], null)).toBeNull();
    // a line that ended well before the chart's end has no value there (app
    // pass #27: a 600 s run drawn with an 1800 s one, legend at 1,800 s) …
    const t = [0, 300, 600, 900, 1200, 1500, 1800];
    expect(legendIdx([1, 2, 3, undefined, undefined, undefined, undefined], t, null)).toBeNull();
    // … but one on another grid that ends a step short of it still reads
    expect(legendIdx([1, 2, 3, 4, 5, 6, undefined], [0, 300, 600, 900, 1200, 1500, 1500.1], null)).toBe(5);
    // at the pointer: the nearest sample along x, none past the line's ends
    expect(legendIdx([5, undefined, undefined, 8, undefined], xs, 1)).toBe(0);
    expect(legendIdx([5, undefined, undefined, 8, undefined], xs, 2)).toBe(3);
    expect(legendIdx([5, undefined, undefined, 8, undefined], xs, 4)).toBeNull();
  });

  it("a zoom box drawn in another window ends on that window's mouseup", async () => {
    vi.stubGlobal("matchMedia", () => ({ matches: false, addEventListener() {}, removeEventListener() {} }));
    const { endDragOn } = await import("./Plot");
    const frame = document.body.appendChild(document.createElement("iframe"));
    const page = frame.contentDocument!;
    const ups: MouseEvent[] = [];
    const onUp = (e: MouseEvent) => ups.push(e);
    document.addEventListener("mouseup", onUp);
    endDragOn(page);
    page.dispatchEvent(new (frame.contentWindow as unknown as typeof window).MouseEvent("mouseup", { clientX: 120, clientY: 40, button: 0 }));
    expect(ups).toHaveLength(1);
    expect([ups[0].clientX, ups[0].clientY, ups[0].button]).toEqual([120, 40, 0]);
    page.dispatchEvent(new MouseEvent("mouseup", { clientX: 1 })); // only the drag's own mouseup
    expect(ups).toHaveLength(1);
    endDragOn(document); // the main window's page: uPlot hears it already
    document.dispatchEvent(new MouseEvent("mouseup"));
    expect(ups).toHaveLength(2);
    document.removeEventListener("mouseup", onUp);
    frame.remove();
  });
});
