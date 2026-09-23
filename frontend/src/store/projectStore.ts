import { useMemo } from "react";
import { create } from "zustand";
import * as api from "../api";
import { loadDraft } from "../persist";
import type {
  Channel,
  ComponentDef,
  Connection,
  DataBusConnection,
  DataCheck,
  ElementInstance,
  LogMessage,
  ParamValue,
  PortDef,
  PortSide,
  Project,
  SimResult,
  SimRun,
  SystemNode,
} from "../types";
import { useUIStore } from "./uiStore";

export function uid(prefix: string): string {
  return `${prefix}-${Math.random().toString(36).slice(2, 9)}`;
}

function now(): string {
  return new Date().toLocaleTimeString([], { hour12: false });
}

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
const MAX_RUNS = 20; // rolling result-history depth (client-side only; holds a full sweep family)

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
    const def = el ? libraryById[el.componentDefId] : undefined;
    if (!el || !def) return null;
    const port =
      def.ports.find((p) => p.id === portId) ??
      el.dynamicPorts?.find((p) => p.id === portId);
    if (!port) return null;
    const unit = unitGroups[port.unitGroup ?? "No Unit"] ?? "-";
    return { elementId, portId, label: `${el.label} · ${port.name}`, unit };
  };
}

// handle for the in-flight live run (not in reactive state on purpose)
let activeRun: api.LiveRunHandle | null = null;
// set by stopRun so an in-flight parameter sweep aborts after the current point
let sweepAborted = false;
// set by stopRun while a run is in flight; reset when the next run starts
let stopRequested = false;

/** Why a finished run is not a complete result, or undefined when it is.
 *  A stop only counts if the solver confirms it cut the run short (a stop
 *  pressed as the run ends leaves a complete result). */
function incompleteReason(result: SimResult, stopped: boolean): string | undefined {
  if (result.status === "failed") return "failed";
  const cancel = stopped ? result.messages.find((m) => /cancel/i.test(m.text)) : undefined;
  if (cancel) {
    const at = cancel.text.match(/t = ([^ ]+ s)/);
    return at ? `stopped at t = ${at[1]}` : "stopped";
  }
  return undefined;
}

interface ProjectState {
  library: ComponentDef[];
  libraryById: Record<string, ComponentDef>;
  /** unitGroup name → display unit (from the backend catalog). */
  unitGroups: Record<string, string>;
  offline: boolean;
  loaded: boolean;

  project: Project | null;
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
  /** rolling history of simulation runs (newest first, capped at MAX_RUNS) */
  runs: SimRun[];
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
  removeElements: (ids: string[]) => void;
  copyElements: (ids: string[]) => void;
  duplicateElements: (ids: string[]) => void;
  pasteClipboard: (position?: { x: number; y: number }) => void;
  clearPendingSelection: () => void;
  renameElement: (id: string, label: string) => void;
  setParameter: (elementId: string, key: string, value: ParamValue) => void;
  setDynamicPorts: (elementId: string, ports: PortDef[]) => void;
  setPortSide: (elementId: string, portId: string, side: PortSide) => void;
  setPortPlacement: (
    elementId: string,
    portId: string,
    side: PortSide,
    offset: number,
  ) => void;
  addConnection: (
    sourceElementId: string,
    sourcePortId: string,
    targetElementId: string,
    targetPortId: string,
  ) => void;
  removeConnections: (ids: string[]) => void;
  addDataBus: (el1: string, p1: string, el2: string, p2: string) => void;
  removeDataBus: (id: string) => void;
  renameSystem: (systemId: string, name: string) => void;

  undo: () => void;
  redo: () => void;

  // project lifecycle
  newProject: () => void;
  openProject: (id: string) => Promise<void>;
  saveRemote: () => Promise<void>;
  exportProject: () => void;
  importProject: (json: string) => void;

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
  /** Sequentially run a case once per swept value, each landing in run history. */
  runSweep: (config: SweepConfig) => Promise<void>;
  stopRun: () => void;
  setActiveRun: (runId: string | null) => void;
  /** Toggle a run in the overlay set (ignored for the active run). */
  toggleOverlayRun: (runId: string) => void;
  /** Replace the overlay set outright (used to overlay a whole sweep family). */
  setOverlayRuns: (runIds: string[]) => void;
  clearOverlays: () => void;
  removeRun: (runId: string) => void;
  clearRuns: () => void;
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

  /**
   * Register a run at the head of the rolling history, stream the live result
   * into it, and resolve with the final SimResult. Shared by `run` (one call)
   * and `runSweep` (one call per swept value). Does NOT run the validation
   * gate, toggle `running`, or switch ribbon tabs — the callers own that.
   */
  async function executeRun(
    projectToRun: Project,
    caseId: string,
    caseName: string,
    extra?: Partial<SimRun>,
  ): Promise<SimResult> {
    const { libraryById, log } = get();
    const runId = uid("run");
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

    const handle = api.runSimulationLive(projectToRun, caseId, {
      onStep: (ev) => {
        buffer.push(ev);
        if (!flushTimer) flushTimer = setTimeout(flush, LIVE_FLUSH_MS);
      },
      onMessage: (m) => log(m.level, m.text),
    });
    activeRun = handle;
    stopRequested = false;
    try {
      const result = await handle.done;
      if (flushTimer) clearTimeout(flushTimer);
      const incomplete = incompleteReason(result, stopRequested);
      set((s) => ({
        runs: s.runs.map((r) =>
          r.id === runId ? { ...r, result, status: result.status, ...(incomplete ? { incomplete } : {}) } : r,
        ),
        activeRunId: runId,
      }));
      return result;
    } catch (e) {
      if (flushTimer) clearTimeout(flushTimer);
      patchRun({ status: "failed", incomplete: "connection lost" });
      throw e;
    } finally {
      activeRun = null;
    }
  }

