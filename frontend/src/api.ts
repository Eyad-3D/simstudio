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

export async function fetchDemoProject(): Promise<{
  project: StoredProject;
  offline: boolean;
}> {
  try {
    const project = await request<StoredProject>("/projects/bev-car");
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
