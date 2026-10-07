// REST client for the LightSim backend. Every call has a bundled-data
// fallback so the UI stays usable when the FastAPI service is not running
// (the fallback is flagged to the caller so it can surface a warning).

import type {
  ComponentDef,
  DataCheck,
  ParamValue,
  RunSources,
  Project,
  SimMessage,
  SimResult,
  SimRun,
  StoredRunInfo,
  Table1D,
  Table2D,
  Study,
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

/** A project as read from disk, with bookkeeping that is not part of the
 *  project: `revision` names that version of the file (for conflict-checked
 *  saves); `filePath` is where a .lightsim file outside the projects folder
 *  is (PLT-33); `upgradedFrom` is the older file format it was upgraded
 *  from; `readOnly` says why it must not be saved over (a file from a newer
 *  LightSim, PLT-07). */
export type StoredProject = Project & {
  revision?: string;
  filePath?: string;
  upgradedFrom?: number;
  readOnly?: string;
};

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
  /** the document it comes from, with any credit its terms ask for (CON-31) */
  source?: string;
  /** why LightSim may ship it: EU-2011/833, US-17USC105, Apache-2.0, ... */
  reuse?: string;
  /** who it is for, in a sentence */
  note?: string;
  /** it carries a road grade a Road Profile can take */
  grade?: boolean;
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

/** The revision of the project's file on disk now (null: no file). */
export async function fetchRevision(projectId: string): Promise<string | null> {
  return (await request<{ revision: string | null }>(`/projects/${encodeURIComponent(projectId)}/revision`)).revision;
}

/** A project file read by the engine and upgraded to the current format. */
export interface UpgradeReply {
  project: Project;
  /** studies an upgrade took out of the file, to keep with the runs */
  studies: Study[];
  upgradedFrom: number | null;
  readOnly: string | null;
}

/** An imported file's JSON in the current format (PLT-07). */
export function upgradeProject(raw: unknown): Promise<UpgradeReply> {
  return request("/projects/upgrade", { method: "POST", body: JSON.stringify(raw) });
}

// ---- .lightsim files anywhere (PLT-33) ------------------------------------

/** A .lightsim file the user opened or saved (Recent files). */
export interface RecentFile extends ProjectEntry {
  path: string;
  /** when it was last opened, ms since 1970 */
  opened: number;
  /** false when the file is not there any more */
  exists: boolean;
}

export function listFiles(): Promise<RecentFile[]> {
  return request("/files");
}

/** Take a file off Recent files (the file itself stays). */
export function forgetFile(projectId: string): Promise<{ forgotten: string }> {
  return request(`/files/${encodeURIComponent(projectId)}`, { method: "DELETE" });
}

// ---- studies (kept with the runs, PLT-34) ---------------------------------

const studiesPath = (projectId: string) => `/projects/${encodeURIComponent(projectId)}/studies`;

export function listStudies(projectId: string): Promise<Study[]> {
  return request(studiesPath(projectId));
}

export function storeStudy(projectId: string, study: Study): Promise<{ saved: string }> {
  return request(`${studiesPath(projectId)}/${encodeURIComponent(study.id)}`, {
    method: "PUT",
    body: JSON.stringify(study),
  });
}

export function deleteStudy(projectId: string, studyId: string): Promise<{ deleted: string }> {
  return request(`${studiesPath(projectId)}/${encodeURIComponent(studyId)}`, { method: "DELETE" });
}

// ---- attached files (STD-02) ----------------------------------------------

/** A file in the project's resources folder. */
export interface AttachedFile {
  /** "resources/<name>" */
  path: string;
  name: string;
  sha256: string;
  bytes: number;
  /** "fmu", "onnx", "data", "model", "program" or "file" */
  kind: string;
}

const attachmentsPath = (projectId: string) => `/projects/${encodeURIComponent(projectId)}/attachments`;

export function listAttachments(projectId: string): Promise<AttachedFile[]> {
  return request(attachmentsPath(projectId));
}

/** Copy a file into the project's resources folder. */
export function uploadAttachment(projectId: string, file: Blob, name: string): Promise<AttachedFile> {
  return request(`${attachmentsPath(projectId)}?name=${encodeURIComponent(name)}`, {
    method: "POST",
    headers: { "Content-Type": "application/octet-stream" },
    body: file,
  });
}

/** Delete a file from the project's resources folder. */
export function deleteAttachment(projectId: string, name: string): Promise<{ deleted: string }> {
  return request(`${attachmentsPath(projectId)}/${encodeURIComponent(name)}`, { method: "DELETE" });
}

/** The project and its attached files as one zip file. */
export async function exportBundle(project: Project): Promise<Blob> {
  const res = await fetch(`${BASE}/bundle`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ project }),
  });
  if (!res.ok) throw new ApiError(res.status, `${res.status} ${res.statusText}`);
  return res.blob();
}

