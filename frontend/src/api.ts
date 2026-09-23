// REST client for the SimStudio backend. Every call has a bundled-data
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
} from "./types";
import fallbackLibrary from "./data/componentLibrary.json";
import fallbackProject from "./data/demoProject.json";

const BASE = "/api";

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
    throw new Error(`${res.status} ${detail}`);
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

export async function fetchDemoProject(): Promise<{
  project: Project;
  offline: boolean;
}> {
  try {
    const project = await request<Project>("/projects/bev-car");
    return { project, offline: false };
  } catch {
    return { project: fallbackProject as unknown as Project, offline: true };
  }
}

export function listProjects(): Promise<
  { id: string; name: string; description?: string | null }[]
> {
  return request("/projects");
}

export function fetchProject(id: string): Promise<Project> {
  return request(`/projects/${encodeURIComponent(id)}`);
}

export function saveProject(project: Project): Promise<{ saved: string }> {
  return request(`/projects/${encodeURIComponent(project.id)}`, {
    method: "PUT",
    body: JSON.stringify(project),
  });
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
