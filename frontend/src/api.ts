// REST client for the LightSim backend. Every call has a bundled-data
// fallback so the UI stays usable when the FastAPI service is not running
// (the fallback is flagged to the caller so it can surface a warning).

import type {
  ComponentDef,
  DataCheck,
  ParamValue,
  Project,
  SimMessage,
  SimResult,
  SimRun,
  StoredRunInfo,
  Table1D,
  Table2D,
} from "./types";
import fallbackLibrary from "./data/componentLibrary.json";
import fallbackProject from "./data/demoProject.json";

const BASE = "/api";

/** An error answer from the engine; `status` is its HTTP status (409 = the
 *  project file changed on disk since it was loaded). */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

/** A project as read from disk. `revision` names that version of the file (it
 *  is bookkeeping for conflict-checked saves, not part of the project). */
export type StoredProject = Project & { revision?: string };

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    headers: { "Content-Type": "application/json" },
    ...init,
  });
  if (!res.ok) {
    let detail = res.statusText;
    try {
      const body = await res.json();
      detail = body.detail ?? JSON.stringify(body);
    } catch {
      /* keep statusText */
    }
    throw new ApiError(res.status, `${res.status} ${detail}`);
  }
  return res.json() as Promise<T>;
}

interface LibraryPayload {
  components: ComponentDef[];
  /** unitGroup name → display unit; single-sourced from the backend catalog. */
  unitGroups?: Record<string, string>;
}

export async function fetchLibrary(): Promise<{
  components: ComponentDef[];
  unitGroups: Record<string, string>;
  offline: boolean;
}> {
  try {
    const data = await request<LibraryPayload>("/library");
    return { components: data.components, unitGroups: data.unitGroups ?? {}, offline: false };
  } catch {
    const bundled = fallbackLibrary as LibraryPayload;
    return {
      components: bundled.components,
      unitGroups: bundled.unitGroups ?? {},
      offline: true,
    };
  }
}

/** The engine's version, which is the app's (both come from the repo's
 *  VERSION file); null when the engine cannot be reached. */
export async function fetchVersion(): Promise<string | null> {
  try {
    return (await request<{ version?: string }>("/health")).version ?? null;
  } catch {
    return null;
  }
}

/** The example a new user starts on (also bundled as the offline fallback). */
export const DEMO_EXAMPLE = "bev-car";

/** The demo example as the app ships it; the store opens it as a copy. */
export async function fetchDemoProject(): Promise<{
  project: Project;
  offline: boolean;
}> {
  try {
    return { project: await fetchExample(DEMO_EXAMPLE), offline: false };
  } catch {
    return { project: fallbackProject as unknown as Project, offline: true };
  }
}

/** A standard drive cycle bundled with the engine (CON-16). Duration,
 *  distance and top speed are computed from its trace. */
export interface CycleInfo {
  id: string;
  name: string;
  region: string;
  /** its row in docs/data-register.csv */
  register: string;
  /** [name, start s, end s] */
  phases: [string, number, number][];
  duration_s: number;
  distance_km: number;
  vmax_kmh: number;
}

/** A drive cycle with its trace: time (s) and speed (km/h). */
export type CycleTrace = CycleInfo & { t: number[]; v: number[] };

/** The bundled drive cycles; none when the engine cannot be reached. */
export async function listCycles(): Promise<CycleInfo[]> {
  try {
    return await request<CycleInfo[]>("/cycles");
  } catch {
    return [];
  }
}

export function fetchCycle(id: string): Promise<CycleTrace> {
  return request(`/cycles/${encodeURIComponent(id)}`);
}

/** A project or example as the engine lists it. */
export interface ProjectEntry {
  id: string;
  name: string;
  description?: string | null;
  /** for the Start page: when the file was saved (ms since 1970), its
   *  number of parts, and its top diagram's part positions */
  modified?: number;
  elements?: number | null;
  thumb?: [number, number][];
}

