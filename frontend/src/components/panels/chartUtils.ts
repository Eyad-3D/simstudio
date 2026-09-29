// Shared chart helpers used by the Results panel, its measurement cursors and
// the dockable mini-chart.
// Nothing here imports uPlot at run time (only its types), so these unit-test
// without a browser.
import { useCallback, useState } from "react";
import type uPlot from "uplot";
import type { Channel, SimRun } from "../../types";

/** Series colours for chart lines and swatches (at least 3:1 on the panel).
 *  Several are too faint for text, so names are drawn in the text colour. */
export const PALETTE = [
  "#2f6fb3",
  "#d97706",
  "#059669",
  "#dc2626",
  "#8b5cf6",
  "#0e7490",
  "#d0457f",
  "#4d7c0f",
  "#b45309",
  "#4d7ce8",
];

export function channelKey(c: Channel): string {
  return `${c.elementId}:${c.portId}`;
}

/** True once the element has a real size — dockview keeps hidden tabs at 0×0,
 *  where a chart cannot be drawn. `ref` is a callback ref, so the observer
 *  attaches whenever the chart area mounts: the panels render it only after
 *  their "no runs yet" placeholder, or swap it between views. */
export function useHasSize<T extends HTMLElement>() {
  const [hasSize, setHasSize] = useState(false);
  const ref = useCallback((node: T | null) => {
    if (!node) return;
    const obs = new ResizeObserver((entries) => {
      const r = entries[0]?.contentRect;
      setHasSize(!!r && r.width > 0 && r.height > 0);
    });
    obs.observe(node);
    return () => {
      obs.disconnect();
      setHasSize(false);
    };
  }, []);
  return { ref, hasSize };
}

// ---- axes and read-outs (RES-18) ----

const sig4 = new Intl.NumberFormat(undefined, { maximumSignificantDigits: 4 });
const whole = new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 });
/** A value read out under the pointer: 4 significant digits (whole numbers
 *  from 10,000 up), so the stored rounding's last digits do not show; -0 reads 0. */
export const fmtNum = (v: number | null | undefined) =>
  v == null ? "—" : (Math.abs(v) >= 1e4 ? whole : sig4).format(v + 0);

export type XAxisMode = "auto" | "s" | "min" | "h" | "distance";
/** The time or distance axis, as plain data (charts memoise their options on
 *  its JSON). The data stay in s or m: `div` turns them into the display
 *  unit, `dec` and `tDec` are the decimals a read-out needs for the samples'
 *  spacing in that unit and in s. */
export type XAxis = { kind: "t" | "distance"; unit: string; div: number; label: string; csv: string; dec: number; tDec: number };

const DIV: Record<string, number> = { s: 1, min: 60, h: 3600, m: 1, km: 1000 };

/** The time unit for a run this long: s up to an hour, min up to 3 h, then h. */
export const timeUnit = (spanS: number) => (spanS <= 3600 ? "s" : spanS <= 10800 ? "min" : "h");
/** Distance in m below 1 km, in km from there. */
export const distanceUnit = (maxM: number) => (maxM < 1000 ? "m" : "km");

/** A run's sample times: its longest channel's (a live run's channels can start late). */
export const timesOf = (r: SimRun) =>
  r.result.channels.reduce<Channel["timeSeries"]>((a, c) => (c.timeSeries.length > a.length ? c.timeSeries : a), []);

/** The Vehicle's distance at each sample (m), made never to go back (a
 *  running maximum) so that an axis of it stays sorted; null without a Vehicle. */
export function distanceOf(run: SimRun): number[] | null {
  const ch = run.result.channels.find((c) => c.portId === "sig_distance");
  if (!ch) return null;
  // ponytail: a car rolling back reads as standing still on this axis; plot
  // Vehicle · Distance against time to see it
  let far = 0;
  return ch.timeSeries.map((p) => (far = Math.max(far, p.value ?? far)));
}

/** Decimals that tell apart samples this far apart (at most 4). */
const stepDecimals = (step: number) => (step > 0 && step < Infinity ? Math.min(4, Math.max(0, Math.ceil(-Math.log10(step) - 1e-9))) : 0);

/** The x axis for `mode` over the plotted runs: auto time reads in the unit
 *  that fits the longest run; distance needs a Vehicle in every run (the
 *  caller falls back to auto when one has none). */
export function xAxisFor(mode: XAxisMode, runs: SimRun[]): XAxis {
  let span = 0;
  let step = Infinity;
  for (const r of runs) {
    const ts = timesOf(r);
    if (ts.length < 2) continue;
    span = Math.max(span, ts[ts.length - 1].t - ts[0].t);
    step = Math.min(step, (ts[ts.length - 1].t - ts[0].t) / (ts.length - 1));
  }
  const tDec = stepDecimals(step);
  if (mode === "distance") {
    let far = 0;
    let dStep = Infinity;
    for (const r of runs) {
      const d = distanceOf(r);
      if (!d || d.length < 2) continue;
      far = Math.max(far, d[d.length - 1]);
      dStep = Math.min(dStep, (d[d.length - 1] - d[0]) / (d.length - 1));
    }
    const unit = distanceUnit(far);
    return { kind: "distance", unit, div: DIV[unit], label: `Distance [${unit}]`, csv: `distance_${unit}`, dec: stepDecimals(dStep / DIV[unit]), tDec };
  }
  const unit = mode === "auto" ? timeUnit(span) : mode;
  return { kind: "t", unit, div: DIV[unit], label: `t [${unit}]`, csv: `t_${unit}`, dec: stepDecimals(step / DIV[unit]), tDec };
}

