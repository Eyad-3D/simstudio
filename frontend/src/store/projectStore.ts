import { useMemo } from "react";
import { create } from "zustand";
import * as api from "../api";
import { confirmDialog, unsavedChangesDialog } from "../dialog";
import { loadDraft } from "../persist";
import { modelFingerprint } from "../provenance";
import type {
  Channel,
  ComponentDef,
  Connection,
  DataBusConnection,
  DataCheck,
  ElementInstance,
  LiveEdit,
  LogMessage,
  OutsidePolicy,
  ParamValue,
  PortDef,
  PortSide,
  Project,
  RunSnapshot,
  SimResult,
  SimRun,
  StoredRunInfo,
  Study,
  StudyPoint,
  SystemNode,
} from "../types";
import { useUIStore } from "./uiStore";

export function uid(prefix: string): string {
  return `${prefix}-${Math.random().toString(36).slice(2, 9)}`;
}

function now(): string {
  return new Date().toLocaleTimeString([], { hour12: false });
}

/** An example opened as an unsaved copy: the example with an id of its own
 *  (named after it), so saving the copy makes a new project, never writes the
 *  example, and the copy's runs and backups are kept apart from a project of
 *  the example's id (such as a copy an earlier version put in the projects
 *  folder). */
function exampleCopy(example: Project): Project {
  return { ...structuredClone(example), id: uid(example.id.slice(0, 120)) };
}

/** Split a project read from disk into the project and its file revision. */
function fromDisk(stored: api.StoredProject): { project: Project; revision: string | null } {
  const { revision, ...project } = stored;
  return { project, revision: revision ?? null };
}

// Saves run one after another, so a second Save starts from the revision the
// first one returned instead of looking like a conflict with it.
let saveQueue: Promise<void> = Promise.resolve();

/** A snapshot of elements + the wiring wholly contained within them. */
interface ClipboardData {
  elements: ElementInstance[];
  connections: Connection[];
  dataBus: DataBusConnection[];
}

/** Snapshot the given element ids of a system plus the connections and
 *  data-bus wires whose *both* endpoints are in the selection. */
function collectSelection(project: Project, systemId: string, ids: string[]): ClipboardData {
  const sys = project.systems.find((s) => s.id === systemId);
  if (!sys) return { elements: [], connections: [], dataBus: [] };
  const idSet = new Set(ids);
  return {
    elements: sys.elements.filter((e) => idSet.has(e.id)).map((e) => structuredClone(e)),
    connections: sys.connections
      .filter((c) => idSet.has(c.sourceElementId) && idSet.has(c.targetElementId))
      .map((c) => structuredClone(c)),
    dataBus: project.dataBusConnections
      .filter((d) => idSet.has(d.element1Id) && idSet.has(d.element2Id))
      .map((d) => structuredClone(d)),
  };
}

/** Deep-clone a sub-system tree under `newParentId`, returning the new system id. */
function cloneSubsystemTree(draft: Project, srcSysId: string, newParentId: string): string {
  const src = draft.systems.find((s) => s.id === srcSysId);
  const newSysId = uid("sys");
  const newSys: SystemNode = {
    id: newSysId,
    name: src?.name ?? "System",
    parentId: newParentId,
    elements: [],
    connections: [],
  };
  draft.systems.push(newSys);
  if (!src) return newSysId;
  const idMap = new Map<string, string>();
  for (const el of src.elements) {
    const nid = uid("el");
    idMap.set(el.id, nid);
    const clone = structuredClone(el);
    clone.id = nid;
    if (clone.isSubSystem && clone.subSystemId) {
      clone.subSystemId = cloneSubsystemTree(draft, el.subSystemId!, newSysId);
    }
    newSys.elements.push(clone);
  }
  for (const c of src.connections) {
    const a = idMap.get(c.sourceElementId);
    const b = idMap.get(c.targetElementId);
    if (a && b)
      newSys.connections.push({ id: uid("c"), sourceElementId: a, sourcePortId: c.sourcePortId, targetElementId: b, targetPortId: c.targetPortId });
  }
  const srcIds = new Set(src.elements.map((e) => e.id));
  for (const d of [...draft.dataBusConnections]) {
    if (srcIds.has(d.element1Id) && srcIds.has(d.element2Id)) {
      const a = idMap.get(d.element1Id);
      const b = idMap.get(d.element2Id);
      if (a && b)
        draft.dataBusConnections.push({ id: uid("dbc"), element1Id: a, port1Id: d.port1Id, element2Id: b, port2Id: d.port2Id });
    }
  }
  return newSysId;
}

/** Clone a clipboard/selection into `targetSystemId` at an offset, remapping
 *  the internal wiring and deep-cloning any container sub-systems. Returns the
 *  new element ids (in source order). */
function cloneElementsInto(
  draft: Project,
  targetSystemId: string,
  data: ClipboardData,
  offset: { x: number; y: number },
): string[] {
  const system = draft.systems.find((s) => s.id === targetSystemId);
  if (!system) return [];
  const idMap = new Map<string, string>();
  const newIds: string[] = [];
  for (const el of data.elements) {
    const nid = uid("el");
    idMap.set(el.id, nid);
    newIds.push(nid);
    const clone = structuredClone(el);
    clone.id = nid;
    clone.position = { x: el.position.x + offset.x, y: el.position.y + offset.y };
    if (clone.isSubSystem && clone.subSystemId) {
      clone.subSystemId = cloneSubsystemTree(draft, el.subSystemId!, targetSystemId);
    }
    system.elements.push(clone);
  }
  for (const c of data.connections) {
    const a = idMap.get(c.sourceElementId);
    const b = idMap.get(c.targetElementId);
    if (a && b)
      system.connections.push({ id: uid("c"), sourceElementId: a, sourcePortId: c.sourcePortId, targetElementId: b, targetPortId: c.targetPortId });
  }
  for (const d of data.dataBus) {
    const a = idMap.get(d.element1Id);
    const b = idMap.get(d.element2Id);
    if (a && b)
      draft.dataBusConnections.push({ id: uid("dbc"), element1Id: a, port1Id: d.port1Id, element2Id: b, port2Id: d.port2Id });
  }
  return newIds;
}

const HISTORY_LIMIT = 50;
const LIVE_FLUSH_MS = 120;
// Runs held in memory and listed in Results (holds a full sweep family). Every
// finished run is also stored on disk with its project; older ones stay there.
const MAX_RUNS = 20;

/** A parameter sweep: run `caseId` once per value, overriding one element param. */
export interface SweepConfig {
  caseId: string;
  elementId: string;
  paramKey: string;
  values: number[];
}

/** Resolve "elementId:portId" stream keys to channel metadata client-side.
 *  Units come from the library's unitGroups map (single-sourced backend data). */
function channelMetaResolver(
  project: Project,
  libraryById: Record<string, ComponentDef>,
  unitGroups: Record<string, string>,
) {
  const elements = new Map(project.systems.flatMap((s) => s.elements.map((e) => [e.id, e] as const)));
  return (key: string): Omit<Channel, "timeSeries"> | null => {
    const sep = key.indexOf(":");
    if (sep < 0) return null;
    const elementId = key.slice(0, sep);
    const portId = key.slice(sep + 1);
    const el = elements.get(elementId);
    const port = el && portsOf(el, libraryById).find((p) => p.id === portId);
    if (!el || !port) return null;
    const unit = unitGroups[port.unitGroup ?? "No Unit"] ?? "-";
    return { elementId, portId, label: `${el.label} · ${port.name}`, unit };
  };
}

/** An element's ports: its library ones plus any it carries itself (Monitor
 *  and Script ports are added per element in Properties). */
export function portsOf(el: ElementInstance, libraryById: Record<string, ComponentDef>): PortDef[] {
  return [...(libraryById[el.componentDefId]?.ports ?? []), ...(el.dynamicPorts ?? [])];
}

/** The signal output already feeding input `elementId.portId` (through a data
 *  bus link or a canvas wire), as "Element.Port", or null. */
function signalSourceOf(
  project: Project,
  libraryById: Record<string, ComponentDef>,
  elementId: string,
  portId: string,
): string | null {
  const elements = new Map(project.systems.flatMap((s) => s.elements.map((e) => [e.id, e] as const)));
  const output = (elId: string, pId: string) => {
    const el = elements.get(elId);
    const port = el && portsOf(el, libraryById).find((p) => p.id === pId);
    return el && port?.direction === "output" ? `${el.label}.${port.name}` : null;
  };
  const links = [
    ...project.dataBusConnections.map((d) => [d.element1Id, d.port1Id, d.element2Id, d.port2Id]),
    ...project.systems.flatMap((s) =>
      s.connections.map((c) => [c.sourceElementId, c.sourcePortId, c.targetElementId, c.targetPortId]),
    ),
  ];
  for (const [e1, p1, e2, p2] of links) {
    const src =
      e1 === elementId && p1 === portId ? output(e2, p2) : e2 === elementId && p2 === portId ? output(e1, p1) : null;
    if (src) return src;
  }
  return null;
}

