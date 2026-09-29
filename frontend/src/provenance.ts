// Fingerprints for run snapshots (RES-09): a run records a hash of the model
// it was made with, so two runs of the same model can be told from runs of a
// changed one; and what changed between two runs' snapshots (RES-10).

import type { ComponentDef, ElementInstance, Project, RunSnapshot } from "./types";

/** JSON with every object's keys sorted, so equal values give equal text
 *  whatever order their fields were written in. */
export function canonicalJson(value: unknown): string {
  return JSON.stringify(value, (_key, v: unknown) =>
    v && typeof v === "object" && !Array.isArray(v)
      ? Object.fromEntries(
          Object.keys(v)
            .sort()
            .map((k) => [k, (v as Record<string, unknown>)[k]]),
        )
      : v,
  );
}

/** SHA-256 of the project's canonical JSON, as hex; undefined where the
 *  browser offers no Web Crypto (a page served from neither localhost nor
 *  https). */
export async function modelFingerprint(project: Project): Promise<string | undefined> {
  const subtle = globalThis.crypto?.subtle;
  if (!subtle) return undefined;
  const digest = await subtle.digest("SHA-256", new TextEncoder().encode(canonicalJson(project)));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** One difference between a baseline run's snapshot and a run's. */
export interface ModelChange {
  /** "Vehicle · Vehicle Mass 1,927 → 2,300 kg" */
  text: string;
  /** for the run's name: "Vehicle Mass 2,300 kg"; none for a live edit */
  short?: string;
  /** the part it is about, to show it on the diagram */
  elementId?: string;
}

// (a run's name is stored with it, so its numbers do not follow the locale)
const num = (v: number) => v.toLocaleString("en-US", { maximumFractionDigits: 6 });
const unitOf = (u?: string) => (u && u !== "-" ? ` ${u}` : "");
/** A value short enough to print, else null (tables, scripts, long text). */
function shown(v: unknown): string | null {
  if (typeof v === "number") return num(v);
  if (typeof v === "boolean") return v ? "on" : "off";
  if (typeof v === "string" && v.length <= 24) return v;
  return null;
}

/** Case settings as the case form names them, with their unit and the value
 *  an absent one means. Settings added later show by their field name. */
const CASE_FIELDS: Record<string, [string, string, unknown?]> = {
  duration: ["Duration", " s"],
  timeStep: ["Step", " s"],
  outputEvery: ["Store every", "", 1],
  kind: ["Kind", "", "cycle"],
  endDistance: ["Distance", " m"],
  startLine: ["Start line", " m"],
  referenceTime: ["Reference time", " s"],
};
// the case's identity, its overrides (compared per parameter) and its pacing,
// which changes no result
const NOT_A_SETTING = new Set(["id", "name", "parameterOverrides", "realtimeFactor"]);

/** What changed from `base` to `next`: the case, each parameter as the
 *  solver used it (the case's override, else the part's, else the library
 *  default), parts added or removed, wiring (canvas wires and Data Bus links,
 *  either way round), case settings, and last each run's live edits. Part
 *  renames and layout change no result and are left out. */
export function diffSnapshots(base: RunSnapshot, next: RunSnapshot, lib: Record<string, ComponentDef>): ModelChange[] {
  const out: ModelChange[] = [];
  const [bc, nc] = [base.case, next.case];
  if (bc.name !== nc.name) out.push({ text: `Case ${bc.name} → ${nc.name}`, short: nc.name });
  const partsOf = (s: RunSnapshot) => new Map(s.project.systems.flatMap((sy) => sy.elements).map((e) => [e.id, e]));
  const [bp, np] = [partsOf(base), partsOf(next)];
  const value = (s: RunSnapshot, e: ElementInstance, key: string) =>
    s.case.parameterOverrides?.[e.id]?.[key] ??
    e.parameterOverrides[key] ??
    lib[e.componentDefId]?.parameters.find((p) => p.key === key)?.default;

  for (const [id, e] of np) {
    const old = bp.get(id);
    if (!old) continue;
    for (const p of lib[e.componentDefId]?.parameters ?? []) {
      const [a, b] = [value(base, old, p.key), value(next, e, p.key)];
      // a table's outside-the-data setting changes what it gives too
      if (canonicalJson([a, old.tableOutside?.[p.key]]) === canonicalJson([b, e.tableOutside?.[p.key]])) continue;
      const [sa, sb] = [shown(a), shown(b)];
      const unit = unitOf(p.unit);
      out.push(
        sa !== null && sb !== null
          ? { text: `${e.label} · ${p.label} ${sa} → ${sb}${unit}`, short: `${p.label} ${sb}${unit}`, elementId: id }
          : { text: `${e.label} · ${p.label} edited`, short: `${p.label} edited`, elementId: id },
      );
    }
  }
  for (const [id, e] of np) if (!bp.has(id)) out.push({ text: `Added ${e.label}`, short: `added ${e.label}`, elementId: id });
  for (const [id, e] of bp) if (!np.has(id)) out.push({ text: `Removed ${e.label}`, short: `removed ${e.label}` });

  const links = (s: RunSnapshot, parts: Map<string, ElementInstance>) => {
    const end = (el: string, port: string) => {
      const e = parts.get(el);
      const ports = e ? [...(lib[e.componentDefId]?.ports ?? []), ...(e.dynamicPorts ?? [])] : [];
      return `${e?.label ?? el}.${ports.find((p) => p.id === port)?.name ?? port}`;
    };
    return new Map(
      [
        ...s.project.systems.flatMap((sy) =>
          sy.connections.map((c) => [c.sourceElementId, c.sourcePortId, c.targetElementId, c.targetPortId]),
        ),
        ...s.project.dataBusConnections.map((d) => [d.element1Id, d.port1Id, d.element2Id, d.port2Id]),
      ].map(([e1, p1, e2, p2]) => [[`${e1}:${p1}`, `${e2}:${p2}`].sort().join("|"), `${end(e1, p1)} – ${end(e2, p2)}`]),
    );
  };
  const [bw, nw] = [links(base, bp), links(next, np)];
  for (const [k, t] of nw) if (!bw.has(k)) out.push({ text: `Wired ${t}`, short: "wiring changed" });
  for (const [k, t] of bw) if (!nw.has(k)) out.push({ text: `Unwired ${t}`, short: "wiring changed" });

  for (const k of new Set([...Object.keys(bc), ...Object.keys(nc)])) {
    if (NOT_A_SETTING.has(k)) continue;
    const [label, unit, absent] = CASE_FIELDS[k] ?? [k, ""];
    const a = (bc as unknown as Record<string, unknown>)[k] ?? absent;
    const b = (nc as unknown as Record<string, unknown>)[k] ?? absent;
    if (canonicalJson(a) === canonicalJson(b)) continue;
    const sb = shown(b) ?? "—";
    out.push({ text: `Case · ${label} ${shown(a) ?? "—"} → ${sb}${unit}`, short: `${label} ${sb}${unit}` });
  }

  const edits = (s: RunSnapshot, which: string, parts: Map<string, ElementInstance>) =>
    s.liveEdits.map((le): ModelChange => {
      const e = parts.get(le.elementId);
      const p = e && lib[e.componentDefId]?.parameters.find((x) => x.key === le.key);
      return {
        text:
          `Live edit in ${which} at t ≈ ${num(Math.round(le.t))} s: ${e?.label ?? le.elementId} · ` +
          `${p?.label ?? le.key} = ${shown(le.value) ?? String(le.value)}${unitOf(p?.unit)}`,
        elementId: le.elementId,
      };
    });
  out.push(...edits(base, "the baseline", bp), ...edits(next, "this run", np));
  return out;
}

/** A run's name from what changed since the previous run of its case:
 *  "Vehicle Mass 2,300 kg", or "Vehicle Mass 2,300 kg +2 more"; undefined
 *  when nothing did (it keeps its clock time). */
export function nameFromChanges(changes: ModelChange[]): string | undefined {
  const named = [...new Set(changes.flatMap((c) => (c.short ? [c.short] : [])))];
  if (named.length === 0) return undefined;
  return named.length === 1 ? named[0] : `${named[0]} +${named.length - 1} more`;
}

/** The decimals a stored value was rounded to, read from the value. */
function decimals(v: number): number {
  const [m, e] = String(v).split("e");
  return Math.max(0, (m.split(".")[1]?.length ?? 0) - Number(e ?? 0));
}

/** A summary value's change from the baseline's value: the difference (to
 *  `digits` decimals, the finer of the two values), the % change (null when
 *  the baseline is 0), and `noise` when it is within one step of the stored
 *  rounding, which KNOWN-LIMITS says to treat as no change. */
export function summaryChange(value: number, base: number) {
  // ponytail: the rounding step is read from the two values' digits, so two
  // values that both end in 0 read one digit coarser; the engine could send
  // each SummaryValue's step if that ever misleads
  const digits = Math.max(decimals(value), decimals(base));
  const diff = value - base;
  return {
    diff,
    digits,
    pct: base !== 0 ? (100 * diff) / Math.abs(base) : null,
    noise: Math.abs(diff) <= 10 ** -digits * (1 + 1e-9),
  };
}