/** An x value read out in the axis's unit, to the samples' resolution; on a
 *  distance axis with the sample's time: "5.12 km (t = 612 s)". */
export function fmtX(x: XAxis, v: number, t?: number | null): string {
  const s = `${(v / x.div + 0).toLocaleString(undefined, { maximumFractionDigits: x.dec })} ${x.unit}`;
  return x.kind === "distance" && t != null ? `${s} (t = ${(t + 0).toLocaleString(undefined, { maximumFractionDigits: x.tDec })} s)` : s;
}

/** Round tick steps: 1, 2, 2.5 and 5 times a power of ten. */
const NICE = Array.from({ length: 16 }, (_, e) => [1, 2, 2.5, 5].map((m) => +`${m}e${e - 6}`)).flat();
const decimals = (step: number) => (String(+step.toPrecision(12)).split(".")[1] ?? "").length;

/** Ticks at round steps of the display unit (data ÷ `div`), each labelled
 *  with the decimals its step needs, so a deep zoom never repeats a label.
 *  `div` stays on the axis for its accessible name (Plot.tsx). */
export function unitAxis(div: number): Pick<uPlot.Axis, "incrs" | "values"> & { div: number } {
  return {
    div,
    incrs: NICE.map((i) => i * div),
    values: (_u, splits, _axis, _space, incr) => {
      const d = decimals(incr / div);
      return splits.map((v) => (v / div + 0).toLocaleString(undefined, { minimumFractionDigits: d, maximumFractionDigits: d }));
    },
  };
}

/** One unit's y axis settings: start at 0, or fixed ends (absent: automatic). */
export type YAxisCfg = { zero?: boolean; min?: number; max?: number };

/** A y scale's range: the data plus 5 % at round ends, reaching 0 only when
 *  that range comes to it (with `zero`, always); a fixed end stays where it
 *  is set, and the other end stays on its far side even when all the data
 *  lie past it (a maximum below them all). A minimum at or above the maximum
 *  is ignored. `rangeNum` is uPlot's, passed in so that this file does not
 *  load uPlot. */
export function yRange({ zero, min, max }: YAxisCfg, rangeNum: typeof uPlot.rangeNum): uPlot.Range.Function {
  if (min != null && max != null && min >= max) min = max = undefined;
  const auto = { pad: 0.05, soft: 0, mode: zero ? 1 : 3 } as const;
  const at = (v: number) => ({ soft: v, hard: v, mode: 1 }) as const;
  const cfg = { min: min != null ? at(min) : auto, max: max != null ? at(max) : auto };
  // the data only as far as the set ends, so the automatic end is fitted to what can show
  const lim = (v: number) => Math.min(max ?? Infinity, Math.max(min ?? -Infinity, v));
  return (_u, lo, hi) => (lo == null ? [null, null] : rangeNum(lim(lo), lim(hi), cfg));
}

/** Columns for series on grids of their own: a row per distinct (x, t)
 *  pair, in order of x then t, so samples at the same x (standing still on a
 *  distance axis) keep a row each; a series is undefined where it has no
 *  sample. Each series' pairs must come in that order already. */
export function mergeRows(series: { x: ArrayLike<number | undefined>; t: ArrayLike<number>; y: ArrayLike<number | null> }[]) {
  const pairs: [number, number][] = [];
  for (const s of series) for (let j = 0; j < s.t.length; j++) if (s.x[j] !== undefined) pairs.push([s.x[j]!, s.t[j]]);
  pairs.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const rows = pairs.filter((p, i) => i === 0 || p[0] !== pairs[i - 1][0] || p[1] !== pairs[i - 1][1]);
  const cols = series.map((s) => {
    const col = new Array<number | null | undefined>(rows.length).fill(undefined);
    for (let j = 0, k = 0; j < s.t.length; j++) {
      if (s.x[j] === undefined) continue;
      while (rows[k][0] !== s.x[j] || rows[k][1] !== s.t[j]) k++;
      col[k] = s.y[j];
    }
    return col;
  });
  return { x: rows.map((r) => r[0]), t: rows.map((r) => r[1]), cols };
}

// ---- measurement cursors (RES-06) ----

type Sample = Channel["timeSeries"][number];

/** The index of the value nearest `at` among n values x(0)…x(n-1) in rising
 *  order (on a tie, the earlier one); -1 when there are none. */
export function nearestIndex(n: number, at: number, x: (i: number) => number): number {
  let lo = 0;
  let hi = n - 1;
  if (hi < 0) return -1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (x(mid) < at) lo = mid + 1;
    else hi = mid;
  }
  return lo > 0 && at - x(lo - 1) <= x(lo) - at ? lo - 1 : lo;
}