/** An example, and whether the user hid it from the Open menu. */
export interface ExampleEntry extends ProjectEntry {
  hidden: boolean;
}

/** The user's saved projects (not the examples). */
export function listProjects(): Promise<ProjectEntry[]> {
  return request("/projects");
}

// ---- examples (shipped with the app, read-only) ----------------------------

export function listExamples(): Promise<ExampleEntry[]> {
  return request("/examples");
}

/** An example as the app ships it. It has no revision: it is not a file the
 *  user can save over, so it is opened as a copy with an id of its own. */
export function fetchExample(id: string): Promise<Project> {
  return request(`/examples/${encodeURIComponent(id)}`);
}

/** Leave an example out of the Open menu until the examples are restored. */
export function hideExample(id: string): Promise<{ hidden: string }> {
  return request(`/examples/${encodeURIComponent(id)}/hide`, { method: "POST" });
}

/** Show every hidden example again; `restored` lists their ids. */
export function restoreExamples(): Promise<{ restored: string[] }> {
  return request("/examples/restore", { method: "POST" });
}

export function fetchProject(id: string): Promise<StoredProject> {
  return request(`/projects/${encodeURIComponent(id)}`);
}

/**
 * Save a project to disk. `base` is the revision the copy was loaded from: the
 * engine refuses the save (ApiError 409) if the file changed since. `null`
 * means the project is not on disk yet (refused if a file with its id
 * exists); leave it out to overwrite whatever is there.
 */
export function saveProject(
  project: Project,
  base?: string | null,
): Promise<{ saved: string; revision?: string }> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (base) headers["If-Match"] = `"${base}"`;
  else if (base === null) headers["If-None-Match"] = "*";
  return request(`/projects/${encodeURIComponent(project.id)}`, {
    method: "PUT",
    headers,
    body: JSON.stringify(project),
  });
}

// ---- backups (earlier versions the engine keeps when a save replaces one) --

/** An earlier version of a project, kept when a save replaced it. `savedAt`
 *  is when that version was saved; `name` and `elements` are null when the
 *  backup cannot be read. */
export interface BackupInfo {
  id: string;
  savedAt: number;
  revision: string;
  bytes: number;
  name: string | null;
  elements: number | null;
}

/** The project's backups, newest first (none for a project never saved over). */
export function listBackups(projectId: string): Promise<BackupInfo[]> {
  return request(`/projects/${encodeURIComponent(projectId)}/backups`);
}

export function fetchBackup(projectId: string, backupId: string): Promise<Project> {
  return request(`/projects/${encodeURIComponent(projectId)}/backups/${encodeURIComponent(backupId)}`);
}

// ---- run history (stored on disk next to the project by the engine) -------

const runsPath = (projectId: string) => `/projects/${encodeURIComponent(projectId)}/runs`;

/** The project's stored runs, newest first (no channel data). */
export function listRuns(projectId: string): Promise<StoredRunInfo[]> {
  return request(runsPath(projectId));
}

export function fetchRun(projectId: string, runId: string): Promise<SimRun> {
  return request(`${runsPath(projectId)}/${encodeURIComponent(runId)}`);
}

export interface StoreRunReply {
  saved: string;
  /** runs the project now has on disk, and their size in bytes */
  stored: number;
  bytes: number;
  /** per-project disk budget; the oldest runs are deleted to stay within it */
  budget: number;
  pruned: string[];
}

export function storeRun(projectId: string, run: SimRun): Promise<StoreRunReply> {
  return request(`${runsPath(projectId)}/${encodeURIComponent(run.id)}`, {
    method: "PUT",
    body: JSON.stringify(run),
  });
}

export function deleteRun(projectId: string, runId: string): Promise<{ deleted: string; stored: number }> {
  return request(`${runsPath(projectId)}/${encodeURIComponent(runId)}`, { method: "DELETE" });
}

export function deleteRuns(projectId: string): Promise<{ deleted: number; stored: number }> {
  return request(runsPath(projectId), { method: "DELETE" });
}