/** Unpack a bundle: its project (upgraded) with its files attached. */
export function importBundle(zip: Blob): Promise<UpgradeReply> {
  return request("/bundle/import", {
    method: "POST",
    headers: { "Content-Type": "application/zip" },
    body: zip,
  });
}

// ---- trust: code the user agreed to run (STD-02) ---------------------------

export async function isTrusted(fingerprint: string): Promise<boolean> {
  return (await request<{ trusted: boolean }>(`/trust/${fingerprint}`)).trusted;
}

export function trustFingerprint(fingerprint: string): Promise<{ trusted: boolean }> {
  return request(`/trust/${fingerprint}`, { method: "POST" });
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

/** An example's stored reference runs (CON-15). */
export interface StoredReference {
  caseId: string;
  result: SimResult;
  creation: { appVersion: string; date: string; gitCommit?: string | null; note: string };
}

export function fetchExampleReference(id: string): Promise<StoredReference[]> {
  return request(`/examples/${encodeURIComponent(id)}/reference`);
}

/** The data and methods a run of the case rests on (VAL-37). */
export function fetchRunSources(project: Project, caseId: string): Promise<RunSources> {
  return request("/sources", {
    method: "POST",
    body: JSON.stringify({ project, caseId }),
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

// ---- studies on all cores (ENG-05) ------------------------------------------

/** One point of a study as the engine reports it when its run ends. */
export interface StudyPointEvent {
  index: number;
  values: number[];
  status: SimResult["status"] | "not run";
  runId?: string | null;
  incomplete?: string | null;
  /** the run's wall time, s */
  wallS?: number;
  /** runs the run store deleted to keep to its disk budget */
  pruned?: string[];
  summary?: SimResult["summary"];
  /** each part's duty (RES-39) */
  duty?: SimResult["duty"];
}

export interface StudyTotals {
  workers: number;
  /** the whole study's wall time, s */
  wallS: number;
  /** its points' wall times added up, s */
  pointWallS: number;
  speedup: number;
}

export interface StudyRequest {
  project: Project;
  caseId: string;
  points: { overrides: Record<string, Record<string, ParamValue>>; values: number[]; label?: string }[];
  sweepId?: string;
  sweepParam?: string | null;
  sweepUnit?: string | null;
  workers?: number;
}

export interface StudyHandle {
  cancel: () => void;
  /** Resolves when the study ends (also when stopped), rejects on transport failure. */
  done: Promise<StudyTotals>;
}

/** Run a study's points side by side in the engine's worker processes; each
 *  point is stored as a run of the project and reported as it ends. */
export function runStudyLive(
  req: StudyRequest,
  callbacks: { onStarted?: (workers: number) => void; onPoint?: (p: StudyPointEvent) => void } = {},
): StudyHandle {
  const proto = window.location.protocol === "https:" ? "wss" : "ws";
  const ws = new WebSocket(`${proto}://${window.location.host}${BASE}/studies/run`);
  let settled = false;
  let resolveDone!: (t: StudyTotals) => void;
  let rejectDone!: (e: Error) => void;
  const done = new Promise<StudyTotals>((resolve, reject) => {
    resolveDone = resolve;
    rejectDone = reject;
  });
  const fail = (message: string) => {
    if (settled) return;
    settled = true;
    rejectDone(new Error(message));
  };
  ws.onopen = () => ws.send(JSON.stringify({ type: "start", ...req }));
  ws.onmessage = (raw) => {
    let msg: Record<string, unknown>;
    try {
      msg = JSON.parse(raw.data as string);
    } catch {
      return;
    }
    switch (msg.type) {
      case "started":
        callbacks.onStarted?.(Number(msg.workers));
        break;
      case "point":
        callbacks.onPoint?.(msg as unknown as StudyPointEvent);
        break;
      case "done":
        settled = true;
        resolveDone(msg as unknown as StudyTotals);
        ws.close();
        break;
      case "error":
        fail(String(msg.detail ?? "study error"));
        ws.close();
        break;
    }
  };
  ws.onerror = () => fail("Study connection failed — is the backend running?");
  ws.onclose = () => fail("Study connection closed unexpectedly.");
  return {
    cancel: () => {
      if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ type: "cancel" }));
    },
    done,
  };
}

/** A logger or lap simulator layout the lap import knows (STD-35). */
export interface LapLogPreset {
  name: string;
  note: string;
  speedUnit: string;
}

export interface LapLogRequest {
  text: string;
  preset: string;
  columns?: Partial<Record<"time" | "distance" | "speed" | "lap", string | null>>;
  speedUnit?: string | null;
  lap?: number | null;
  repeatToKm?: number;
  driverChangeS?: number;
}

/** A lap read from a file, as a Driving Task profile. */
export interface LapLogResult {
  columns: string[];
  units: Record<string, string>;
  preset: string;
  picked: Record<"time" | "distance" | "speed" | "lap", string | null>;
  laps: { lap: number; span: number; points: number }[];
  lap: number | null;
  duration_s: number;
  distance_m: number;
  source_distance_m: number | null;
  repeated: number;
  profile: string;
  preview: [number, number][];
  warnings: string[];
}

export async function laplogPresets(): Promise<LapLogPreset[]> {
  return request<LapLogPreset[]>("/laplog/presets");
}

export async function readLapLog(req: LapLogRequest): Promise<LapLogResult> {
  return request<LapLogResult>("/laplog/read", { method: "POST", body: JSON.stringify(req) });
}

/** A logged lap for the lap mode calibration (VAL-38). */
export interface LoggedLapIn {
  text: string;
  columns?: Partial<Record<"time" | "distance" | "speed" | "lat_accel" | "power" | "lap", string | null>>;
  lap?: number | null;
  speedUnit?: string;
}

export interface LapPrediction {
  status: string;
  lap_time_log_s: number;
  lap_time_model_s?: number;
  lap_time_error_pct?: number;
  speed_rms_kmh?: number;
  energy_model_kwh?: number;
  energy_log_kwh?: number;
  energy_error_pct?: number;
  messages: string[];
}

export interface CalibrationResult {
  fit: { mu_scale: number; cza: number; rms_kmh: number; evaluations: number };
  calibration_lap: LapPrediction;
  check_lap?: LapPrediction;
}

export async function calibrateLap(
  project: Project,
  calibration: LoggedLapIn,
  check?: LoggedLapIn,
): Promise<CalibrationResult> {
  return request<CalibrationResult>("/laplog/calibrate", {
    method: "POST",
    body: JSON.stringify({ project, calibration, check: check ?? null }),
  });
}

/** One step of the US label estimate: a figure and how it was worked out. */
export interface LabelStep {
  what: string;
  value: number;
  unit: string;
  how: string;
}

/** CON-32: the US window-sticker estimate from UDDS and HWFET runs. */
export interface LabelEstimate {
  notCertified: string;
  electric: boolean;
  modelYearCoefficients: number;
  coefficients: Record<string, number>;
  chargerEfficiency: number | null;
  usableKwh: number | null;
  /** the case run for each cycle (udds, hwfet) */
  cases: Record<string, string>;
  problems: string[];
  figures: Record<string, number>;
  steps: LabelStep[];
}

export function labelEstimate(project: Project, caseId: string | null, modelYear: number): Promise<LabelEstimate> {
  return request("/label-estimate", {
    method: "POST",
    body: JSON.stringify({ project, caseId, modelYear }),
  });
}

/** One figure from the one-click vehicle tests (CON-06). */
export interface VehicleTestRow {
  what: string;
  value: number | null;
  unit: string;
  how: string;
  /** why the figure is missing or what limits it */
  note: string;
}

export function vehicleTests(project: Project, tests: string[]): Promise<{ rows: VehicleTestRow[]; note: string }> {
  return request("/vehicle-tests", { method: "POST", body: JSON.stringify({ project, tests }) });
}

/** A field of a template's form: the value a new project asks for (CON-18). */
export interface TemplateField {
  elementId: string;
  key: string;
  label: string;
  unit: string;
  default: ParamValue;
  minimum?: number | null;
  maximum?: number | null;
  help?: string;
}

/** A vehicle template: a pre-wired model with named slots and a form. */
export interface VehicleTemplate {
  id: string;
  name: string;
  description: string;
  version: number;
  builtin: boolean;
  example?: string | null;
  slots: Record<string, string>;
  form: TemplateField[];
}

export async function listTemplates(): Promise<VehicleTemplate[]> {
  try {
    return await request<VehicleTemplate[]>("/templates");
  } catch {
    return [];
  }
}

export function newFromTemplate(id: string, values: Record<string, ParamValue>, name: string): Promise<Project> {
  return request(`/templates/${encodeURIComponent(id)}/new`, {
    method: "POST",
    body: JSON.stringify({ values, name }),
  });
}

export function saveTemplate(body: {
  project: Project;
  name: string;
  description: string;
  form: TemplateField[];
  slots: Record<string, string>;
}): Promise<VehicleTemplate> {
  return request("/templates", { method: "POST", body: JSON.stringify(body) });
}

export function deleteTemplate(id: string): Promise<unknown> {
  return request(`/templates/${encodeURIComponent(id)}`, { method: "DELETE" });
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