// handle for the in-flight live run (not in reactive state on purpose)
let activeRun: api.LiveRunHandle | null = null;
// live parameter edits sent to the in-flight run, for its snapshot
let liveLog: { runId: string; edits: LiveEdit[] } | null = null;
// set by stopRun so an in-flight parameter sweep aborts after the current point
let sweepAborted = false;
// bumped per run-history load, so an answer for an earlier load is dropped
let runHistorySeq = 0;

/** The run Results opens on: the newest complete run that is not a sweep
 *  point, or the newest run when there is none (runs are newest first). */
function mainRunOf<R extends Pick<SimRun, "status" | "incomplete" | "sweepId">>(runs: R[]): R | undefined {
  return runs.find((r) => !r.incomplete && r.status !== "failed" && !r.sweepId) ?? runs[0];
}

/** A study's table row for a point that ran: its status and summary values. */
function studyPoint(run: SimRun, values: number[]): StudyPoint {
  const notValid = run.result.summary.filter((v) => v.notValid);
  return {
    values,
    runId: run.id,
    status: run.status === "running" ? "failed" : run.status,
    ...(run.incomplete ? { incomplete: run.incomplete } : {}),
    // (a value JSON cannot carry would make the project unsavable)
    kpis: Object.fromEntries(run.result.summary.filter((v) => Number.isFinite(v.value)).map((v) => [v.label, v.value])),
    ...(notValid.length ? { notValid: Object.fromEntries(notValid.map((v) => [v.label, v.notValid!])) } : {}),
  };
}

/** Undo and redo step through edits, not studies: a project from the
 *  history gets the studies the project has now. */
function keepStudies(target: Project, current: Project): Project {
  return target.studies === current.studies ? target : { ...target, studies: current.studies };
}

/** Why a finished run is not a complete result, or undefined when it is.
 *  A stop only counts when the solver says it cut the run short (status
 *  "cancelled"; a stop pressed as the run ends leaves a complete result). */
function incompleteReason(result: SimResult): string | undefined {
  if (result.status === "failed") return "failed";
  if (result.status !== "cancelled") return undefined;
  const at = result.messages.find((m) => /cancel/i.test(m.text))?.text.match(/t = ([^ ]+ s)/);
  return at ? `stopped at t = ${at[1]}` : "stopped";
}

interface ProjectState {
  library: ComponentDef[];
  libraryById: Record<string, ComponentDef>;
  /** unitGroup name → display unit (from the backend catalog). */
  unitGroups: Record<string, string>;
  offline: boolean;
  loaded: boolean;
  /** the app's version as the engine reports it (recorded with each run) */
  appVersion: string | null;

  project: Project | null;
  /** Revision of the project file the open copy was loaded from or last saved
   *  as; a save is refused if the file changed since. null: not on disk yet. */
  revision: string | null;
  /** The example the open project is an unsaved copy of (its id), else null.
   *  Save keeps the copy as a new project; the example is never written. */
  exampleId: string | null;
  activeSystemId: string | null;
  selectedElementId: string | null;
  dirty: boolean;

  /** in-memory element clipboard (copy/paste); not persisted or in undo history */
  clipboard: ClipboardData | null;
  /** ids the canvas should select next render (e.g. freshly pasted elements) */
  pendingCanvasSelection: string[] | null;

  past: Project[];
  future: Project[];

  messages: LogMessage[];
  dataChecks: DataCheck[] | null;
  /** the project's newest runs (newest first, at most MAX_RUNS) */
  runs: SimRun[];
  /** how many runs of the open project are stored on disk */
  storedRunCount: number;
  /** true while the open project's stored runs are being read */
  runsLoading: boolean;
  activeCaseId: string | null;
  /** run shown in Results (primary); additional runs overlaid on the chart */
  activeRunId: string | null;
  overlayRunIds: string[];
  running: boolean;
  checking: boolean;
  /** latest values per "elementId:portId" while (and after) a live run */
  liveValues: Record<string, number>;
  liveT: number;
  livePct: number;

  // lifecycle
  init: () => Promise<void>;
  log: (level: LogMessage["level"], text: string) => void;
  clearMessages: () => void;

  // navigation & selection
  select: (elementId: string | null) => void;
  setActiveSystem: (systemId: string) => void;

  // topology editing
  addElement: (defId: string, position: { x: number; y: number }) => void;
  moveElement: (id: string, position: { x: number; y: number }) => void;
  resizeElement: (id: string, size: { width: number; height: number }) => void;
  beginHistory: () => void;
  /** Delete parts (with every wire on them) and the given wires, as one undo step. */
  removeElements: (ids: string[], connectionIds?: string[]) => void;
  copyElements: (ids: string[]) => void;
  duplicateElements: (ids: string[]) => void;
  pasteClipboard: (position?: { x: number; y: number }) => void;
  clearPendingSelection: () => void;
  renameElement: (id: string, label: string) => void;
  setParameter: (elementId: string, key: string, value: ParamValue) => void;
  /** A table's outside-the-data settings, one per axis (applies on the next run). */
  setTableOutside: (elementId: string, key: string, policies: OutsidePolicy[]) => void;
  setDynamicPorts: (elementId: string, ports: PortDef[]) => void;
  setPortSide: (elementId: string, portId: string, side: PortSide) => void;
  setPortPlacement: (
    elementId: string,
    portId: string,
    side: PortSide,
    offset: number,
  ) => void;
  /** Wire two ports; with `replaceId`, the new wire takes that one's place in
   *  the same undo step, and the old wire stays if the new one is refused. */
  addConnection: (
    sourceElementId: string,
    sourcePortId: string,
    targetElementId: string,
    targetPortId: string,
    replaceId?: string,
  ) => void;
  addDataBus: (el1: string, p1: string, el2: string, p2: string) => void;
  removeDataBus: (id: string) => void;
  renameSystem: (systemId: string, name: string) => void;

  undo: () => void;
  redo: () => void;

  // project lifecycle
  newProject: () => void;
  openProject: (id: string) => Promise<void>;
  /** Open an example as an unsaved copy with an id of its own. */
  openExample: (id: string) => Promise<void>;
  /** Leave an example out of the Open menu; resolves false when that failed. */
  hideExample: (id: string, name: string) => Promise<boolean>;
  /** Show every hidden example in the Open menu again. */
  restoreExamples: () => Promise<void>;
  saveRemote: () => Promise<void>;
  exportProject: () => void;
  importProject: (json: string) => void;
  /** Open `project` as a new, unsaved project named `name`: it gets an id of
   *  its own, so saving it never replaces another project's file. */
  openAsCopy: (project: Project, name: string, note: string) => void;

  // cases & simulation
  setActiveCase: (id: string) => void;
  setCaseField: (
    caseId: string,
    patch: Partial<{
      name: string;
      duration: number;
      timeStep: number;
      outputEvery: number;
      realtimeFactor: number;
      kind: "cycle" | "performance";
    }>,
  ) => void;
  addCase: () => void;
  duplicateCase: (id: string) => void;
  removeCase: (id: string) => void;
  /** Set a per-case parameter override (elementId.key = value for this case only). */
  setCaseOverride: (caseId: string, elementId: string, key: string, value: ParamValue) => void;
  /** Remove a per-case parameter override; prunes the element entry when empty. */
  clearCaseOverride: (caseId: string, elementId: string, key: string) => void;
  runDataChecks: () => Promise<DataCheck[]>;
  /** Error-level data-check gate; resolves true when a run/sweep may proceed. */
  passesRunGate: () => Promise<boolean>;
  run: () => Promise<void>;
  /** Sequentially run a case once per swept value, each landing in run
   *  history; the study and its results table are saved with the project. */
  runSweep: (config: SweepConfig) => Promise<void>;
  /** Delete a saved study (its runs stay in the history). */
  removeStudy: (studyId: string) => void;
  stopRun: () => void;
  setActiveRun: (runId: string | null) => void;
  /** Toggle a run in the overlay set (ignored for the active run). */
  toggleOverlayRun: (runId: string) => void;
  /** Replace the overlay set outright (used to overlay a whole sweep family). */
  setOverlayRuns: (runIds: string[]) => void;
  clearOverlays: () => void;
  /** Delete a run from the history and from disk. */
  removeRun: (runId: string) => Promise<void>;
  /** Delete every stored run of the open project. */
  clearRuns: () => Promise<void>;
  /** Open the model a run was made with (its snapshot) as an unsaved copy. */
  openRunModel: (runId: string) => void;
}