export function validateProject(project: Project): Promise<DataCheck[]> {
  return request("/validate", {
    method: "POST",
    body: JSON.stringify({ project }),
  });
}

export function runSimulation(project: Project, caseId: string): Promise<SimResult> {
  return request("/simulate", {
    method: "POST",
    body: JSON.stringify({ project, caseId }),
  });
}

// ---- live simulation over WebSocket ----------------------------------------

export interface StepEvent {
  t: number;
  pct: number;
  values: Record<string, number>; // "elementId:portId" → value
}

export interface LiveRunCallbacks {
  onStep?: (ev: StepEvent) => void;
  onMessage?: (m: SimMessage) => void;
}

export interface LiveRunHandle {
  /** Push a live parameter change into the running simulation. */
  setParam: (elementId: string, key: string, value: ParamValue) => void;
  /** Ask the solver to stop; the final (partial) result still arrives. */
  cancel: () => void;
  /** Resolves with the final SimResult (also on cancel), rejects on transport failure. */
  done: Promise<SimResult>;
}

export function runSimulationLive(
  project: Project,
  caseId: string,
  callbacks: LiveRunCallbacks = {},
): LiveRunHandle {
  const proto = window.location.protocol === "https:" ? "wss" : "ws";
  const ws = new WebSocket(`${proto}://${window.location.host}${BASE}/simulate/run`);
  let settled = false;
  let resolveDone!: (r: SimResult) => void;
  let rejectDone!: (e: Error) => void;
  const done = new Promise<SimResult>((resolve, reject) => {
    resolveDone = resolve;
    rejectDone = reject;
  });

  ws.onopen = () => ws.send(JSON.stringify({ type: "start", project, caseId }));
  ws.onmessage = (raw) => {
    let msg: Record<string, unknown>;
    try {
      msg = JSON.parse(raw.data as string);
    } catch {
      return;
    }
    switch (msg.type) {
      case "step":
        callbacks.onStep?.(msg as unknown as StepEvent);
        break;
      case "message":
        callbacks.onMessage?.({ level: msg.level, text: msg.text } as SimMessage);
        break;
      case "done":
        settled = true;
        resolveDone(msg.result as SimResult);
        ws.close();
        break;
      case "error":
        settled = true;
        rejectDone(new Error(String(msg.detail ?? "simulation error")));
        ws.close();
        break;
    }
  };
  ws.onerror = () => {
    if (!settled) {
      settled = true;
      rejectDone(new Error("Live simulation connection failed — is the backend running?"));
    }
  };
  ws.onclose = () => {
    if (!settled) {
      settled = true;
      rejectDone(new Error("Live simulation connection closed unexpectedly."));
    }
  };

  return {
    setParam: (elementId, key, value) => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: "set_param", elementId, key, value }));
      }
    },
    cancel: () => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: "cancel" }));
      }
    },
    done,
  };
}

// ---- files in and out (STD-09, STD-10, STD-36) --------------------------------

/** Save `blob` as a download named `name`. */
export function downloadBlob(blob: Blob, name: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

/** The file name a download answer gives in Content-Disposition, else `fallback`. */
function fileNameOf(res: Response, fallback: string): string {
  const cd = res.headers.get("Content-Disposition") ?? "";
  const star = /filename\*=UTF-8''([^;]+)/i.exec(cd);
  if (star) {
    try {
      return decodeURIComponent(star[1]);
    } catch {
      /* fall through */
    }
  }
  return /filename="([^"]+)"/i.exec(cd)?.[1] ?? fallback;
}

async function fetchFile(path: string, init?: RequestInit, fallback = "lightsim"): Promise<{ blob: Blob; name: string }> {
  const res = await fetch(`${BASE}${path}`, { headers: { "Content-Type": "application/json" }, ...init });
  if (!res.ok) {
    let detail = res.statusText;
    try {
      detail = (await res.json()).detail ?? detail;
    } catch {
      /* keep statusText */
    }
    throw new ApiError(res.status, `${res.status} ${detail}`);
  }
  return { blob: await res.blob(), name: fileNameOf(res, fallback) };
}

