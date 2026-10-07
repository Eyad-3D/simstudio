// FMU parts (STD-01): import an FMU file, read what it is, allow it to run,
// and turn the variables the user ticks into the block's signal pins.
// The engine does the reading (app/fmu); nothing here runs the FMU.

import { confirmDialog } from "./dialog";
import { useProjectStore } from "./store/projectStore";
import type { ElementInstance, PortDef } from "./types";

export const FMU_DEF_ID = "signal.fmu";
/** Parameter keys holding a start value the user set: `start:<variable>`. */
export const START_PREFIX = "start:";
/** The block's own parameters that the FMU panel shows instead of the table. */
export const FMU_FILE_KEYS = ["fmu_path", "fmu_sha256", "fmu_name"];

export interface FmuVariable {
  name: string;
  valueReference: number;
  type: string;
  causality: string;
  variability: string;
  unit: string;
  start: number | null;
  description: string;
  /** the pin it can be, or null (not a number, or a parameter) */
  pin: "input" | "output" | null;
  /** a value the user may set before the run */
  settable: boolean;
}

export interface FmuInfo {
  ok: boolean;
  problems: string[];
  warnings?: string[];
  fmiVersion?: string;
  kinds?: string[];
  modelName?: string;
  description?: string;
  generationTool?: string;
  defaultStepSize?: number | null;
  fmpyMissing?: boolean;
  platform?: {
    badge: string;
    runsHere: boolean;
    thisOs: string;
    operatingSystems: string[];
    missingOperatingSystems: string[];
    hasSources: boolean;
  };
  variables: FmuVariable[];
}

export interface FmuFile {
  found: boolean;
  problem: string;
  sha256: string;
  path: string;
  name: string;
  allowed: boolean;
  info: FmuInfo;
}

async function post<T>(path: string, body: BodyInit, contentType: string): Promise<T> {
  const res = await fetch(`/api${path}`, { method: "POST", headers: { "Content-Type": contentType }, body });
  if (!res.ok) {
    let detail = res.statusText;
    try {
      detail = (await res.json()).detail ?? detail;
    } catch {
      /* keep statusText */
    }
    throw new Error(detail);
  }
  return res.json() as Promise<T>;
}

export function describeFmu(params: Record<string, unknown>): Promise<FmuFile> {
  return post<FmuFile>(
    "/fmus/describe",
    JSON.stringify({
      fmuPath: String(params.fmu_path ?? ""),
      fmuSha256: String(params.fmu_sha256 ?? ""),
      fmuName: String(params.fmu_name ?? ""),
    }),
    "application/json",
  );
}

export function allowFmu(sha256: string, name: string): Promise<unknown> {
  return post(`/fmus/${encodeURIComponent(sha256)}/allow?name=${encodeURIComponent(name)}`, "", "application/json");
}

/** The question asked once per FMU before it may run on this computer. */
export function askToAllow(name: string): Promise<boolean> {
  return confirmDialog({
    title: `Allow ${name} to run?`,
    message:
      "An FMU contains compiled code from another tool or company. LightSim runs it in a separate, " +
      "locked-down process, but allow it only if you trust where it came from. You are asked once " +
      "per FMU on this computer; a changed FMU is asked about again.",
    confirmLabel: "Allow it to run",
    cancelLabel: "Not now",
  });
}

/** Keep a file the user chose or dropped, asking first whether it may run. */
export async function importFmuFile(file: File): Promise<FmuFile> {
  if (!file.name.toLowerCase().endsWith(".fmu")) throw new Error(`${file.name} is not an FMU (.fmu) file.`);
  const allow = await askToAllow(file.name);
  return post<FmuFile>(
    `/fmus?name=${encodeURIComponent(file.name)}&allow=${allow}`,
    file,
    "application/octet-stream",
  );
}

/** A port id for an FMU variable: letters, digits and underscores, unique. */
function portIdFor(name: string, taken: Set<string>): string {
  let id = "fmu_" + (name.replace(/[^A-Za-z0-9]+/g, "_").replace(/^_+|_+$/g, "") || "var");
  while (taken.has(id)) id += "_";
  taken.add(id);
  return id;
}

/** The block's pins for a set of ticked variables. A variable that already
 *  has a pin keeps its id, so its wires stay. */
export function pinsFor(element: ElementInstance, variables: FmuVariable[], ticked: Set<string>): PortDef[] {
  const old = new Map((element.dynamicPorts ?? []).map((p) => [p.name, p]));
  const taken = new Set<string>();
  for (const v of variables) if (ticked.has(v.name) && old.has(v.name)) taken.add(old.get(v.name)!.id);
  const out: PortDef[] = [];
  for (const v of variables) {
    if (!ticked.has(v.name) || !v.pin) continue;
    const kept = old.get(v.name);
    out.push({
      id: kept && kept.direction === v.pin ? kept.id : portIdFor(v.name, taken),
      name: v.name,
      direction: v.pin,
      kind: "signal",
      unitGroup: "No Unit",
    });
  }
  return out;
}

/** Point the block at an imported FMU. A new block gets the FMU's inputs and
 *  outputs as pins and the model's name; one that had pins keeps those the
 *  new FMU still has. */
export function applyFmuToElement(elementId: string, file: FmuFile): void {
  const st = useProjectStore.getState();
  const element = st.project?.systems.flatMap((s) => s.elements).find((e) => e.id === elementId);
  if (!element) return;
  const first = !element.parameterOverrides.fmu_sha256;
  st.setParameters(elementId, { fmu_path: file.path, fmu_sha256: file.sha256, fmu_name: file.name });
  const variables = file.info.variables ?? [];
  const ticked = first
    ? new Set(variables.filter((v) => v.causality === "input" || v.causality === "output").map((v) => v.name))
    : new Set((element.dynamicPorts ?? []).map((p) => p.name));
  const fresh = useProjectStore.getState().project?.systems.flatMap((s) => s.elements).find((e) => e.id === elementId);
  if (fresh) st.setDynamicPorts(elementId, pinsFor(fresh, variables, ticked));
  const def = st.libraryById[FMU_DEF_ID];
  if (first && file.info.modelName && def && element.label.startsWith(def.name)) {
    st.renameElement(elementId, file.info.modelName);
  }
}

/** A dropped .fmu file: a new FMU block at `position`, set up from the file. */
export async function dropFmuFile(file: File, position: { x: number; y: number }): Promise<void> {
  const st = useProjectStore.getState();
  try {
    const imported = await importFmuFile(file);
    st.addElement(FMU_DEF_ID, position);
    const id = useProjectStore.getState().selectedElementId;
    if (id) applyFmuToElement(id, imported);
    if (!imported.info.ok) st.log("warning", `${file.name}: ${imported.info.problems.join(" ")}`);
  } catch (e) {
    st.log("error", `Could not import ${file.name}: ${(e as Error).message}`);
  }
}