export const useProjectStore = create<ProjectState>((set, get) => {
  // Coalesce rapid same-target edits (typing in a parameter/name field) into a
  // single undo entry: history is recorded for the first edit of a burst only.
  let lastEdit: { sig: string; at: number } | null = null;
  const EDIT_COALESCE_MS = 1200;

  /** Apply a mutation to a deep clone of the project (optionally recording undo
   *  history). `editSig` identifies a coalescable edit target (e.g. one field);
   *  consecutive edits with the same signature share one history entry. */
  function updateProject(
    fn: (draft: Project) => void,
    recordHistory = true,
    editSig?: string,
  ) {
    const { project, past } = get();
    if (!project) return;
    let record = recordHistory;
    if (record && editSig) {
      const now = Date.now();
      if (lastEdit && lastEdit.sig === editSig && now - lastEdit.at < EDIT_COALESCE_MS) {
        record = false;
      }
      lastEdit = { sig: editSig, at: now };
    } else if (record) {
      lastEdit = null;
    }
    const draft = structuredClone(project);
    fn(draft);
    set({
      project: draft,
      dirty: true,
      ...(record
        ? { past: [...past.slice(-(HISTORY_LIMIT - 1)), project], future: [] }
        : {}),
    });
  }

  function rootSystemOf(project: Project): SystemNode {
    return project.systems.find((s) => s.parentId === null) ?? project.systems[0];
  }

  /** Drop runs from the in-memory history (and from the chart selection). */
  function dropRuns(s: ProjectState, runIds: string[]): Partial<ProjectState> {
    const gone = new Set(runIds);
    const runs = s.runs.filter((r) => !gone.has(r.id));
    return {
      runs,
      activeRunId: s.activeRunId && gone.has(s.activeRunId) ? (runs[0]?.id ?? null) : s.activeRunId,
      overlayRunIds: s.overlayRunIds.filter((id) => !gone.has(id)),
    };
  }

  /**
   * List the project's stored runs: the newest MAX_RUNS are read from disk
   * and merged with runs in memory that are not stored (one still running,
   * or one whose store failed). Unless a run is already shown, Results opens
   * on the newest complete run that is not a sweep point (mainRunOf); that
   * one is read first so it is on screen while the others load. An answer
   * that arrives after another project was opened, or after a newer load
   * started, is dropped.
   */
  async function loadRunHistory(projectId: string): Promise<void> {
    const seq = ++runHistorySeq;
    const current = () => seq === runHistorySeq && get().project?.id === projectId;
    let index: StoredRunInfo[];
    try {
      index = await api.listRuns(projectId);
    } catch (e) {
      if (current()) {
        set({ runsLoading: false });
        get().log("warning", `Stored runs could not be listed: ${(e as Error).message}`);
      }
      return;
    }
    if (!current()) return;
    set({ storedRunCount: index.length, runsLoading: true });
    const shown = index.slice(0, MAX_RUNS);
    const onDisk = new Set(index.map((e) => e.id));
    const inMemory = new Set(get().runs.map((r) => r.id));
    const toRead = shown.filter((e) => !inMemory.has(e.id));
    const loaded = new Map<string, SimRun>();
    const read = (e: StoredRunInfo) =>
      api.fetchRun(projectId, e.id).then(
        (r) => void loaded.set(e.id, r),
        () => undefined,
      );
    const merge = (done: boolean) => {
      if (!current()) return;
      set((s) => {
        const byId = new Map(s.runs.map((r) => [r.id, r]));
        const runs = [
          ...s.runs.filter((r) => !onDisk.has(r.id)),
          ...shown.map((e) => byId.get(e.id) ?? loaded.get(e.id)).filter((r): r is SimRun => Boolean(r)),
        ]
          .sort((a, b) => b.startedAt - a.startedAt)
          .slice(0, MAX_RUNS);
        const ids = new Set(runs.map((r) => r.id));
        return {
          runs,
          runsLoading: !done,
          activeRunId: s.activeRunId && ids.has(s.activeRunId) ? s.activeRunId : (mainRunOf(runs)?.id ?? null),
          overlayRunIds: s.overlayRunIds.filter((id) => ids.has(id)),
        };
      });
    };
    const lead = toRead.find((e) => e.id === mainRunOf(shown)?.id);
    if (lead) {
      await read(lead);
      merge(false);
    }
    await Promise.all(toRead.filter((e) => e !== lead).map(read));
    merge(true);
    const unreadable = toRead.filter((e) => !loaded.has(e.id)).length;
    if (unreadable > 0 && current()) {
      get().log("warning", `${unreadable} stored run(s) could not be read and are not listed.`);
    }
  }

  /** Add a live edit to the in-flight run's snapshot. Edits of one parameter
   *  at one simulated time (typing a number) keep only the last value. */
  function logLiveEdit(edit: LiveEdit): void {
    if (!liveLog) return;
    const { runId, edits } = liveLog;
    const last = edits.at(-1);
    if (last && last.t === edit.t && last.elementId === edit.elementId && last.key === edit.key) {
      edits[edits.length - 1] = edit;
    } else {
      edits.push(edit);
    }
    set((s) => ({
      runs: s.runs.map((r) =>
        r.id === runId && r.snapshot ? { ...r, snapshot: { ...r.snapshot, liveEdits: [...edits] } } : r,
      ),
    }));
  }

  /** Store a finished run on disk with its project. A run that cannot be
   *  stored stays listed for this session only. */
  async function storeRun(projectId: string, run: SimRun): Promise<void> {
    const { log } = get();
    try {
      const reply = await api.storeRun(projectId, run);
      if (get().project?.id !== projectId) return;
      set((s) => ({ storedRunCount: reply.stored, ...dropRuns(s, reply.pruned) }));
      if (reply.pruned.length > 0) {
        log(
          "warning",
          `Stored runs of this project reached the ${Math.round(reply.budget / 2 ** 20)} MB disk budget: ` +
            `deleted the ${reply.pruned.length} oldest run(s).`,
        );
      }
    } catch (e) {
      log("warning", `Run '${run.caseName}' could not be stored on disk (${(e as Error).message}); it is kept for this session only.`);
    }
  }

  /**
   * Register a run at the head of the rolling history, stream the live result
   * into it, store it on disk once it ends, and resolve with the final
   * SimResult. The run carries a snapshot of what made it: `projectToRun`
   * and its case, the app version, the project's fingerprint and the live
   * edits made while it ran. Shared by `run` (one call) and `runSweep` (one
   * call per swept value). Does NOT run the validation
   * gate, toggle `running`, or switch ribbon tabs — the callers own that.
   */
  async function executeRun(
    projectToRun: Project,
    caseId: string,
    caseName: string,
    extra?: Partial<SimRun>,
  ): Promise<SimResult> {
    const { libraryById, log, appVersion } = get();
    const runId = uid("run");
    const simCase = projectToRun.cases.find((c) => c.id === caseId);
    // the model is the project without its saved studies (results, not
    // model): the engine runs it and the snapshot keeps it
    const { studies: _studies, ...model } = projectToRun;
    const snapshot: RunSnapshot | undefined = simCase && {
      project: model,
      case: simCase,
      appVersion,
      liveEdits: [],
    };
    const fingerprint = modelFingerprint(model).catch(() => undefined);
    const partial: SimResult = {
      caseId,
      status: "success",
      messages: [],
      channels: [],
      summary: [],
    };
    const newRun: SimRun = {
      id: runId,
      caseId,
      caseName,
      startedAt: Date.now(),
      status: "running",
      result: partial,
      ...(snapshot ? { snapshot } : {}),
      ...extra,
    };
    set((s) => ({
      runs: [newRun, ...s.runs].slice(0, MAX_RUNS),
      activeRunId: runId,
      liveValues: {},
      liveT: 0,
      livePct: 0,
    }));

    // incremental result assembly: step events stream in, the store is
    // flushed at most every LIVE_FLUSH_MS so charts/monitors update live
    const meta = channelMetaResolver(projectToRun, libraryById, get().unitGroups);
    const chanByKey = new Map<string, Channel>();
    let buffer: api.StepEvent[] = [];
    let flushTimer: ReturnType<typeof setTimeout> | null = null;
    const patchRun = (patch: Partial<SimRun>) =>
      set((s) => ({ runs: s.runs.map((r) => (r.id === runId ? { ...r, ...patch } : r)) }));
    const flush = () => {
      flushTimer = null;
      if (buffer.length === 0) return;
      const latest = buffer[buffer.length - 1];
      for (const step of buffer) {
        for (const [key, value] of Object.entries(step.values)) {
          let ch = chanByKey.get(key);
          if (!ch) {
            const m = meta(key);
            if (!m) continue;
            ch = { ...m, timeSeries: [] };
            chanByKey.set(key, ch);
            partial.channels.push(ch);
          }
          ch.timeSeries.push({ t: step.t, value });
        }
      }
      buffer = [];
      set((s) => ({
        runs: s.runs.map((r) => (r.id === runId ? { ...r, result: { ...partial } } : r)),
        liveValues: { ...latest.values },
        liveT: latest.t,
        livePct: latest.pct,
      }));
    };

    const handle = api.runSimulationLive(model, caseId, {
      onStep: (ev) => {
        buffer.push(ev);
        if (!flushTimer) flushTimer = setTimeout(flush, LIVE_FLUSH_MS);
      },
      onMessage: (m) => log(m.level, m.text),
    });
    activeRun = handle;
    liveLog = { runId, edits: [] };
    // the snapshot as the run ends: its live edits and the model's fingerprint
    const finalSnapshot = async (): Promise<Partial<SimRun>> => {
      if (!snapshot) return {};
      const edits = liveLog?.runId === runId ? [...liveLog.edits] : [];
      const modelHash = await fingerprint;
      return { snapshot: { ...snapshot, liveEdits: edits, ...(modelHash ? { modelHash } : {}) } };
    };
    try {
      const result = await handle.done;
      if (flushTimer) clearTimeout(flushTimer);
      const incomplete = incompleteReason(result);
      const finished: SimRun = {
        ...newRun,
        result,
        status: result.status,
        ...(incomplete ? { incomplete } : {}),
        ...(await finalSnapshot()),
      };
      set((s) => ({
        runs: s.runs.map((r) => (r.id === runId ? finished : r)),
        activeRunId: runId,
      }));
      await storeRun(projectToRun.id, finished);
      return result;
    } catch (e) {
      if (flushTimer) clearTimeout(flushTimer);
      patchRun({ status: "failed", incomplete: "connection lost", ...(await finalSnapshot()) });
      // keep what arrived before the connection dropped, if the engine is still there
      const lost = get().runs.find((r) => r.id === runId);
      if (lost) await storeRun(projectToRun.id, lost);
      throw e;
    } finally {
      activeRun = null;
      liveLog = null;
    }
  }

  return {
    library: [],
    libraryById: {},
    unitGroups: {},
    offline: false,
    loaded: false,
    appVersion: null,
    project: null,
    revision: null,
    exampleId: null,
    activeSystemId: null,
    selectedElementId: null,
    dirty: false,
    clipboard: null,
    pendingCanvasSelection: null,
    past: [],
    future: [],
    messages: [],
    dataChecks: null,
    runs: [],
    storedRunCount: 0,
    runsLoading: false,
    activeCaseId: null,
    activeRunId: null,
    overlayRunIds: [],
    running: false,
    checking: false,
    liveValues: {},
    liveT: 0,
    livePct: 0,

    init: async () => {
      const [lib, appVersion] = await Promise.all([api.fetchLibrary(), api.fetchVersion()]);
      const demo = await api.fetchDemoProject();
      const libraryById = Object.fromEntries(lib.components.map((c) => [c.id, c]));
      // restore the autosaved working copy if one exists, else open the demo
      // example as a copy
      const draft = loadDraft();
      const unsaved = Boolean(draft && !draft.clean);
      const exampleId = draft ? (draft.example ?? null) : demo.project.id;
      let { project, revision } = draft
        ? { project: draft.project, revision: draft.revision ?? null }
        : { project: exampleCopy(demo.project), revision: null };
      if (draft?.clean && !lib.offline) {
        // nothing was unsaved: reopen the project from disk (it may be newer
        // than the kept copy), or an example's current version (an update may
        // have corrected it) under the copy's id, which its runs are stored
        // under; falling back to the copy if it is gone
        try {
          if (exampleId) {
            project = { ...(await api.fetchExample(exampleId)), id: draft.project.id };
          } else {
            ({ project, revision } = fromDisk(await api.fetchProject(draft.project.id)));
          }
        } catch {
          /* keep the copy */
        }
      }
      set({
        library: lib.components,
        libraryById,
        unitGroups: lib.unitGroups,
        offline: lib.offline,
        loaded: true,
        appVersion,
        project,
        revision,
        exampleId,
        activeSystemId: rootSystemOf(project).id,
        activeCaseId: project.cases[0]?.id ?? null,
        dirty: unsaved,
      });
      const log = get().log;
      log("info", `Component library loaded (${lib.components.length} components).`);
      if (draft && unsaved) {
        log("info", `Restored your unsaved draft from ${new Date(draft.savedAt).toLocaleString()}.`);
      } else {
        log("info", `Project '${project.name}' opened.`);
      }
      if (lib.offline) {
        log(
          "warning",
          "Backend not reachable — running from bundled data. Start the FastAPI service to enable save, data checks and simulation.",
        );
      } else {
        void loadRunHistory(project.id);
      }
    },

    log: (level, text) =>
      set((s) => ({ messages: [...s.messages, { level, text, time: now() }] })),
    clearMessages: () => set({ messages: [] }),

    select: (elementId) => set({ selectedElementId: elementId }),

    setActiveSystem: (systemId) =>
      set({ activeSystemId: systemId, selectedElementId: null }),

    addElement: (defId, position) => {
      const { libraryById, activeSystemId, project } = get();
      const def = libraryById[defId];
      if (!def || !project || !activeSystemId) return;
      const count = project.systems.reduce(
        (n, s) => n + s.elements.filter((e) => e.componentDefId === defId).length,
        0,
      );
      const elId = uid("el");
      const isContainer = defId === "container.system";
      const subSystemId = isContainer ? uid("sys") : undefined;
      updateProject((draft) => {
        const system = draft.systems.find((s) => s.id === activeSystemId);
        if (!system) return;
        system.elements.push({
          id: elId,
          componentDefId: defId,
          label: `${def.name} ${count + 1}`,
          position,
          parameterOverrides: {},
          ...(isContainer ? { isSubSystem: true, subSystemId } : {}),
        });
        if (isContainer && subSystemId) {
          draft.systems.push({
            id: subSystemId,
            name: `${def.name} ${count + 1}`,
            parentId: activeSystemId,
            elements: [],
            connections: [],
          });
        }
      });
      set({ selectedElementId: elId });
    },

    moveElement: (id, position) =>
      updateProject((draft) => {
        for (const s of draft.systems) {
          const el = s.elements.find((e) => e.id === id);
          if (el) el.position = position;
        }
      }, false),

    resizeElement: (id, size) =>
      updateProject((draft) => {
        for (const s of draft.systems) {
          const el = s.elements.find((e) => e.id === id);
          if (el) {
            el.size = {
              width: Math.round(size.width),
              height: Math.round(size.height),
            };
          }
        }
      }, false),

    beginHistory: () => {
      const { project, past } = get();
      if (!project) return;
      lastEdit = null; // a drag/resize burst is its own history entry
      set({ past: [...past.slice(-(HISTORY_LIMIT - 1)), project], future: [] });
    },

    removeElements: (ids, connectionIds = []) => {
      if (ids.length === 0 && connectionIds.length === 0) return;
      updateProject((draft) => {
        // collect sub-system trees rooted at removed container elements
        const doomedSystems = new Set<string>();
        const collectSystems = (sysId: string) => {
          doomedSystems.add(sysId);
          for (const s of draft.systems) {
            if (s.parentId === sysId) collectSystems(s.id);
          }
        };
        const doomedElements = new Set(ids);
        for (const s of draft.systems) {
          for (const el of s.elements) {
            if (doomedElements.has(el.id) && el.subSystemId) collectSystems(el.subSystemId);
          }
        }
        for (const sysId of doomedSystems) {
          const sys = draft.systems.find((s) => s.id === sysId);
          sys?.elements.forEach((el) => doomedElements.add(el.id));
        }
        draft.systems = draft.systems.filter((s) => !doomedSystems.has(s.id));
        for (const s of draft.systems) {
          s.elements = s.elements.filter((e) => !doomedElements.has(e.id));
          s.connections = s.connections.filter(
            (c) =>
              !connectionIds.includes(c.id) &&
              !doomedElements.has(c.sourceElementId) &&
              !doomedElements.has(c.targetElementId),
          );
        }
        draft.dataBusConnections = draft.dataBusConnections.filter(
          (d) =>
            !connectionIds.includes(d.id) &&
            !doomedElements.has(d.element1Id) &&
            !doomedElements.has(d.element2Id),
        );
      });
      const { selectedElementId, activeSystemId, project } = get();
      if (selectedElementId && ids.includes(selectedElementId)) set({ selectedElementId: null });
      // if the active system was deleted, fall back to root
      if (project && !project.systems.some((s) => s.id === activeSystemId)) {
        set({ activeSystemId: rootSystemOf(project).id });
      }
    },

    copyElements: (ids) => {
      const { project, activeSystemId } = get();
      if (!project || !activeSystemId || ids.length === 0) return;
      set({ clipboard: collectSelection(project, activeSystemId, ids) });
    },

    duplicateElements: (ids) => {
      const { project, activeSystemId } = get();
      if (!project || !activeSystemId || ids.length === 0) return;
      const data = collectSelection(project, activeSystemId, ids);
      let newIds: string[] = [];
      updateProject((draft) => {
        newIds = cloneElementsInto(draft, activeSystemId, data, { x: 28, y: 28 });
      });
      if (newIds.length) {
        set({ pendingCanvasSelection: newIds, selectedElementId: newIds[newIds.length - 1] });
      }
    },

    pasteClipboard: (position) => {
      const { project, activeSystemId, clipboard } = get();
      if (!project || !activeSystemId || !clipboard || clipboard.elements.length === 0) return;
      let offset = { x: 28, y: 28 };
      if (position) {
        const minX = Math.min(...clipboard.elements.map((e) => e.position.x));
        const minY = Math.min(...clipboard.elements.map((e) => e.position.y));
        offset = { x: position.x - minX, y: position.y - minY };
      }
      let newIds: string[] = [];
      updateProject((draft) => {
        newIds = cloneElementsInto(draft, activeSystemId, clipboard, offset);
      });
      if (newIds.length) {
        set({ pendingCanvasSelection: newIds, selectedElementId: newIds[newIds.length - 1] });
      }
    },

    clearPendingSelection: () => set({ pendingCanvasSelection: null }),

    renameElement: (id, label) =>
      updateProject(
        (draft) => {
          for (const s of draft.systems) {
            const el = s.elements.find((e) => e.id === id);
            if (el) {
              el.label = label;
              if (el.subSystemId) {
                const sub = draft.systems.find((sys) => sys.id === el.subSystemId);
                if (sub) sub.name = label;
              }
            }
          }
        },
        true,
        `label:${id}`,
      ),

    setParameter: (elementId, key, value) => {
      updateProject(
        (draft) => {
          for (const s of draft.systems) {
            const el = s.elements.find((e) => e.id === elementId);
            if (el) el.parameterOverrides[key] = value;
          }
        },
        true,
        `param:${elementId}:${key}`,
      );
      // scalar edits stream into a running simulation (tables/code apply next run)
      if (activeRun && typeof value !== "object") {
        activeRun.setParam(elementId, key, value);
        logLiveEdit({ t: get().liveT, elementId, key, value });
      }
    },

    setTableOutside: (elementId, key, policies) =>
      updateProject(
        (draft) => {
          for (const s of draft.systems) {
            const el = s.elements.find((e) => e.id === elementId);
            if (el) el.tableOutside = { ...el.tableOutside, [key]: policies };
          }
        },
        true,
        `outside:${elementId}:${key}`,
      ),

    setPortSide: (elementId, portId, side) =>
      updateProject((draft) => {
        for (const s of draft.systems) {
          const el = s.elements.find((e) => e.id === elementId);
          if (el) el.portSides = { ...el.portSides, [portId]: side };
        }
      }),

    setPortPlacement: (elementId, portId, side, offset) =>
      updateProject((draft) => {
        const clamped = Math.min(0.92, Math.max(0.08, offset));
        for (const s of draft.systems) {
          const el = s.elements.find((e) => e.id === elementId);
          if (el) {
            el.portSides = { ...el.portSides, [portId]: side };
            el.portOffsets = { ...el.portOffsets, [portId]: clamped };
          }
        }
      }),

    setDynamicPorts: (elementId, ports) =>
      updateProject((draft) => {
        let el: (typeof draft.systems)[number]["elements"][number] | undefined;
        for (const s of draft.systems) {
          el = s.elements.find((e) => e.id === elementId) ?? el;
        }
        if (!el) return;
        el.dynamicPorts = ports;
        // drop connections that reference removed ports
        const validIds = new Set(ports.map((p) => p.id));
        draft.dataBusConnections = draft.dataBusConnections.filter((d) => {
          if (d.element1Id === elementId && !validIds.has(d.port1Id)) {
            const def = get().libraryById[el!.componentDefId];
            if (!def?.ports.some((p) => p.id === d.port1Id)) return false;
          }
          if (d.element2Id === elementId && !validIds.has(d.port2Id)) {
            const def = get().libraryById[el!.componentDefId];
            if (!def?.ports.some((p) => p.id === d.port2Id)) return false;
          }
          return true;
        });
        for (const s of draft.systems) {
          s.connections = s.connections.filter((c) => {
            const def = get().libraryById[el!.componentDefId];
            const staticIds = new Set(def?.ports.map((p) => p.id) ?? []);
            if (c.sourceElementId === elementId &&
                !validIds.has(c.sourcePortId) && !staticIds.has(c.sourcePortId)) return false;
            if (c.targetElementId === elementId &&
                !validIds.has(c.targetPortId) && !staticIds.has(c.targetPortId)) return false;
            return true;
          });
        }
      }),

    addConnection: (sourceElementId, sourcePortId, targetElementId, targetPortId, replaceId) => {
      const { project, libraryById, log } = get();
      if (!project) return;
      const elements = project.systems.flatMap((s) => s.elements);
      const src = elements.find((e) => e.id === sourceElementId);
      const tgt = elements.find((e) => e.id === targetElementId);
      if (!src || !tgt) return;
      const srcPort = libraryById[src.componentDefId]?.ports.find((p) => p.id === sourcePortId);
      const tgtPort = libraryById[tgt.componentDefId]?.ports.find((p) => p.id === targetPortId);
      if (!srcPort || !tgtPort) return;

      if (srcPort.kind === "signal" || tgtPort.kind === "signal") {
        if (srcPort.kind !== "signal" || tgtPort.kind !== "signal") {
          log("error", `Cannot connect a ${srcPort.kind} port to a ${tgtPort.kind} port.`);
          return;
        }
        // signal wiring on the canvas is stored as a Data Bus connection
        get().addDataBus(sourceElementId, sourcePortId, targetElementId, targetPortId);
        return;
      }
      if (srcPort.kind !== tgtPort.kind) {
        log(
          "error",
          `Incompatible connection: '${srcPort.name}' (${srcPort.kind}) ↔ '${tgtPort.name}' (${tgtPort.kind}).`,
        );
        return;
      }
      const dup = project.systems.some((s) =>
        s.connections.some(
          (c) =>
            c.id !== replaceId &&
            ((c.sourceElementId === sourceElementId &&
              c.sourcePortId === sourcePortId &&
              c.targetElementId === targetElementId &&
              c.targetPortId === targetPortId) ||
              (c.sourceElementId === targetElementId &&
                c.sourcePortId === targetPortId &&
                c.targetElementId === sourceElementId &&
                c.targetPortId === sourcePortId)),
        ),
      );
      if (dup) return;
      const { activeSystemId } = get();
      updateProject((draft) => {
        if (replaceId) {
          for (const s of draft.systems) s.connections = s.connections.filter((c) => c.id !== replaceId);
        }
        const system = draft.systems.find((s) => s.id === activeSystemId);
        system?.connections.push({
          id: uid("c"),
          sourceElementId,
          sourcePortId,
          targetElementId,
          targetPortId,
        });
      });
    },

    addDataBus: (el1, p1, el2, p2) => {
      const { project, libraryById, log } = get();
      if (!project) return;
      const elements = project.systems.flatMap((s) => s.elements);
      const e1 = elements.find((e) => e.id === el1);
      const e2 = elements.find((e) => e.id === el2);
      const port1 = e1 && portsOf(e1, libraryById).find((p) => p.id === p1);
      const port2 = e2 && portsOf(e2, libraryById).find((p) => p.id === p2);
      if (!e1 || !e2 || !port1 || !port2) return;
      if (port1.kind !== "signal" || port2.kind !== "signal") {
        log("error", "Data bus connections must link two signal ports.");
        return;
      }
      if (port1.direction === port2.direction) {
        log(
          "warning",
          `Data bus: '${e1.label}.${port1.name}' and '${e2.label}.${port2.name}` +
            `' are both ${port1.direction}s — connection added, but no data will flow.`,
        );
      }
      const dup = project.dataBusConnections.some(
        (d) =>
          (d.element1Id === el1 && d.port1Id === p1 && d.element2Id === el2 && d.port2Id === p2) ||
          (d.element1Id === el2 && d.port1Id === p2 && d.element2Id === el1 && d.port2Id === p1),
      );
      if (dup) {
        log("info", "This data bus connection already exists.");
        return;
      }
      // an input takes one signal (the engine would silently keep only the
      // last link), so a second source is refused
      const [inEl, inPort] =
        port1.direction === "output" && port2.direction !== "output"
          ? [e2, port2]
          : port2.direction === "output" && port1.direction !== "output"
            ? [e1, port1]
            : [null, null];
      const existing = inEl && inPort && signalSourceOf(project, libraryById, inEl.id, inPort.id);
      if (inEl && inPort && existing) {
        log(
          "error",
          `'${inEl.label}.${inPort.name}' already takes its signal from '${existing}' — an input ` +
            "can have only one source. Remove that link first to connect a different one.",
        );
        return;
      }
      updateProject((draft) => {
        draft.dataBusConnections.push({
          id: uid("dbc"),
          element1Id: el1,
          port1Id: p1,
          element2Id: el2,
          port2Id: p2,
        });
      });
      log("info", `Data bus: '${e1.label}.${port1.name}' ↔ '${e2.label}.${port2.name}' connected.`);
    },

    removeDataBus: (id) =>
      updateProject((draft) => {
        draft.dataBusConnections = draft.dataBusConnections.filter((d) => d.id !== id);
      }),

    renameSystem: (systemId, name) =>
      updateProject(
        (draft) => {
          const sys = draft.systems.find((s) => s.id === systemId);
          if (sys) sys.name = name;
          draft.name = draft.systems.find((s) => s.parentId === null)?.name ?? draft.name;
        },
        true,
        `system:${systemId}`,
      ),

    undo: () => {
      const { past, future, project } = get();
      if (past.length === 0 || !project) return;
      const prev = past[past.length - 1];
      set({
        project: keepStudies(prev, project),
        past: past.slice(0, -1),
        future: [project, ...future].slice(0, HISTORY_LIMIT),
        dirty: true,
      });
    },
    redo: () => {
      const { past, future, project } = get();
      if (future.length === 0 || !project) return;
      const next = future[0];
      set({
        project: keepStudies(next, project),
        future: future.slice(1),
        past: [...past.slice(-(HISTORY_LIMIT - 1)), project],
        dirty: true,
      });
    },

    newProject: () => {
      const rootId = uid("sys");
      const caseId = uid("case");
      const project: Project = {
        id: uid("project"),
        name: "New Project",
        systems: [{ id: rootId, name: "New Project", parentId: null, elements: [], connections: [] }],
        dataBusConnections: [],
        cases: [{ id: caseId, name: "Case 1", duration: 600, timeStep: 1 }],
      };
      set({
        project,
        revision: null,
        exampleId: null,
        activeSystemId: rootId,
        activeCaseId: caseId,
        activeRunId: null,
        overlayRunIds: [],
        selectedElementId: null,
        past: [],
        future: [],
        runs: [],
        storedRunCount: 0,
        runsLoading: false,
        dataChecks: null,
        dirty: false,
      });
      get().log("info", "New project created.");
    },

    openProject: async (id) => {
      try {
        const { project, revision } = fromDisk(await api.fetchProject(id));
        set({
          project,
          revision,
          exampleId: null,
          activeSystemId: project.systems.find((s) => s.parentId === null)?.id ?? project.systems[0]?.id,
          activeCaseId: project.cases[0]?.id ?? null,
          activeRunId: null,
          overlayRunIds: [],
          selectedElementId: null,
          past: [],
          future: [],
          runs: [],
          storedRunCount: 0,
          runsLoading: false,
          dataChecks: null,
          dirty: false,
        });
        get().log("info", `Project '${project.name}' opened.`);
        void loadRunHistory(project.id);
      } catch (e) {
        get().log("error", `Failed to open project: ${(e as Error).message}`);
      }
    },

    openExample: async (id) => {
      let example: Project;
      try {
        example = await api.fetchExample(id);
      } catch (e) {
        get().log("error", `Failed to open the example: ${(e as Error).message}`);
        return;
      }
      const project = exampleCopy(example);
      runHistorySeq++; // runs still loading for the project it replaces are dropped
      set({
        project,
        revision: null,
        exampleId: id,
        activeSystemId: rootSystemOf(project).id,
        activeCaseId: project.cases[0]?.id ?? null,
        activeRunId: null,
        overlayRunIds: [],
        selectedElementId: null,
        past: [],
        future: [],
        runs: [],
        storedRunCount: 0,
        runsLoading: false,
        dataChecks: null,
        dirty: false,
      });
      get().log(
        "info",
        `Example '${project.name}' opened as a copy. Save keeps it as a new project of yours; the example stays as it is.`,
      );
    },

    hideExample: async (id, name) => {
      try {
        await api.hideExample(id);
      } catch (e) {
        get().log("error", `Could not hide the example: ${(e as Error).message}`);
        return false;
      }
      get().log("info", `Example '${name}' hidden from the Open menu (Restore hidden examples brings it back).`);
      return true;
    },

    restoreExamples: async () => {
      try {
        const { restored } = await api.restoreExamples();
        get().log("info", `${restored.length} hidden example(s) are back in the Open menu.`);
      } catch (e) {
        get().log("error", `Could not restore the examples: ${(e as Error).message}`);
      }
    },

    saveRemote: () => {
      const save = async () => {
        const { project, revision, exampleId, log } = get();
        if (!project) return;
        try {
          const res = await api.saveProject(project, revision);
          // an edit made while the save was in flight is still unsaved
          set({ dirty: get().project !== project, revision: res.revision ?? revision, exampleId: null });
          log(
            "info",
            exampleId
              ? `Project '${project.name}' saved to the server as a new project; the example it was copied from is unchanged.`
              : `Project '${project.name}' saved to the server.`,
          );
        } catch (e) {
          if ((e as { status?: number }).status !== 409) {
            log("error", `Save failed: ${(e as Error).message}. Use Export to download the project file instead.`);
            return;
          }
          // the file on disk is not the version this copy started from
          const why =
            revision === null
              ? `A project with the id '${project.id}' already exists on disk.`
              : `'${project.name}' was changed on disk after you opened it (saved from another window or by another program).`;
          log("warning", `Not saved yet: ${why}`);
          const overwrite = await confirmDialog({
            title: revision === null ? "Project already on disk" : "Project changed on disk",
            message:
              `${why} Saving now would replace that version with yours. ` +
              "Cancel keeps the file on disk as it is; your changes stay open here, " +
              "and Export saves a copy of them.",
            confirmLabel: "Overwrite",
            cancelLabel: "Cancel",
            danger: true,
          });
          if (!overwrite) {
            log("warning", "Save cancelled — the file on disk was left unchanged; your changes are still unsaved.");
            return;
          }
          try {
            const res = await api.saveProject(project);
            // edits made while the dialog was open were not in this save
            set({ dirty: get().project !== project, revision: res.revision ?? null, exampleId: null });
            log("info", `Project '${project.name}' saved to the server, replacing the version on disk ` +
              "(Project → Restore opens that version again).");
          } catch (e2) {
            log("error", `Save failed: ${(e2 as Error).message}. Use Export to download the project file instead.`);
          }
        }
      };
      const next = saveQueue.then(save);
      saveQueue = next;
      return next;
    },

    exportProject: () => {
      const { project } = get();
      if (!project) return;
      const blob = new Blob([JSON.stringify(project, null, 2)], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `${project.id}.json`;
      a.click();
      URL.revokeObjectURL(url);
      get().log("info", `Project exported as ${project.id}.json.`);
    },

    importProject: (json) => {
      try {
        // a file saved from the engine's API may carry its revision: drop it
        const { project } = fromDisk(JSON.parse(json) as api.StoredProject);
        if (!project.id || !Array.isArray(project.systems)) {
          throw new Error("not a LightSim project file");
        }
        project.dataBusConnections ??= [];
        project.cases ??= [];
        set({
          project,
          revision: null,
          exampleId: null,
          activeSystemId: project.systems.find((s) => s.parentId === null)?.id ?? project.systems[0]?.id,
          activeCaseId: project.cases[0]?.id ?? null,
          activeRunId: null,
          overlayRunIds: [],
          selectedElementId: null,
          past: [],
          future: [],
          runs: [],
          storedRunCount: 0,
          runsLoading: false,
          dataChecks: null,
          dirty: true,
        });
        get().log("info", `Project '${project.name}' imported.`);
        void loadRunHistory(project.id);
      } catch (e) {
        get().log("error", `Import failed: ${(e as Error).message}`);
      }
    },

    openAsCopy: (source, name, note) => {
      const project: Project = { ...structuredClone(source), id: uid("project"), name };
      runHistorySeq++; // runs still loading for the project it replaces are dropped
      set({
        project,
        revision: null,
        exampleId: null,
        activeSystemId: rootSystemOf(project).id,
        activeCaseId: project.cases[0]?.id ?? null,
        activeRunId: null,
        overlayRunIds: [],
        selectedElementId: null,
        past: [],
        future: [],
        runs: [],
        storedRunCount: 0,
        runsLoading: false,
        dataChecks: null,
        dirty: true,
      });
      get().log("info", note);
    },

    setActiveCase: (id) => set({ activeCaseId: id }),

    setCaseField: (caseId, patch) =>
      updateProject(
        (draft) => {
          const c = draft.cases.find((cc) => cc.id === caseId);
          if (c) Object.assign(c, patch);
        },
        true,
        `case:${caseId}:${Object.keys(patch).sort().join(",")}`,
      ),

    addCase: () => {
      const id = uid("case");
      updateProject((draft) => {
        draft.cases.push({
          id,
          name: `Case ${draft.cases.length + 1}`,
          duration: 600,
          timeStep: 1,
        });
      });
      set({ activeCaseId: id });
    },

    duplicateCase: (id) => {
      const newId = uid("case");
      updateProject((draft) => {
        const idx = draft.cases.findIndex((c) => c.id === id);
        if (idx < 0) return;
        draft.cases.splice(idx + 1, 0, {
          ...draft.cases[idx],
          id: newId,
          name: `${draft.cases[idx].name} (copy)`,
          // deep-clone so the copy's overrides aren't a shared reference
          parameterOverrides: structuredClone(draft.cases[idx].parameterOverrides ?? {}),
        });
      });
      set({ activeCaseId: newId });
    },

    removeCase: (id) => {
      const { project, activeCaseId } = get();
      if (!project || project.cases.length <= 1) return;
      const fallback = project.cases.find((c) => c.id !== id)?.id ?? null;
      updateProject((draft) => {
        draft.cases = draft.cases.filter((c) => c.id !== id);
      });
      if (activeCaseId === id) set({ activeCaseId: fallback });
    },

    setCaseOverride: (caseId, elementId, key, value) =>
      updateProject(
        (draft) => {
          const c = draft.cases.find((cc) => cc.id === caseId);
          if (!c) return;
          const ov = { ...(c.parameterOverrides ?? {}) };
          ov[elementId] = { ...(ov[elementId] ?? {}), [key]: value };
          c.parameterOverrides = ov;
        },
        true,
        `caseov:${caseId}:${elementId}:${key}`,
      ),

    clearCaseOverride: (caseId, elementId, key) =>
      updateProject((draft) => {
        const c = draft.cases.find((cc) => cc.id === caseId);
        const forEl = c?.parameterOverrides?.[elementId];
        if (!c || !forEl) return;
        const next = { ...forEl };
        delete next[key];
        const ov = { ...c.parameterOverrides };
        if (Object.keys(next).length === 0) delete ov[elementId];
        else ov[elementId] = next;
        c.parameterOverrides = ov;
      }),

    runDataChecks: async () => {
      const { project, log } = get();
      if (!project) return [];
      set({ checking: true });
      try {
        const checks = await api.validateProject(project);
        set({ dataChecks: checks, checking: false });
        if (get().project !== project) scheduleRecheck(); // edited while it checked
        const errors = checks.filter((c) => c.level === "error").length;
        const warnings = checks.filter((c) => c.level === "warning").length;
        log(
          errors ? "error" : warnings ? "warning" : "info",
          `Data checks: ${errors} error(s), ${warnings} warning(s).`,
        );
        {
          const ui = useUIStore.getState();
          if (ui.ribbonTab === "results") ui.setRibbonTab("home");
          ui.focusPanel("data-checks");
        }
        return checks;
      } catch (e) {
        set({ checking: false });
        log("error", `Data checks failed: ${(e as Error).message}`);
        return [];
      }
    },

    run: async () => {
      const { project, activeCaseId, log, running } = get();
      if (!project || running) return;
      if (!activeCaseId) {
        log("error", "No simulation case selected.");
        return;
      }

      // pre-flight validation gate: block the run on error-level data checks so
      // broken models fail fast (and visibly) instead of deep inside the solver.
      // A fresh single run starts with a clean overlay set.
      set({ running: true, overlayRunIds: [] });
      if (!(await get().passesRunGate())) {
        set({ running: false });
        return;
      }

      const caseId = activeCaseId;
      const simCase = project.cases.find((c) => c.id === caseId);
      log("info", `Running case '${simCase?.name ?? caseId}' …`);
      try {
        const result = await executeRun(project, caseId, simCase?.name ?? caseId);
        set({ running: false });
        if (result.status === "failed") {
          log("error", "Simulation failed — see messages above.");
          const ui = useUIStore.getState();
          if (ui.ribbonTab === "results") ui.setRibbonTab("home");
          ui.focusPanel("messages");
        } else {
          log(
            result.status === "warning" ? "warning" : "info",
            `Simulation finished with status '${result.status}'. ${result.channels.length} channels available in Results.`,
          );
          // switch to the full-page Results workspace
          useUIStore.getState().setRibbonTab("results");
        }
      } catch (e) {
        set({ running: false });
        log("error", `Simulation failed: ${(e as Error).message}`);
        const ui = useUIStore.getState();
        if (ui.ribbonTab === "results") ui.setRibbonTab("home");
        ui.focusPanel("messages");
      }
    },

    runSweep: async ({ caseId, elementId, paramKey, values }) => {
      const { project, log, libraryById, running } = get();
      if (!project || running) return;
      const simCase = project.cases.find((c) => c.id === caseId);
      if (!simCase) {
        log("error", "Sweep target case not found.");
        return;
      }
      if (values.length === 0) {
        log("error", "Sweep has no values to run.");
        return;
      }

      const el = project.systems.flatMap((s) => s.elements).find((e) => e.id === elementId);
      const pdef = el && libraryById[el.componentDefId]?.parameters.find((p) => p.key === paramKey);
      const paramLabel = pdef ? pdef.label : paramKey;
      const paramUnit = pdef && pdef.unit !== "-" ? pdef.unit : "";
      const unit = paramUnit ? ` ${paramUnit}` : "";
      const sweepId = uid("sweep");
      const startedAt = Date.now();
      // the study's table: a row per value run, in run order, and its columns
      const points: StudyPoint[] = [];
      const kpiUnits = new Map<string, string>();

      set({ running: true });
      if (!(await get().passesRunGate())) {
        set({ running: false });
        return;
      }

      sweepAborted = false;
      log(
        "info",
        `Sweep: ${el?.label ?? elementId} · ${paramLabel} over ${values.length} value(s) …`,
      );
      try {
        for (const value of values) {
          if (sweepAborted) break;
          // clone the project so the swept override is scoped to this one run
          const runProject = structuredClone(project);
          const rc = runProject.cases.find((c) => c.id === caseId);
          if (!rc) break;
          rc.parameterOverrides = {
            ...(rc.parameterOverrides ?? {}),
            [elementId]: { ...(rc.parameterOverrides?.[elementId] ?? {}), [paramKey]: value },
          };
          const label = `${simCase.name} · ${paramLabel}=${value}${unit}`;
          try {
            await executeRun(runProject, caseId, label, {
              sweepId,
              sweepParam: paramLabel,
              sweepValue: value,
              sweepUnit: paramUnit,
            });
          } catch (e) {
            log("error", `Sweep point ${paramLabel}=${value} failed: ${(e as Error).message}`);
            // keep going with the remaining points
          }
          // this point's run: the newest run of the sweep not in the table yet
          const tabled = new Set(points.map((p) => p.runId));
          const pointRun = get().runs.find((r) => r.sweepId === sweepId && !tabled.has(r.id));
          points.push(pointRun ? studyPoint(pointRun, [value]) : { values: [value], status: "failed", kpis: {} });
          for (const v of pointRun?.result.summary ?? []) if (!kpiUnits.has(v.label)) kpiUnits.set(v.label, v.unit);
        }
      } finally {
        set({ running: false });
      }
      const family = get()
        .runs.filter((r) => r.sweepId === sweepId)
        .sort((a, b) => (a.sweepValue ?? 0) - (b.sweepValue ?? 0));
      // a point counts only if its run finished normally; stopped or failed
      // points keep their partial data in the history but stay off the curve
      const complete = family.filter((r) => !r.incomplete);
      const incomplete = family.filter((r) => r.incomplete);
      if (family.length > 0) {
        const notes = [
          ...incomplete.map((r) => `${paramLabel}=${r.sweepValue}${unit} ${r.incomplete}`),
          ...(family.length < values.length ? [`${values.length - family.length} not run`] : []),
        ];
        log(
          notes.length ? "warning" : "info",
          `Sweep finished — ${complete.length} of ${values.length} point(s) complete` +
            (notes.length ? ` (${notes.join("; ")}). Incomplete points are left out of the sweep chart and table.` : "."),
        );
        // save the study with the project (unless another one was opened
        // meanwhile), with a row for every value, run or not
        if (get().project?.id === project.id) {
          const notRun = values.slice(points.length).map((v): StudyPoint => ({ values: [v], status: "not run", kpis: {} }));
          const study: Study = {
            id: sweepId,
            startedAt,
            caseId,
            caseName: simCase.name,
            factors: [
              { elementId, paramKey, elementLabel: el?.label ?? elementId, paramLabel, unit: paramUnit, values },
            ],
            kpis: [...kpiUnits].map(([label, kpiUnit]) => ({ label, unit: kpiUnit })),
            points: [...points, ...notRun],
          };
          updateProject((draft) => {
            draft.studies = [...(draft.studies ?? []), study];
          }, false);
          log(
            "info",
            `Sweep saved as a study of '${project.name}' (Cases & Parameters → Saved studies); ` +
              "save the project to keep it on disk.",
          );
        }
        // overlay the complete family: lowest swept value is the primary run,
        // the rest are overlaid, so all appear together in Results by default.
        const shown = complete.length > 0 ? complete : family.slice(0, 1);
        set({
          activeRunId: shown[0].id,
          overlayRunIds: shown.slice(1).map((r) => r.id),
        });
        useUIStore.getState().setRibbonTab("results");
      } else {
        log("warning", "Sweep produced no runs.");
      }
    },

    removeStudy: (studyId) =>
      updateProject((draft) => {
        draft.studies = (draft.studies ?? []).filter((st) => st.id !== studyId);
      }, false),

    /** Error-level data-check gate shared by run + runSweep. */
    passesRunGate: async () => {
      const { project, log } = get();
      if (!project) return false;
      try {
        const checks = await api.validateProject(project);
        set({ dataChecks: checks });
        if (get().project !== project) scheduleRecheck(); // edited while it checked
        const errors = checks.filter((c) => c.level === "error");
        if (errors.length > 0) {
          log("error", `Run blocked — fix ${errors.length} data-check error(s) first.`);
          const ui = useUIStore.getState();
          if (ui.ribbonTab === "results") ui.setRibbonTab("home");
          ui.focusPanel("data-checks");
          return false;
        }
      } catch (e) {
        // backend unreachable — the run's own connection will surface the failure
        log("warning", `Pre-flight data checks unavailable (${(e as Error).message}); running anyway.`);
      }
      return true;
    },

    stopRun: () => {
      sweepAborted = true;
      if (activeRun) {
        activeRun.cancel();
        get().log("info", "Stop requested — waiting for the solver to wind down …");
      }
    },

    setActiveRun: (runId) =>
      // a run can't overlay itself — drop it from the overlay set if selected
      set((s) => ({
        activeRunId: runId,
        overlayRunIds: s.overlayRunIds.filter((id) => id !== runId),
      })),
    toggleOverlayRun: (runId) =>
      set((s) => {
        if (runId === s.activeRunId) return {};
        return {
          overlayRunIds: s.overlayRunIds.includes(runId)
            ? s.overlayRunIds.filter((id) => id !== runId)
            : [...s.overlayRunIds, runId],
        };
      }),
    setOverlayRuns: (runIds) =>
      set((s) => ({ overlayRunIds: runIds.filter((id) => id !== s.activeRunId) })),
    clearOverlays: () => set({ overlayRunIds: [] }),
    removeRun: async (runId) => {
      const { project, log } = get();
      runHistorySeq++; // a load still in flight must not list the run again
      set((s) => dropRuns(s, [runId]));
      if (!project) return;
      try {
        const reply = await api.deleteRun(project.id, runId);
        if (get().project?.id === project.id) set({ storedRunCount: reply.stored });
      } catch (e) {
        // 404: the run was never stored (its store failed), so it is gone already
        if (!(e as Error).message.startsWith("404")) {
          log("error", `Could not delete the stored run: ${(e as Error).message}`);
        }
      }
      // list the next older stored run in its place (or put back one not deleted)
      await loadRunHistory(project.id);
    },
    openRunModel: (runId) => {
      const run = get().runs.find((r) => r.id === runId);
      const snap = run?.snapshot;
      if (!run || !snap) return;
      const when = new Date(run.startedAt).toLocaleString();
      const edits = snap.liveEdits.length;
      get().openAsCopy(
        snap.project,
        `${snap.project.name} (run of ${when})`,
        `Opened the model of run '${run.caseName}' (${when}) as an unsaved copy; the project it came from is unchanged.` +
          (edits
            ? ` It is the model as the run started: the ${edits} live edit(s) made during the run are listed in its Run info.`
            : ""),
      );
      if (snap.project.cases.some((c) => c.id === run.caseId)) set({ activeCaseId: run.caseId });
    },
    clearRuns: async () => {
      const { project, log } = get();
      runHistorySeq++; // a load still in flight must not list the runs again
      set({ runs: [], activeRunId: null, overlayRunIds: [], storedRunCount: 0, runsLoading: false });
      if (!project) return;
      try {
        const reply = await api.deleteRuns(project.id);
        log("info", `Deleted ${reply.deleted} stored run(s) of '${project.name}'.`);
      } catch (e) {
        log("error", `Could not delete the stored runs: ${(e as Error).message}`);
        await loadRunHistory(project.id);
      }
    },
  };
});

// Once the model has been checked (Data Checks, or the gate before a run), its
// checks follow it: a quiet re-check runs RECHECK_MS after the last edit, so
// part badges and the status-bar count clear as soon as the problems are
// fixed. A model nobody checked is left alone, and a re-check due while a run
// is in progress waits for the run to end.
const RECHECK_MS = 600;
let recheckTimer: ReturnType<typeof setTimeout> | undefined;
useProjectStore.subscribe((s, prev) => {
  if (s.project !== prev.project && s.dataChecks) scheduleRecheck();
});

function scheduleRecheck(): void {
  clearTimeout(recheckTimer);
  recheckTimer = setTimeout(recheck, RECHECK_MS);
}

async function recheck(): Promise<void> {
  const { project, dataChecks, running } = useProjectStore.getState();
  if (!project || !dataChecks) return;
  if (running) {
    recheckTimer = setTimeout(recheck, RECHECK_MS);
    return;
  }
  try {
    const checks = await api.validateProject(project);
    // an edit made meanwhile has a re-check of its own coming
    if (useProjectStore.getState().project === project) useProjectStore.setState({ dataChecks: checks });
  } catch {
    /* engine unreachable: keep the last checks */
  }
}

/** Ask before `action` (New, Open, Import) replaces a project with unsaved
 *  changes. Resolves true when it may go ahead: nothing was unsaved, the save
 *  worked, or the user chose not to save. A failed save keeps the project
 *  open; its error is in Messages. */
export async function confirmReplaceProject(action: string): Promise<boolean> {
  const { dirty, project } = useProjectStore.getState();
  if (!dirty || !project) return true;
  const choice = await unsavedChangesDialog({
    title: `Save changes to '${project.name}'?`,
    message: `${action} replaces the open project. Changes you don't save are lost.`,
  });
  if (choice !== "save") return choice === "discard";
  await useProjectStore.getState().saveRemote();
  return !useProjectStore.getState().dirty;
}

// -- convenience selectors ----------------------------------------------------

export function useActiveRun(): SimRun | null {
  return useProjectStore((s) => s.runs.find((r) => r.id === s.activeRunId) ?? null);
}

/** Runs overlaid on the active run in Results (in the order they were added).
 *  Derived via useMemo from stable slices so the selector stays cacheable. */
export function useOverlayRuns(): SimRun[] {
  const overlayRunIds = useProjectStore((s) => s.overlayRunIds);
  const runs = useProjectStore((s) => s.runs);
  return useMemo(
    () =>
      overlayRunIds
        .map((id) => runs.find((r) => r.id === id))
        .filter((r): r is SimRun => Boolean(r)),
    [overlayRunIds, runs],
  );
}

export function useActiveSystem(): SystemNode | null {
  return useProjectStore((s) => {
    if (!s.project || !s.activeSystemId) return null;
    return s.project.systems.find((sys) => sys.id === s.activeSystemId) ?? null;
  });
}

export function systemBreadcrumb(project: Project, systemId: string): SystemNode[] {
  const byId = new Map(project.systems.map((s) => [s.id, s]));
  const chain: SystemNode[] = [];
  let cur = byId.get(systemId);
  while (cur) {
    chain.unshift(cur);
    cur = cur.parentId ? byId.get(cur.parentId) : undefined;
  }
  return chain;
}