  return {
    library: [],
    libraryById: {},
    unitGroups: {},
    offline: false,
    loaded: false,
    project: null,
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
    activeCaseId: null,
    activeRunId: null,
    overlayRunIds: [],
    running: false,
    checking: false,
    liveValues: {},
    liveT: 0,
    livePct: 0,

    init: async () => {
      const lib = await api.fetchLibrary();
      const demo = await api.fetchDemoProject();
      const libraryById = Object.fromEntries(lib.components.map((c) => [c.id, c]));
      // restore the autosaved working copy if one exists, else open the demo
      const draft = loadDraft();
      const unsaved = Boolean(draft && !draft.clean);
      let project = draft?.project ?? demo.project;
      if (draft?.clean && !lib.offline) {
        // nothing was unsaved: reopen the project from disk (it may be newer
        // than the kept copy), falling back to the copy if it is gone
        try {
          project = await api.fetchProject(draft.project.id);
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
        project,
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

    removeElements: (ids) => {
      if (ids.length === 0) return;
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
            (c) => !doomedElements.has(c.sourceElementId) && !doomedElements.has(c.targetElementId),
          );
        }
        draft.dataBusConnections = draft.dataBusConnections.filter(
          (d) => !doomedElements.has(d.element1Id) && !doomedElements.has(d.element2Id),
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
      }
    },

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

    addConnection: (sourceElementId, sourcePortId, targetElementId, targetPortId) => {
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
            (c.sourceElementId === sourceElementId &&
              c.sourcePortId === sourcePortId &&
              c.targetElementId === targetElementId &&
              c.targetPortId === targetPortId) ||
            (c.sourceElementId === targetElementId &&
              c.sourcePortId === targetPortId &&
              c.targetElementId === sourceElementId &&
              c.targetPortId === sourcePortId),
        ),
      );
      if (dup) return;
      const { activeSystemId } = get();
      updateProject((draft) => {
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

    removeConnections: (ids) => {
      if (ids.length === 0) return;
      updateProject((draft) => {
        for (const s of draft.systems) {
          s.connections = s.connections.filter((c) => !ids.includes(c.id));
        }
        draft.dataBusConnections = draft.dataBusConnections.filter((d) => !ids.includes(d.id));
      });
    },

    addDataBus: (el1, p1, el2, p2) => {
      const { project, libraryById, log } = get();
      if (!project) return;
      const elements = project.systems.flatMap((s) => s.elements);
      const e1 = elements.find((e) => e.id === el1);
      const e2 = elements.find((e) => e.id === el2);
      const port1 = e1 && libraryById[e1.componentDefId]?.ports.find((p) => p.id === p1);
      const port2 = e2 && libraryById[e2.componentDefId]?.ports.find((p) => p.id === p2);
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
        project: prev,
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
        project: next,
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
        activeSystemId: rootId,
        activeCaseId: caseId,
        activeRunId: null,
        overlayRunIds: [],
        selectedElementId: null,
        past: [],
        future: [],
        runs: [],
        dataChecks: null,
        dirty: false,
      });
      get().log("info", "New project created.");
    },

    openProject: async (id) => {
      try {
        const project = await api.fetchProject(id);
        set({
          project,
          activeSystemId: project.systems.find((s) => s.parentId === null)?.id ?? project.systems[0]?.id,
          activeCaseId: project.cases[0]?.id ?? null,
          activeRunId: null,
          overlayRunIds: [],
          selectedElementId: null,
          past: [],
          future: [],
          runs: [],
          dataChecks: null,
          dirty: false,
        });
        get().log("info", `Project '${project.name}' opened.`);
      } catch (e) {
        get().log("error", `Failed to open project: ${(e as Error).message}`);
      }
    },

    saveRemote: async () => {
      const { project, log } = get();
      if (!project) return;
      try {
        await api.saveProject(project);
        set({ dirty: false });
        log("info", `Project '${project.name}' saved to the server.`);
      } catch (e) {
        log("error", `Save failed: ${(e as Error).message}. Use Export to download the project file instead.`);
      }
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
        const project = JSON.parse(json) as Project;
        if (!project.id || !Array.isArray(project.systems)) {
          throw new Error("not a SimStudio project file");
        }
        project.dataBusConnections ??= [];
        project.cases ??= [];
        set({
          project,
          activeSystemId: project.systems.find((s) => s.parentId === null)?.id ?? project.systems[0]?.id,
          activeCaseId: project.cases[0]?.id ?? null,
          activeRunId: null,
          overlayRunIds: [],
          selectedElementId: null,
          past: [],
          future: [],
          runs: [],
          dataChecks: null,
          dirty: true,
        });
        get().log("info", `Project '${project.name}' imported.`);
      } catch (e) {
        get().log("error", `Import failed: ${(e as Error).message}`);
      }
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

    /** Error-level data-check gate shared by run + runSweep. */
    passesRunGate: async () => {
      const { project, log } = get();
      if (!project) return false;
      try {
        const checks = await api.validateProject(project);
        set({ dataChecks: checks });
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
        stopRequested = true;
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
    removeRun: (runId) =>
      set((s) => {
        const runs = s.runs.filter((r) => r.id !== runId);
        return {
          runs,
          activeRunId: s.activeRunId === runId ? (runs[0]?.id ?? null) : s.activeRunId,
          overlayRunIds: s.overlayRunIds.filter((id) => id !== runId),
        };
      }),
    clearRuns: () => set({ runs: [], activeRunId: null, overlayRunIds: [] }),
  };
});

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
