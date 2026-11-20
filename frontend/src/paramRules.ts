import type { ParameterDef } from "./types";
import { showNumber } from "./units";

/** A parameter's label without its note: "Charge Capacity (0 = …)" is
 *  "Charge Capacity", as Data Checks name it. */
export const paramName = (def: ParameterDef) => def.label.split(" (")[0];

/** A number parameter's limits in words ("above 0 and at most 100 %"), or
 *  null when it has none; in `unit` when the parameter is shown in another
 *  unit than the library's (exactly, so a limit typed as shown is inside). */
export function limitsText(def: ParameterDef, unit = def.unit): string | null {
  const { minimum: low, exclusiveMinimum: above, maximum: high } = def;
  const n = (v: number) => showNumber(v, def.unit, unit);
  const parts = above != null ? [`above ${n(above)}`] : low != null ? [`at least ${n(low)}`] : [];
  if (high != null) parts.push(`at most ${n(high)}`);
  return parts.length ? `${parts.join(" and ")}${unit === "-" ? "" : ` ${unit}`}` : null;
}

/** Why `value` (in the library's unit) breaks the parameter's limits ("must
 *  be above 0 and at most 100 %"), or null; the words in `unit`. Same rule
 *  and words as ParameterDef.range_problem in backend/app/schemas.py, which
 *  Data Checks use; like them, text (NaN here) "is not a number". */
export function rangeProblem(def: ParameterDef, value: number, unit = def.unit): string | null {
  if (!Number.isFinite(value)) return "is not a number";
  const { minimum: low, exclusiveMinimum: above, maximum: high } = def;
  if ((low == null || value >= low) && (above == null || value > above) && (high == null || value <= high))
    return null;
  return `must be ${limitsText(def, unit)}`;
}
