// The run reports the engine sends with a finished run: where the energy
// went (RES-22), what held the car back (RES-38) and each part's duty
// (RES-39). Pure helpers for the Results page, the diagram and the studies.

import { csvText } from "./components/panels/csv";
import type { DutyPart, EnergyFlow, EnergyReport, LimitLane, SimResult } from "./types";

// Colours: the dataviz reference palette's light and dark steps, in an
// order that passes its colour-vision checks in both themes (only four hues
// stay apart in every pairing, so the battery's own limit shares the set
// limit's hue, hatched). Every state and band is also named in text.
type Look = { label: string; light: string; dark: string; hint: string; hatch?: boolean };

/** What each limit state is called and drawn in. */
export const LIMIT_LOOK: Record<string, Look> = {
  grip: { label: "Tyre grip", light: "#eda100", dark: "#c98500", hint: "a driven wheel was at its tyres' grip limit (it slipped)" },
  set_limit: {
    label: "Set power limit",
    light: "#e87ba4",
    dark: "#d55181",
    hint: "the battery's Output Power Limit held the motors back (for example the Formula Student 80 kW)",
  },
  supply: {
    label: "Battery or supply",
    light: "#e87ba4",
    dark: "#d55181",
    hatch: true,
    hint: "the battery, fuel cell or converter could not give more",
  },
  machine: {
    label: "Motor or engine",
    light: "#2a78d6",
    dark: "#3987e5",
    hint: "the driver asked for full torque, or a motor was at its maximum speed: the machine gave all it had",
  },
  demand: { label: "Driver's demand met", light: "#008300", dark: "#008300", hint: "nothing held the car back: it did what the driver asked" },
  coasting: { label: "Coasting", light: "#c6cbd3", dark: "#4a515c", hint: "no drive and no braking asked for" },
  braking: { label: "Braking", light: "#8a919c", dark: "#7b828d", hint: "the driver braked (friction brakes or regeneration)" },
};

export const lookColor = (l: { light: string; dark: string } | undefined, dark: boolean) =>
  l ? (dark ? l.dark : l.light) : "#8a8f98";

/** A lane's changes as segments [t0, t1, state]. */
export function limitSegments(lane: LimitLane, states: string[], tEnd: number): [number, number, string][] {
  return lane.changes.map(([t, code], i) => [t, i + 1 < lane.changes.length ? lane.changes[i + 1][0] : tEnd, states[code]]);
}

/** A lane's seconds per state, longest first. */
export function limitShares(lane: LimitLane): [string, number][] {
  return Object.entries(lane.seconds)
    .filter(([, s]) => s > 0)
    .sort((a, b) => b[1] - a[1]);
}

// ---- energy -------------------------------------------------------------

/** The Sankey's groups, in drawing order, with their names and colours;
 *  "source" and "released" are the left side's. */
export const ENERGY_GROUPS: Record<string, { label: string; light: string; dark: string }> = {
  source: { label: "Sources", light: "#2a78d6", dark: "#3987e5" },
  released: { label: "Given back by the car", light: "#1baf7a", dark: "#199e70" },
  road: { label: "Driving: air and rolling", light: "#eb6834", dark: "#d95926" },
  stored: { label: "Kept as speed or height", light: "#1baf7a", dark: "#199e70" },
  brakes: { label: "Friction brakes", light: "#4a3aa7", dark: "#9085e9" },
  losses: { label: "Losses in parts", light: "#eda100", dark: "#c98500" },
  loads: { label: "Used by loads", light: "#e87ba4", dark: "#d55181" },
  recovered: { label: "Charged back", light: "#008300", dark: "#008300" },
  remainder: { label: "Not accounted for", light: "#8a919c", dark: "#7b828d" },
};
const SINK_ORDER = ["road", "stored", "brakes", "losses", "loads", "recovered", "remainder"];

export interface SankeyNode {
  id: string;
  label: string;
  kWh: number;
  color: string;
  column: number;
  y: number;
  h: number;
  elementId?: string | null;
}
export interface SankeyLink {
  from: string;
  to: string;
  kWh: number;
  color: string;
  // the band's top edge at each end, and its thickness
  y0: number;
  y1: number;
  h: number;
}

