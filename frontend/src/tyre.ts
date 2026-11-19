// A tyre from its size code and load index (MOD-48). The same rules as
// backend/app/tyre.py, which Data Checks use: keep the two in step (the data
// is backend/app/library/tyres.json, copied here by scripts/sync-data.mjs).
//
// ISO metric codes ("205/55 R16 91V", "P205/55R16", "225/45ZR17 94W XL",
// "195/75 R16C 107/105R") and flotation codes ("20.5x7.0-13", inches). The
// unloaded radius is rim ÷ 2 + width × aspect ratio (or the flotation
// diameter ÷ 2); the load index gives the most a tyre may carry; with it,
// Rill's "engineer's guess" for a passenger-car tyre as Project Chrono's
// TMeasy tyre implements it (GuessPassCar70Par, BSD-3-Clause; see
// THIRD-PARTY-NOTICES.txt) estimates its slip stiffness and friction.

import tyres from "./data/tyres.json";

const GRAVITY = 9.81;
const INCH = 0.0254;

/** Load index → maximum load per tyre, kg (0-279): the ISO 4000-1 / ETRTO
 *  table, as in Chrono's ChTMeasyTire::GetTireMaxLoad. */
export const LOAD_INDEX_KG: readonly number[] = tyres.load_index_kg;

// TMeasy passenger-car guess: at the nominal load pn (half the load-index
// capacity), slip stiffness ÷ pn and peak forces ÷ pn; at 2 pn, peak ÷ 2 pn
const TM = tyres.tmeasy_passenger_car;

/** EU tyre label (Regulation (EU) 2020/740, C1 tyres): the rolling
 *  resistance LightSim takes for each fuel-efficiency class, N/kN. */
export const LABEL_RRC: Record<string, number> = tyres.eu_label_rrc_n_per_kn;

const METRIC =
  /^(P|LT|ST|T)?\s*(\d{3})\s*\/\s*(\d{2,3})\s*(?:Z?R|-|D|B|ZR)\s*(\d{2}(?:\.\d)?)\s*(C)?(?:\s*(\d{2,3})(?:\s*\/\s*\d{2,3})?\s*\(?([A-Z]\d?)?\)?)?(?:\s*(?:XL|RF|EXTRA\s*LOAD|REINF))?\s*$/i;
const FLOTATION = /^(\d{2}(?:\.\d)?)\s*[xX×]\s*(\d{1,2}(?:\.\d{1,2})?)\s*-\s*(\d{2})\b.*$/;

export interface TyreSpec {
  code: string;
  widthM: number;
  unloadedRadiusM: number;
  rimM: number;
  loadIndex: number | null;
  speedSymbol: string | null;
}

/** The tyre a size code describes, or null when it is not one. */
export function parseTyreCode(code: string): TyreSpec | null {
  const text = code.trim().split(/\s+/).join(" ");
  let m = METRIC.exec(text);
  if (m) {
    const width = Number(m[2]) / 1000;
    const aspect = Number(m[3]) / 100;
    const rim = Number(m[4]) * INCH;
    let li: number | null = m[6] ? Number(m[6]) : null;
    if (li !== null && li >= LOAD_INDEX_KG.length) li = null;
    return {
      code: text, widthM: width, unloadedRadiusM: rim / 2 + width * aspect, rimM: rim,
      loadIndex: li, speedSymbol: m[7] ? m[7].toUpperCase() : null,
    };
  }
  m = FLOTATION.exec(text);
  if (m) {
    return {
      code: text, widthM: Number(m[2]) * INCH, unloadedRadiusM: (Number(m[1]) * INCH) / 2,
      rimM: Number(m[3]) * INCH, loadIndex: null, speedSymbol: null,
    };
  }
  return null;
}

/** The most one tyre may carry by its load index, kg (null without one). */
export function maxLoadKg(spec: TyreSpec): number | null {
  return spec.loadIndex === null ? null : LOAD_INDEX_KG[spec.loadIndex];
}

const round = (x: number, digits: number) => Number(x.toFixed(digits));

/** A Wheel's parameter values from a tyre: its rolling radius and, with a
 *  load index, the TMeasy estimates about its nominal load. */
export function tyreValues(spec: TyreSpec, rollingRadiusFactor = 0.97): Record<string, number> {
  const out: Record<string, number> = { radius_m: round(spec.unloadedRadiusM * rollingRadiusFactor, 4) };
  const kg = maxLoadKg(spec);
  if (kg !== null) {
    const pn = 0.5 * kg * GRAVITY;
    out.slip_stiffness = TM.dfx0_pn;
    out.mu = TM.fxm_pn;
    out.mu_lateral = TM.fym_pn;
    out.mu_nominal_load_N = round(pn, 1);
    out.mu_load_sensitivity_per_kN = round((TM.fxm_p2n - TM.fxm_pn) / (pn / 1000), 5);
  }
  return out;
}

/** What a code means, in one line for the Wheel's form. */
export function describeTyre(spec: TyreSpec, rollingRadiusFactor = 0.97): string {
  const mm = (m: number) => (m * 1000).toFixed(1);
  const kg = maxLoadKg(spec);
  const load = kg === null ? "" : `, carries up to ${kg.toLocaleString("en")} kg (load index ${spec.loadIndex})`;
  return (
    `${spec.code}: unloaded radius ${mm(spec.unloadedRadiusM)} mm, rolls on ` +
    `${mm(spec.unloadedRadiusM * rollingRadiusFactor)} mm${load}.`
  );
}