/** A signal between samples i0 and i1 (in either order): its values there
 *  (a, b), its lowest and highest, and, weighted by time with the trapezoid
 *  rule, its integral, mean and RMS. A pair of samples with a gap (null) in
 *  it is left out. Null when there are only gaps. */
export function windowStats(ts: Sample[], i0: number, i1: number) {
  // ponytail: a pass over the window on every cursor move; about 28 series of
  // 36,000 samples reach 8 ms (p95) a move. Cached prefix sums per channel
  // make it O(1) if longer runs or more overlays need it.
  const [lo, hi] = i0 <= i1 ? [i0, i1] : [i1, i0];
  let min = Infinity;
  let max = -Infinity;
  let integral = 0;
  let squares = 0;
  let span = 0;
  for (let i = lo; i <= hi; i++) {
    const v = ts[i].value;
    if (v === null) continue;
    if (v < min) min = v;
    if (v > max) max = v;
    const p = i > lo ? ts[i - 1].value : null;
    if (p === null) continue;
    const dt = ts[i].t - ts[i - 1].t;
    integral += ((p + v) / 2) * dt;
    squares += ((p * p + v * v) / 2) * dt;
    span += dt;
  }
  if (min === Infinity) return null;
  const a = ts[i0].value;
  const b = ts[i1].value;
  // one sample: its own value
  const one = a ?? b ?? min;
  return { a, b, min, max, integral, mean: span > 0 ? integral / span : one, rms: span > 0 ? Math.sqrt(squares / span) : Math.abs(one) };
}

/** The first sample from index `from` on where the signal reaches `target`:
 *  it equals it, or has crossed it since the last sample before (gaps are
 *  stepped over); -1 when it never does. */
export function firstReach(ts: Sample[], target: number, from = 0): number {
  let prev: number | null = null;
  for (let i = Math.max(0, from); i < ts.length; i++) {
    const v = ts[i].value;
    if (v === null) continue;
    if (v === target || (prev !== null && (prev - target) * (v - target) < 0)) return i;
    prev = v;
  }
  return -1;
}

/** A rate's integral over time: [factor from unit × s, the unit it is in].
 *  Other units (V, %, °C, N·m, g, …) have none. */
export const INTEGRAL_UNITS: Record<string, [number, string]> = {
  kW: [1 / 3600, "kWh"],
  A: [1 / 3600, "Ah"],
  "km/h": [1 / 3.6, "m"],
  "kg/h": [1 / 3600, "kg"],
  "1/min": [1 / 60, "rev"],
};

// ---- headline numbers and the first plot (RES-30) ----

/** The channels a run's plot opens on until the user picks some: did the car
 *  follow the cycle (target against actual speed), then SOC and battery
 *  power. The order sets the colours. */
export function defaultChannelKeys(channels: Channel[]): string[] {
  const batteries = new Set(channels.filter((c) => c.portId === "sig_soc").map((c) => c.elementId));
  const wanted: ((c: Channel) => boolean)[] = [
    (c) => c.portId === "sig_demand", // Driving Task · Target Speed
    (c) => c.portId === "sig_speed" && c.label.endsWith(" · Vehicle Speed"),
    (c) => c.portId === "sig_soc",
    (c) => c.portId === "sig_power" && batteries.has(c.elementId), // whatever the pack is called
  ];
  const keys = wanted.flatMap((f) => channels.filter(f)).slice(0, 4).map(channelKey);
  return keys.length > 0 ? keys : channels.slice(0, 2).map(channelKey);
}

// Summary rows shown as headline numbers, in this order, when the run has
// them (labels as the engine writes them: backend/app/solver/core.py,
// verdict.py, lapsim.py)
const HEADLINE = [
  /^Time to /, // a test's time: to 100 km/h, or over its distance (Time to 75 m)
  /^Speed at \d/, // an acceleration test's speed at the line
  /^Maximum speed$/,
  /^Lap time$/,
  /^Fuel consumption$/,
  /^Consumption$/,
  /^Distance driven$/,
  / — final SOC$/,
  / — energy delivered$/,
  / — energy recuperated$/,
  /^CO₂ emissions$/,
];

/** The run's headline numbers (at most 6) from its summary rows: a failed
 *  check first, then the rows above; a model with none of them (a test
 *  bench) shows its first rows. */
export function headlineRows<T extends { label: string; passed?: boolean | null }>(summary: T[]): T[] {
  const picked = new Set([
    ...summary.filter((s) => s.passed === false),
    ...HEADLINE.flatMap((re) => summary.filter((s) => re.test(s.label))),
  ]);
  return (picked.size > 0 ? [...picked] : summary).slice(0, 6);
}

/** The summary figure a sweep opens on: the first headline number it has
 *  (a test's time, fuel or energy consumption), else its first. */
export const pickSweepMetric = (labels: string[]) => headlineRows(labels.map((label) => ({ label })))[0]?.label ?? "";