/** Below this share of the sources a sink is folded into its group's
 *  "other" node, so the chart stays readable. */
export const SANKEY_MIN_SHARE = 0.01;

/** Four columns: sources → the energy in → where it went (groups) → each
 *  sink. Band widths are proportional to energy; what the books do not
 *  explain is a band of its own (on the right when the sinks fall short,
 *  on the left when they exceed the sources). `height` is the chart's
 *  height for the energy, without gaps. */
export function sankeyLayout(e: EnergyReport, dark = false, height = 300, gap = 6) {
  const sources: EnergyFlow[] = [...e.sources];
  const sinks: EnergyFlow[] = [...e.sinks];
  const rem = e.remainderKWh;
  // (a remainder within the books' rounding is no band)
  if (Math.abs(rem) > 1e-4 * Math.max(e.sourceKWh, 1e-9)) {
    const band = { label: "Not accounted for", kWh: Math.abs(rem), group: "remainder" };
    if (rem > 0) sinks.push(band);
    else sources.push(band);
  }
  const total = Math.max(
    sources.reduce((a, f) => a + f.kWh, 0),
    sinks.reduce((a, f) => a + f.kWh, 0),
    1e-12,
  );
  const k = height / total;
  const nodes: SankeyNode[] = [];
  const links: SankeyLink[] = [];
  const colorOf = (g: string) => lookColor(ENERGY_GROUPS[g], dark);

  // column 0: the sources, largest first
  let y = 0;
  const srcSorted = sources.sort((a, b) => b.kWh - a.kWh);
  srcSorted.forEach((f, i) => {
    const g = f.group === "remainder" ? "remainder" : f.group;
    nodes.push({ id: `s${i}`, label: f.label, kWh: f.kWh, color: colorOf(g), column: 0, y, h: f.kWh * k, elementId: f.elementId });
    y += f.kWh * k + gap;
  });
  const sumIn = srcSorted.reduce((a, f) => a + f.kWh, 0);
  const sumOut = sinks.reduce((a, f) => a + f.kWh, 0);
  // column 1: the energy in, one node
  const mid = Math.max(sumIn, sumOut);
  nodes.push({ id: "in", label: "Energy in", kWh: sumIn, color: colorOf("source"), column: 1, y: 0, h: mid * k });
  let yIn = 0;
  nodes
    .filter((n) => n.column === 0)
    .forEach((n) => {
      links.push({ from: n.id, to: "in", kWh: n.kWh, color: n.color, y0: n.y, y1: yIn, h: n.h });
      yIn += n.h;
    });

  // columns 2 and 3: the groups, then each sink (small ones folded)
  let yOut = 0;
  let yG = 0;
  let yS = 0;
  for (const g of SINK_ORDER) {
    const members = sinks.filter((f) => f.group === g).sort((a, b) => b.kWh - a.kWh);
    if (members.length === 0) continue;
    const sum = members.reduce((a, f) => a + f.kWh, 0);
    const gid = `g:${g}`;
    nodes.push({ id: gid, label: ENERGY_GROUPS[g]?.label ?? g, kWh: sum, color: colorOf(g), column: 2, y: yG, h: sum * k });
    links.push({ from: "in", to: gid, kWh: sum, color: colorOf(g), y0: yOut, y1: yG, h: sum * k });
    yOut += sum * k;
    const big = members.filter((f) => f.kWh >= SANKEY_MIN_SHARE * total);
    const small = members.filter((f) => f.kWh < SANKEY_MIN_SHARE * total);
    const items: { label: string; kWh: number; elementId?: string | null }[] = [...big];
    if (small.length === 1) items.push(small[0]);
    else if (small.length > 1)
      items.push({ label: `${small.length} more, each under ${SANKEY_MIN_SHARE * 100} %`, kWh: small.reduce((a, f) => a + f.kWh, 0) });
    // one item that is the whole group needs no node of its own
    let yFrom = yG;
    items.forEach((f, i) => {
      const id = `${gid}:${i}`;
      nodes.push({ id, label: f.label, kWh: f.kWh, color: colorOf(g), column: 3, y: yS, h: f.kWh * k, elementId: f.elementId });
      links.push({ from: gid, to: id, kWh: f.kWh, color: colorOf(g), y0: yFrom, y1: yS, h: f.kWh * k });
      yFrom += f.kWh * k;
      yS += f.kWh * k + 2;
    });
    yG += sum * k + gap;
    yS += gap - 2;
  }
  const bottom = Math.max(y - gap, yG - gap, yS - gap, mid * k);
  return { nodes, links, total, bottom };
}