/** A run as a MATLAB .mat file (a struct per part, plus `meta` with the
 *  units and run details), all channels as CSV, or its run card (JSON). */
export function exportRunFile(run: SimRun, format: "mat" | "csv" | "json"): Promise<{ blob: Blob; name: string }> {
  return fetchFile(`/export/run?format=${format}`, { method: "POST", body: JSON.stringify(run) }, `run.${format}`);
}

/** The file's bytes as base64, for the import routes. */
export async function fileToBase64(file: Blob): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

/** How the engine read one column, axis or the values of an imported table. */
export interface ImportUnit {
  /** the unit the numbers were read in, and the parameter's */
  used: string;
  target: string;
  written: string | null;
  how: "header" | "chosen" | "assumed" | "guessed";
  options: string[];
  question: string | null;
}

export interface ImportProblem {
  text: string;
  row?: number;
  cell?: string;
}

/** What the engine found in a file for a table, map or profile (STD-10). */
export interface TableImport {
  kind: "table1d" | "table2d" | "profile";
  ok: boolean;
  /** ready to store: {x: y}, {outer: {inner: y}} or "t:v; …" */
  value: Table1D | Table2D | string | null;
  points: number;
  range: string;
  sheet: string;
  sheets: { name: string; rows: number; cols: number }[];
  /** the first cells of the sheet, as read */
  cells: (number | string | null)[][];
  columns: { index: number; letter: string; header: string; name: string; unit: string | null }[];
  xColumn: number | null;
  yColumn: number | null;
  transpose: boolean;
  units: Record<string, ImportUnit>;
  axes: { name: string; unit: string }[];
  valueName: string;
  valueUnit: string;
  notes: string[];
  errors: ImportProblem[];
  warnings: ImportProblem[];
  /** [[x, y], …] or a map's {cols, rows, values} in the parameter's units */
  preview: [number, number][] | { cols: number[]; rows: number[]; values: number[][] } | null;
  target: string;
}

export interface TableImportOptions {
  sheet?: string;
  range?: string;
  xColumn?: number;
  yColumn?: number;
  transpose?: boolean;
  units?: Record<string, string>;
}

export function importTableFile(
  file: { name: string; data: string },
  target: { componentDefId: string; paramKey: string; mode?: string },
  options: TableImportOptions = {},
): Promise<TableImport> {
  return request("/import/table", {
    method: "POST",
    body: JSON.stringify({ filename: file.name, data: file.data, ...target, ...options }),
  });
}

/** Every parameter of the project as one .xlsx workbook or CSV sheet (STD-36). */
export function exportParameterSheet(project: Project, format: "xlsx" | "csv"): Promise<{ blob: Blob; name: string }> {
  return fetchFile("/params/export", { method: "POST", body: JSON.stringify({ project, format }) }, `parameters.${format}`);
}

/** The Formula Student example's parameter sheet, as a template. */
export function fetchParameterTemplate(): Promise<{ blob: Blob; name: string }> {
  return fetchFile("/params/template", undefined, "LightSim Formula Student parameter template.xlsx");
}

export interface ParameterChange {
  elementId: string;
  element: string;
  key: string;
  parameter: string;
  unit: string;
  old: ParamValue;
  new: ParamValue;
  row?: number;
  sheet?: string;
}

export interface ParameterSheetImport {
  ok: boolean;
  changes: ParameterChange[];
  errors: { text: string; row?: number | null; sheet?: string }[];
  warnings: { text: string; row?: number | null; sheet?: string }[];
  rows: number;
  unchanged: number;
}

/** The changes a parameter sheet would make; nothing is changed yet. */
export function importParameterSheet(project: Project, file: { name: string; data: string }): Promise<ParameterSheetImport> {
  return request("/params/import", {
    method: "POST",
    body: JSON.stringify({ project, filename: file.name, data: file.data }),
  });
}