const csv = (rows: (string | number)[][]) => csvText(rows) + "\n";

/** The energy table and the Sankey's bands as CSV. */
export function energyCsv(e: EnergyReport): string {
  const rows: (string | number)[][] = [
    ["part", "kind", "in_kWh", "out_kWh", "lost_kWh", "stored_change_kWh", "lost_pct_of_sources"],
    ...e.parts.map((p) => [p.label, p.kind, p.inKWh, p.outKWh, p.lostKWh, p.storedKWh, p.lostPct]),
    [],
    ["flow", "group", "kWh"],
    ...e.sources.map((f) => [f.label, f.group, f.kWh]),
    ...e.sinks.map((f) => [f.label, f.group, f.kWh]),
    ["Not accounted for", "remainder", e.remainderKWh],
    ["Sources together", "total", e.sourceKWh],
  ];
  return csv(rows);
}

// ---- duty -----------------------------------------------------------------

/** The duty table as CSV, with the time above each threshold given. */
export function dutyCsv(duty: DutyPart[], above?: Record<string, number | null>): string {
  const rows: (string | number)[][] = [["part", "quantity", "unit", "max", "min", "mean", "rms", "time_above_s"]];
  for (const d of duty)
    for (const r of d.rows) rows.push([d.label, r.quantity, r.unit, r.max, r.min, r.mean, r.rms, above?.[`${d.elementId}:${r.quantity}`] ?? ""]);
  return csv(rows);
}

/** Each part's RMS power and current and its peak power as study KPIs
 *  (STU-08): "E-Motor — RMS Shaft power". */
export function dutyKpis(result: SimResult): { label: string; value: number; unit: string }[] {
  const out: { label: string; value: number; unit: string }[] = [];
  for (const d of result.duty ?? [])
    for (const r of d.rows) {
      if (r.quantity === "Losses" || (r.unit !== "kW" && r.unit !== "A")) continue;
      out.push({ label: `${d.label} — RMS ${r.quantity}`, value: r.rms, unit: r.unit });
      if (r.unit === "kW") out.push({ label: `${d.label} — peak ${r.quantity}`, value: Math.max(Math.abs(r.max), Math.abs(r.min)), unit: r.unit });
    }
  return out;
}

/** Seconds a stored channel spent above a threshold: each stored point
 *  stands for the step that ended at it. */
export function timeAbove(ts: { t: number; value: number | null }[], threshold: number): number {
  let s = 0;
  for (let i = 1; i < ts.length; i++) {
    const v = ts[i].value;
    if (v != null && v > threshold) s += ts[i].t - ts[i - 1].t;
  }
  return s;
}

/** The stored channel that holds a duty row's quantity, as port id. */
export const DUTY_CHANNEL: Record<string, Record<string, string>> = {
  "motor.emotor": { "Shaft power": "sig_mech_power", "Electrical power": "sig_elec_power", Torque: "sig_torque", Losses: "sig_losses" },
  "battery.generic": { Power: "sig_power", Current: "sig_current" },
  "engine.combustion": { Power: "sig_power", Torque: "sig_torque", "Fuel rate": "sig_fuel_rate" },
  "fuelcell.stack": { Power: "sig_power", Current: "sig_current" },
  "controller.dcdc": { "Power in": "sig_power_in", "Power out": "sig_power_out", Losses: "sig_losses" },
};

export function downloadText(text: string, name: string, type = "text/csv") {
  const url = URL.createObjectURL(new Blob([text], { type }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
