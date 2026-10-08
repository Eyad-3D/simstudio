import { useMemo } from "react";
import { create } from "zustand";
import * as api from "../api";
import { confirmDialog, scriptTrustDialog, unsavedChangesDialog } from "../dialog";
import { desktop } from "../desktop";
import { loadDraft } from "../persist";
import { diffSnapshots, modelFingerprint, nameFromChanges } from "../provenance";
import { dutyKpis } from "../reports";
import type {
  Attachment,
  Channel,
  ComponentDef,
  Connection,
  DataBusConnection,
  DataCheck,
  ElementInstance,
  ExampleCard,
  LegacyProject,
  LiveEdit,
  LogMessage,
  OutsidePolicy,
  ParamValue,
  ParameterSource,
  PortDef,
  PortSide,
  Project,
  ReferenceValue,
  RunSnapshot,
  SimCase,
  SimResult,
  SimRun,
  StoredRunInfo,
  Study,
  StudyPoint,
  SystemNode,
} from "../types";
import { rangeProblem } from "../paramRules";
import { FS_EVENTS, type FsEvent } from "../fsEvents";
import { codeOf, fingerprintOf } from "../trust";
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

/** Split a project read from disk into the project and the bookkeeping that
 *  comes with it: its file revision, where it is (a .lightsim file outside
 *  the projects folder) and why it is read-only (a newer LightSim's file). */
function fromDisk(stored: api.StoredProject): {
  project: Project;
  revision: string | null;
  filePath: string | null;
  readOnly: string | null;
  upgradedFrom: number | null;
} {
  const { revision, filePath, readOnly, upgradedFrom, ...project } = stored;
  return {
    project,
    revision: revision ?? null,
    filePath: filePath ?? null,
    readOnly: readOnly ?? null,
    upgradedFrom: upgradedFrom ?? null,
  };
}

/** A project from before 0.3.0 may carry its studies; they now live with the
 *  runs (PLT-34). Returns the project without them, and them. */
function splitStudies(project: LegacyProject): { project: Project; studies: Study[] } {
  const { studies, ...rest } = project;
  return { project: rest, studies: Array.isArray(studies) ? studies : [] };
}

/** What a project opened afresh starts with: no studies (they load with its
 *  runs), not a file outside the projects folder, not read-only. */
const NO_FILE = { studies: [] as Study[], filePath: null, readOnly: null };

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
/** Script code typed in this window (PLT-35). The user wrote it, so it is
 *  approved without asking when it runs or is saved. Bounded: the oldest
 *  versions of long edits are dropped. */
const typedScripts = new Set<string>();
/** Saving keeps the scripts typed here approved, so reopening the project
 *  later does not ask about the user's own code. Best effort. */
async function approveTypedScripts(project: Project) {
  const codes = project.systems
    .flatMap((s) => s.elements)
    .map((e) => e.parameterOverrides?.code)
    .filter((c): c is string => typeof c === "string" && typedScripts.has(c));
  if (codes.length) await api.approveScripts(codes).catch(() => {});
}
function rememberTyped(code: string) {
  typedScripts.add(code);
  if (typedScripts.size > 500) typedScripts.delete(typedScripts.values().next().value as string);
}

let activeRun: api.LiveRunHandle | null = null;
// live parameter edits sent to the in-flight run, for its snapshot
let liveLog: { runId: string; edits: LiveEdit[] } | null = null;
// the sweep running in the engine's worker processes (ENG-05); stopRun stops it
let activeStudy: api.StudyHandle | null = null;
// bumped per run-history load, so an answer for an earlier load is dropped
let runHistorySeq = 0;
// The project (by id) whose code the user is trusted to change in this
// session: one made here, an example, or one whose code the user agreed to
// run (STD-02). Code edited in the app since is the user's own and runs
// without asking again; a project opened, imported or reloaded from disk
// clears it, so code that came from outside is asked about.
let trustedProject: string | null = null;
// a revision of the open project's file already offered for reload (checkDisk)
let offeredRevision: string | null = null;
let diskCheckBusy = false;

/** The run Results opens on: the newest complete run that is not a sweep
 *  point, or the newest run when there is none (runs are newest first). */
function mainRunOf<R extends Pick<SimRun, "status" | "incomplete" | "sweepId">>(runs: R[]): R | undefined {
  return runs.find((r) => !r.incomplete && r.status !== "failed" && !r.sweepId) ?? runs[0];
}

/** The previous run of a case: its newest run that started before `before`
 *  and finished normally (not running, stopped or failed), sweep points
 *  included; runs are newest first. A re-run is named by what changed since
 *  it and compared with it (RES-10, RES-19). */
export function previousRunOf(caseId: string, runs: SimRun[], before = Infinity): SimRun | undefined {
  return runs.find(
    (r) => r.caseId === caseId && r.startedAt < before && r.status !== "running" && r.status !== "failed" && !r.incomplete,
  );
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

export interface ProjectState {
  library: ComponentDef[];
  libraryById: Record<string, ComponentDef>;
  /** unitGroup name → display unit (from the backend catalog). */
  unitGroups: Record<string, string>;
  offline: boolean;
  loaded: boolean;
  /** the app's version as the engine reports it (recorded with each run) */
  appVersion: string | null;
  /** the standard drive cycles the engine bundles (CON-16) */
  cycles: api.CycleInfo[];

  project: Project | null;
  /** Revision of the project file the open copy was loaded from or last saved
   *  as; a save is refused if the file changed since. null: not on disk yet. */
  revision: string | null;
  /** Where the open project's .lightsim file is when it is one outside the
   *  projects folder (PLT-33), else null. */
  filePath: string | null;
  /** Why the open project must not be saved over (a file from a newer
   *  LightSim, PLT-07), else null. */
  readOnly: string | null;
  /** The open project's parameter studies, oldest first; kept with its runs,
   *  not in the model file (PLT-34). */
  studies: Study[];
  /** The example the open project is an unsaved copy of (its id), else null.
   *  Save keeps the copy as a new project; the example is never written. */
  exampleId: string | null;
  activeSystemId: string | null;
  selectedElementId: string | null;
  dirty: boolean;

  /** in-memory element clipboard (copy/paste); not persisted or in undo history */
  clipboard: ClipboardData | null;

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
  /** Both return the new elements' ids, for the canvas to select. */
  duplicateElements: (ids: string[]) => string[];
  pasteClipboard: (position?: { x: number; y: number }) => string[];
  renameElement: (id: string, label: string) => void;
  setParameter: (elementId: string, key: string, value: ParamValue) => void;
  /** Several parameters at once (a component preset), as one undo step. */
  setParameters: (elementId: string, values: Record<string, ParamValue>) => void;
  /** Record where a parameter value comes from (CON-13); null forgets it. */
  setParameterSource: (elementId: string, key: string, source: ParameterSource | null) => void;
  /** Values for parameters of several parts (an imported parameter sheet,
   *  STD-36), as one undo step. */
  applyParameterChanges: (changes: { elementId: string; key: string; value: ParamValue }[]) => void;
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
  /** Link a signal output to an input; with `replaceId`, the new link takes
   *  that one's place in the same undo step. A link between two inputs or two
   *  outputs is refused with the reason. */
  addDataBus: (el1: string, p1: string, el2: string, p2: string, replaceId?: string) => void;
  removeDataBus: (id: string) => void;
  renameSystem: (systemId: string, name: string) => void;
  /** Set or change the project's card (CON-15); null removes it. */
  setCard: (card: ExampleCard | null) => void;

  undo: () => void;
  redo: () => void;

  // project lifecycle
  newProject: () => void;
  openProject: (id: string) => Promise<void>;
  /** Open an example as an unsaved copy with an id of its own. */
  openExample: (id: string) => Promise<void>;
  /** Open a project made from a template (CON-18), unsaved. */
  openNewProject: (project: Project, message: string) => void;
  /** Leave an example out of the Open menu; resolves false when that failed. */
  hideExample: (id: string, name: string) => Promise<boolean>;
  /** Show every hidden example in the Open menu again. */
  restoreExamples: () => Promise<void>;
  saveRemote: () => Promise<void>;
  /** Save the project as a .lightsim file the user picks (desktop app only,
   *  PLT-33); resolves true once saved. */
  saveAs: () => Promise<boolean>;
  /** Pick a .lightsim file in the system's Open dialog and open it (desktop app). */
  openFile: () => Promise<void>;
  /** Take a file off Recent files (the file itself stays). */
  forgetFile: (id: string) => Promise<void>;
  /** Notice the open project's file changing on disk (a git pull, another
   *  window) and offer to reload it; the app calls this every few seconds. */
  checkDisk: () => Promise<void>;
  exportProject: () => Promise<void>;
  /** Open a project file's JSON as a new, unsaved project (upgraded by the
   *  engine to the current format first). */
  importProject: (json: string) => Promise<void>;
  /** Open a zip bundle (a project with its attached files). */
  importBundle: (zip: Blob) => Promise<void>;
  /** Attach a file to the project (STD-02); resolves with its path, or null. */
  attachFile: (file: File) => Promise<string | null>;
  /** Remove an attached file from the project and its resources folder. */
  detachFile: (path: string) => Promise<void>;
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
      kind: "cycle" | "performance" | "acceleration" | "lap";
      endDistance: number | null;
      endLaps: number | null;
      chargeBalance: boolean | null;
      startLine: number;
      referenceTime: number | null;
      energyReport: boolean;
      fsEvent: FsEvent | null;
      referenceEnergy: number | null;
      referenceEnergyTime: number | null;
      references: ReferenceValue[];
    }>,
  ) => void;
  addCase: () => void;
  duplicateCase: (id: string) => void;
  removeCase: (id: string) => void;
  /** Set a per-case parameter override (elementId.key = value for this case only). */
  setCaseOverride: (caseId: string, elementId: string, key: string, value: ParamValue) => void;
  /** Remove a per-case parameter override; prunes the element entry when empty. */
  clearCaseOverride: (caseId: string, elementId: string, key: string) => void;
  /** Point a Driving Task at a bundled drive cycle ("" = its typed profile),
   *  or with `caseId` only that case. The cycle cases that then drive it take
   *  the cycle's length, in the same undo step. */
  setDrivingCycle: (elementId: string, cycleId: string, caseId?: string) => void;
  runDataChecks: () => Promise<DataCheck[]>;
  /** Error-level data-check gate for a run of `caseId` (errors about the
   *  model, and about that case: not another case's own values or kind),
   *  and the one-time question whether the user trusts the code the project
   *  carries; resolves true when a run/sweep may proceed. */
  passesRunGate: (caseId: string) => Promise<boolean>;
  /** Show the project's scripts that this user has not approved and ask
   *  before they run (PLT-35); resolves true when nothing is left to ask.
   *  `when` "open" offers Open without running scripts, "run" Don't run. */
  reviewScripts: (when: "open" | "run") => Promise<boolean>;
  run: () => Promise<void>;
  /** The one-click Formula Student acceleration test: select the first
   *  acceleration case (adding a 75 m one if there is none) and run it. */
  runAccelerationTest: () => Promise<void>;
  /** The one-click Formula Student events (MOD-43): mark or add an
   *  Acceleration, Skidpad, Autocross and Endurance case (one undo) and run
   *  the four; their points show in Cases & Parameters. */
  runFsEvents: () => Promise<void>;
  /** Add a cycle case that drives a lap read from a logger or lap simulator
   *  file (STD-35), adding a Driving Task wired to the Driver when the model
   *  has none (one undo). */
  addImportedLap: (lap: api.LapLogResult, name: string, sha256: string) => void;
  /** The endurance energy study (STU-38): run an endurance case at every
   *  pair of a battery's capacity and Output Power Limit, and save the grid
   *  as a two-factor study of the project. */
  runEnduranceStudy: (args: { caseId: string; batteryId: string; packs: number[]; caps: number[] }) => Promise<void>;
  /** Apply a lap mode calibration (VAL-38): every wheel's μ, lateral μ and
   *  load sensitivity times `muScale`, and the Vehicle's CzA (one undo). */
  applyLapCalibration: (muScale: number, cza: number) => void;
  /** Sequentially run a case once per swept value, each landing in run
   *  history; the study and its results table are kept with the runs. */
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
  /** Set a run's name or note (trimmed; empty removes it) and store it
   *  again; nothing while a run is going. */
  editRun: (runId: string, edit: { name?: string; note?: string }) => void;
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
    void loadStudies(projectId);
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

  /** Whether the attached code in `project` (FMUs, AI models, programs)
   *  may run (STD-02): none, code the user trusted before (by fingerprint),
   *  their own edits to a trusted project, or a yes to the one-time question
   *  now. Script blocks are reviewed by reviewScripts (PLT-35). */
  async function trustsCode(project: Project): Promise<boolean> {
    const code = codeOf(project, get().libraryById);
    if (!code) return true;
    let fingerprint: string;
    try {
      fingerprint = await fingerprintOf(code);
    } catch {
      return true; // no Web Crypto (a plain-http page): nothing to remember it by
    }
    const remember = () => api.trustFingerprint(fingerprint)?.catch?.(() => undefined);
    if (trustedProject === project.id) {
      void remember();
      return true;
    }
    try {
      if (await api.isTrusted(fingerprint)) {
        trustedProject = project.id;
        return true;
      }
    } catch {
      /* ask */
    }
    const yes = await confirmDialog({
      title: "Run this project's code?",
      message:
        `'${project.name}' carries code that runs on your computer when the model runs: ${code.items.join(", ")}. ` +
        "Run it only if you trust where the project came from. LightSim asks once; it asks again when that code changes.",
      confirmLabel: "Trust and run",
      cancelLabel: "Cancel",
    });
    if (!yes) return false;
    trustedProject = project.id;
    await remember();
    return true;
  }

  /** Open an imported project as a new, unsaved one. */
  function openImported(project: Project, studies: Study[], readOnly: string | null, note: string): void {
    project.dataBusConnections ??= [];
    project.cases ??= [];
    runHistorySeq++;
    trustedProject = null;
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
      ...NO_FILE,
      readOnly,
      dataChecks: null,
      dirty: true,
    });
    get().log("info", note);
    adoptStudies(project.id, studies);
    void loadRunHistory(project.id);
    // scripts that came from another computer: show their code now (PLT-35)
    void get().reviewScripts("open");
  }

  /** List the project's studies (kept with its runs, PLT-34), keeping any
   *  made in this session that the engine does not have (a failed store). */
  async function loadStudies(projectId: string): Promise<void> {
    let stored: Study[];
    try {
      stored = (await api.listStudies(projectId)) ?? [];
    } catch {
      return; // the studies list stays as it is; the runs warning covers it
    }
    if (get().project?.id !== projectId) return;
    set((s) => {
      const ids = new Set(stored.map((st) => st.id));
      const extra = s.studies.filter((st) => !ids.has(st.id));
      return { studies: [...stored, ...extra].sort((a, b) => a.startedAt - b.startedAt) };
    });
  }

  /** Keep studies with the project's runs (an old file's or a draft's). */
  function adoptStudies(projectId: string, studies: Study[]): void {
    if (studies.length === 0) return;
    set((s) => ({ studies: [...s.studies.filter((st) => !studies.some((n) => n.id === st.id)), ...studies] }));
    for (const study of studies) {
      api.storeStudy(projectId, study)?.catch?.((e: Error) =>
        get().log("warning", `Study could not be stored with the runs (${e.message}); it is kept for this session only.`),
      );
    }
  }

  /** Say what opening a file from disk found: an older format upgraded, or
   *  a newer LightSim's file that stays read-only. */
  function noteFormat(name: string, upgradedFrom: number | null, readOnly: string | null): void {
    const { log } = get();
    if (readOnly) log("warning", `'${name}' is read-only. ${readOnly}`);
    else if (upgradedFrom !== null)
      log(
        "info",
        `'${name}' was saved in an older file format (${upgradedFrom}); LightSim upgraded it. ` +
          "Saving writes the new format and keeps the old file in its backups (Project → Restore…).",
      );
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
    // the engine runs the model and the snapshot keeps it
    const model = projectToRun;
    const snapshot: RunSnapshot | undefined = simCase && {
      project: model,
      case: simCase,
      appVersion,
      liveEdits: [],
    };
    const fingerprint = modelFingerprint(model).catch(() => undefined);
    // named by what changed since the previous run of its case (a sweep
    // point is named by its swept value)
    const prev = extra?.sweepId ? undefined : previousRunOf(caseId, get().runs);
    const name = prev?.snapshot && snapshot && nameFromChanges(diffSnapshots(prev.snapshot, snapshot, libraryById));
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
      ...(name ? { name } : {}),
      ...extra,
    };
    set((s) => {
      const runs = [newRun, ...s.runs].slice(0, MAX_RUNS);
      const ids = new Set(runs.map((r) => r.id));
      return {
        runs,
        activeRunId: runId,
        // a run that dropped out of the list is no longer overlaid
        overlayRunIds: s.overlayRunIds.filter((id) => ids.has(id)),
        liveValues: {},
        liveT: 0,
        livePct: 0,
      };
    });

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
      // new channel objects each flush, so views memoised on a channel (the
      // Signal Plot) redraw; the series arrays grow in place and stay shared,
      // as copying them would cost every sample on every flush
      const result = { ...partial, channels: partial.channels.map((c) => ({ ...c })) };
      set((s) => ({
        runs: s.runs.map((r) => (r.id === runId ? { ...r, result } : r)),
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
    cycles: [],
    project: null,
    revision: null,
    filePath: null,
    readOnly: null,
    studies: [],
    exampleId: null,
    activeSystemId: null,
    selectedElementId: null,
    dirty: false,
    clipboard: null,
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
      const [lib, appVersion, cycles] = await Promise.all([
        api.fetchLibrary(),
        api.fetchVersion(),
        api.listCycles(),
      ]);
      const demo = await api.fetchDemoProject();
      const libraryById = Object.fromEntries(lib.components.map((c) => [c.id, c]));
      // restore the autosaved working copy if one exists, else open the demo
      // example as a copy
      const draft = loadDraft();
      const unsaved = Boolean(draft && !draft.clean);
      const exampleId = draft ? (draft.example ?? null) : demo.project.id;
      const legacy = splitStudies(draft ? (draft.project as LegacyProject) : exampleCopy(demo.project));
      let project = legacy.project;
      let revision: string | null = draft ? (draft.revision ?? null) : null;
      let filePath: string | null = null;
      let readOnly: string | null = null;
      if (draft?.clean && !lib.offline) {
        // nothing was unsaved: reopen the project from disk (it may be newer
        // than the kept copy), or an example's current version (an update may
        // have corrected it) under the copy's id, which its runs are stored
        // under; falling back to the copy if it is gone
        try {
          if (exampleId) {
            project = { ...(await api.fetchExample(exampleId)), id: draft.project.id };
          } else {
            ({ project, revision, filePath, readOnly } = fromDisk(await api.fetchProject(draft.project.id)));
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
        cycles,
        project,
        revision,
        filePath,
        readOnly,
        exampleId,
        activeSystemId: rootSystemOf(project).id,
        activeCaseId: project.cases[0]?.id ?? null,
        dirty: unsaved,
      });
      trustedProject = exampleId ? project.id : null;
      if (!lib.offline) adoptStudies(project.id, legacy.studies);
      const log = get().log;
      log("info", `Component library loaded (${lib.components.length} components).`);
      if (draft && unsaved) {
        useUIStore.getState().setRibbonTab("home"); // restored work is shown, not the Start page
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

    log: (level, text) => {
      set((s) => ({ messages: [...s.messages, { level, text, time: now() }] }));
      // Messages has no badge (Problems counts the model's problems), so an
      // error such as a failed save shows at once
      if (level === "error") useUIStore.getState().focusPanel("messages");
    },
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
      if (!project || !activeSystemId || ids.length === 0) return [];
      const data = collectSelection(project, activeSystemId, ids);
      let newIds: string[] = [];
      updateProject((draft) => {
        newIds = cloneElementsInto(draft, activeSystemId, data, { x: 28, y: 28 });
      });
      if (newIds.length) set({ selectedElementId: newIds[newIds.length - 1] });
      return newIds;
    },

    pasteClipboard: (position) => {
      const { project, activeSystemId, clipboard } = get();
      if (!project || !activeSystemId || !clipboard || clipboard.elements.length === 0) return [];
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
      if (newIds.length) set({ selectedElementId: newIds[newIds.length - 1] });
      return newIds;
    },

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
      if (key === "code" && typeof value === "string") rememberTyped(value);
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
        // a number outside its limits waits for one inside (Data Checks refuse it)
        const { project, libraryById } = get();
        const el = project?.systems.flatMap((s) => s.elements).find((e) => e.id === elementId);
        const pdef = el && libraryById[el.componentDefId]?.parameters.find((p) => p.key === key);
        if (pdef?.type === "number" && rangeProblem(pdef, Number(value))) return;
        activeRun.setParam(elementId, key, value);
        logLiveEdit({ t: get().liveT, elementId, key, value });
      }
    },

    // presets set "fixed" parameters only, so nothing streams into a live run
    setParameters: (elementId, values) =>
      updateProject((draft) => {
        for (const s of draft.systems) {
          const el = s.elements.find((e) => e.id === elementId);
          if (el) Object.assign(el.parameterOverrides, values);
        }
      }),

    setParameterSource: (elementId, key, source) =>
      updateProject(
        (draft) => {
          for (const s of draft.systems) {
            const el = s.elements.find((e) => e.id === elementId);
            if (!el) continue;
            const next = { ...el.parameterSources };
            if (source) next[key] = source;
            else delete next[key];
            el.parameterSources = next;
          }
        },
        true,
        `source:${elementId}:${key}`,
      ),
    applyParameterChanges: (changes) =>
      updateProject((draft) => {
        for (const s of draft.systems)
          for (const el of s.elements)
            for (const c of changes) if (c.elementId === el.id) el.parameterOverrides[c.key] = c.value;
      }),

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

    addDataBus: (el1, p1, el2, p2, replaceId) => {
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
          "error",
          `'${e1.label} · ${port1.name}' and '${e2.label} · ${port2.name}' are both ${port1.direction}s: ` +
            "a signal runs from an output to an input, so no data would flow. Not connected.",
        );
        return;
      }
      const dup = project.dataBusConnections.find(
        (d) =>
          (d.element1Id === el1 && d.port1Id === p1 && d.element2Id === el2 && d.port2Id === p2) ||
          (d.element1Id === el2 && d.port1Id === p2 && d.element2Id === el1 && d.port2Id === p1),
      );
      if (dup) {
        // picking the source an input already has changes nothing
        if (dup.id !== replaceId) log("info", "This data bus connection already exists.");
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
      const rest = replaceId
        ? { ...project, dataBusConnections: project.dataBusConnections.filter((d) => d.id !== replaceId) }
        : project;
      const existing = inEl && inPort && signalSourceOf(rest, libraryById, inEl.id, inPort.id);
      if (inEl && inPort && existing) {
        log(
          "error",
          `'${inEl.label}.${inPort.name}' already takes its signal from '${existing}' — an input ` +
            "can have only one source. Remove that link first to connect a different one.",
        );
        return;
      }
      updateProject((draft) => {
        if (replaceId) draft.dataBusConnections = draft.dataBusConnections.filter((d) => d.id !== replaceId);
        draft.dataBusConnections.push({
          id: uid("dbc"),
          element1Id: el1,
          port1Id: p1,
          element2Id: el2,
          port2Id: p2,
        });
      });
      const end = (e: ElementInstance, p: PortDef) => `${e.label} · ${p.name}`;
      const [from, to] =
        port1.direction === "output" ? [end(e1, port1), end(e2, port2)] : [end(e2, port2), end(e1, port1)];
      log("info", `Data bus: ${from} → ${to} connected.`);
    },

    removeDataBus: (id) =>
      updateProject((draft) => {
        draft.dataBusConnections = draft.dataBusConnections.filter((d) => d.id !== id);
      }),

    setCard: (card) =>
      updateProject(
        (draft) => {
          draft.card = card;
        },
        true,
        "card",
      ),

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
        ...NO_FILE,
        dataChecks: null,
        dirty: false,
      });
      trustedProject = project.id;
      get().log("info", "New project created.");
    },

    openProject: async (id) => {
      try {
        const { project, revision, filePath, readOnly, upgradedFrom } = fromDisk(await api.fetchProject(id));
        runHistorySeq++; // runs still loading for the project it replaces are dropped
        trustedProject = null;
        offeredRevision = null;
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
          ...NO_FILE,
          dataChecks: null,
          dirty: false,
        });
        set({ filePath, readOnly });
        get().log("info", filePath ? `Project '${project.name}' opened from ${filePath}.` : `Project '${project.name}' opened.`);
        noteFormat(project.name, upgradedFrom, readOnly);
        void loadRunHistory(project.id);
        void get().reviewScripts("open");
      } catch (e) {
        get().log("error", `Failed to open project: ${(e as Error).message}`);
      }
    },

    openNewProject: (project, message) => {
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
      get().log("info", message);
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
        ...NO_FILE,
        dataChecks: null,
        dirty: false,
      });
      trustedProject = project.id; // shipped with the app
      get().log(
        "info",
        `Example '${project.name}' opened as a copy. Save keeps it as a new project of yours; the example stays as it is.`,
      );
      // its stored reference results, so Results has something to show at
      // once (CON-15); a Run recomputes them
      let stored: api.StoredReference[];
      try {
        stored = await api.fetchExampleReference(id);
      } catch {
        return; // an engine without them: the Results page starts empty
      }
      if (get().project?.id !== project.id || stored.length === 0) return;
      const runs: SimRun[] = stored.flatMap((s) => {
        const c = project.cases.find((cc) => cc.id === s.caseId);
        if (!c) return [];
        return [
          {
            id: `stored-${s.caseId}`,
            caseId: s.caseId,
            caseName: c.name,
            startedAt: Date.parse(s.creation.date) || 0,
            status: s.result.status,
            result: s.result,
            name: "Stored result",
            note: `Stored with LightSim ${s.creation.appVersion} on ${s.creation.date}: only the comparison signals are kept. Press Run to recompute.`,
            snapshot: { project, case: c, appVersion: s.creation.appVersion, liveEdits: [] },
          },
        ];
      });
      set((st) => {
        const own = new Set(st.runs.map((r) => r.caseId));
        const add = runs.filter((r) => !own.has(r.caseId));
        const all = [...st.runs, ...add];
        return {
          runs: all,
          activeRunId: st.activeRunId ?? add.find((r) => r.caseId === st.activeCaseId)?.id ?? null,
        };
      });
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
        const { project, revision, exampleId, log, readOnly, filePath } = get();
        if (!project) return;
        if (readOnly) {
          log("error", `Not saved: ${readOnly}`);
          return;
        }
        const where = filePath ? ` to ${filePath}` : " to the server";
        try {
          const res = await api.saveProject(project, revision);
          void approveTypedScripts(project);
          // an edit made while the save was in flight is still unsaved
          set({ dirty: get().project !== project, revision: res.revision ?? revision, exampleId: null });
          offeredRevision = null;
          log(
            "info",
            exampleId
              ? `Project '${project.name}' saved${where} as a new project; the example it was copied from is unchanged.`
              : `Project '${project.name}' saved${where}.`,
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
            log("info", `Project '${project.name}' saved${where}, replacing the version on disk ` +
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

    saveAs: async () => {
      const { project, log, readOnly } = get();
      const shell = desktop();
      if (!project || !shell) return false;
      let picked;
      try {
        picked = await shell.saveFileAs(project.id, project.name);
      } catch (e) {
        log("error", `Save As failed: ${(e as Error).message}`);
        return false;
      }
      if (!picked) return false; // cancelled
      const saved = { ...project, id: picked.id };
      try {
        // the system's Save dialog already asked before replacing a file
        const res = await api.saveProject(saved);
        const sameProject = picked.id === project.id;
        runHistorySeq++;
        offeredRevision = null;
        set((s) => ({
          project: s.project === project ? saved : { ...s.project!, id: picked.id },
          revision: res.revision ?? null,
          filePath: picked.path,
          readOnly: null,
          exampleId: null,
          dirty: s.project !== project,
          ...(sameProject ? {} : { runs: [], storedRunCount: 0, studies: [], activeRunId: null, overlayRunIds: [] }),
        }));
        if (trustedProject === project.id) trustedProject = picked.id;
        log(
          "info",
          `Project '${project.name}' saved as ${picked.path}.` +
            (readOnly ? " It is in this LightSim's file format now." : "") +
            (sameProject ? "" : " It is a copy: the runs stay with the project it came from."),
        );
        void loadRunHistory(picked.id);
        return true;
      } catch (e) {
        log("error", `Save As failed: ${(e as Error).message}`);
        return false;
      }
    },

    openFile: async () => {
      const shell = desktop();
      if (!shell) return;
      try {
        const picked = await shell.openFile();
        if (picked) await get().openProject(picked.id);
      } catch (e) {
        get().log("error", `Could not open the file: ${(e as Error).message}`);
      }
    },

    forgetFile: async (id) => {
      try {
        await api.forgetFile(id);
      } catch (e) {
        get().log("error", `Could not remove it from Recent files: ${(e as Error).message}`);
      }
    },

    checkDisk: async () => {
      const { project, revision, offline } = get();
      if (!project || !revision || offline || diskCheckBusy) return;
      diskCheckBusy = true;
      try {
        await saveQueue; // a save in flight changes the file itself
        if (get().project !== project && get().project?.id !== project.id) return;
        const onDisk = await api.fetchRevision(project.id);
        const now = get();
        if (now.project?.id !== project.id || now.revision !== revision) return; // saved or reopened meanwhile
        if (onDisk === revision || onDisk === offeredRevision) return;
        offeredRevision = onDisk;
        if (onDisk === null) {
          now.log("warning", `The file of '${project.name}' was deleted or moved on disk; Save writes it again.`);
          return;
        }
        const reload = await confirmDialog({
          title: "Project changed on disk",
          message:
            `'${project.name}' was changed on disk (for example by a git pull, or saved from another window). ` +
            (now.dirty
              ? "Reload it to see that version: your unsaved changes here would be lost. Keep editing to save yours over it later (Save asks first)."
              : "Reload it to see that version?"),
          confirmLabel: "Reload",
          cancelLabel: now.dirty ? "Keep my changes" : "Not now",
          danger: now.dirty,
        });
        if (reload && get().project?.id === project.id) {
          const keepCase = get().activeCaseId;
          await get().openProject(project.id);
          if (get().project?.cases.some((c) => c.id === keepCase)) set({ activeCaseId: keepCase });
        }
      } catch {
        /* the engine is busy or gone: try again next time */
      } finally {
        diskCheckBusy = false;
      }
    },

    exportProject: async () => {
      const { project, log } = get();
      if (!project) return;
      const download = (blob: Blob, name: string) => {
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = name;
        a.click();
        URL.revokeObjectURL(url);
      };
      if ((project.attachments ?? []).length > 0) {
        try {
          download(await api.exportBundle(project), `${project.id}.lightsim.zip`);
          log("info", `Project exported with its attached files as ${project.id}.lightsim.zip.`);
          return;
        } catch (e) {
          log("warning", `The attached files could not be packed (${(e as Error).message}); exporting the project file only.`);
        }
      }
      download(new Blob([JSON.stringify(project, null, 2)], { type: "application/json" }), `${project.id}.lightsim`);
      log("info", `Project exported as ${project.id}.lightsim.`);
    },

    importProject: async (json) => {
      const { log, offline } = get();
      let raw: api.StoredProject;
      try {
        raw = JSON.parse(json) as api.StoredProject;
        if (!raw || typeof raw !== "object" || !raw.id || !Array.isArray(raw.systems)) {
          throw new Error("not a LightSim project file");
        }
      } catch (e) {
        log("error", `Import failed: ${(e as Error).message}`);
        return;
      }
      // a file saved from the engine's API may carry bookkeeping: drop it
      const { project: parsed } = fromDisk(raw);
      let reply: api.UpgradeReply | undefined;
      if (!offline) {
        try {
          reply = await api.upgradeProject(parsed);
        } catch (e) {
          log("error", `Import failed: ${(e as Error).message}`);
          return;
        }
      }
      // offline (no engine): open it as it is; the engine upgrades it on save
      const { project, studies } = reply ? { project: reply.project, studies: reply.studies } : splitStudies(parsed);
      openImported(project, studies, reply?.readOnly ?? null, `Project '${project.name}' imported.`);
      noteFormat(project.name, reply?.upgradedFrom ?? null, reply?.readOnly ?? null);
    },

    importBundle: async (zip) => {
      let reply: api.UpgradeReply;
      try {
        reply = await api.importBundle(zip);
      } catch (e) {
        get().log("error", `Import failed: ${(e as Error).message}`);
        return;
      }
      const n = reply.project.attachments?.length ?? 0;
      openImported(reply.project, reply.studies, reply.readOnly, `Project '${reply.project.name}' imported with ${n} attached file(s).`);
      noteFormat(reply.project.name, reply.upgradedFrom, reply.readOnly);
    },

    attachFile: async (file) => {
      const { project, log, readOnly } = get();
      if (!project || readOnly) return null;
      let info: api.AttachedFile;
      try {
        info = await api.uploadAttachment(project.id, file, file.name);
      } catch (e) {
        log("error", `Could not attach '${file.name}': ${(e as Error).message}`);
        return null;
      }
      if (get().project?.id !== project.id) return null;
      const ref: Attachment = { path: info.path, sha256: info.sha256, bytes: info.bytes };
      updateProject((draft) => {
        draft.attachments = [...(draft.attachments ?? []).filter((a) => a.path !== ref.path), ref];
      });
      log(
        "info",
        `Attached '${info.name}' (${(info.bytes / 1024).toLocaleString("en", { maximumFractionDigits: 0 })} kB) to the project` +
          (info.name !== file.name ? ` as '${info.name}'` : "") +
          "; save the project to keep it in the list.",
      );
      return info.path;
    },

    detachFile: async (path) => {
      const { project, log } = get();
      if (!project) return;
      updateProject((draft) => {
        draft.attachments = (draft.attachments ?? []).filter((a) => a.path !== path);
      });
      try {
        await api.deleteAttachment(project.id, path.replace(/^resources\//, ""));
        log("info", `Removed '${path}' from the project and its resources folder.`);
      } catch (e) {
        if (!(e as Error).message.startsWith("404"))
          log("warning", `'${path}' is off the project's list, but the file could not be deleted: ${(e as Error).message}`);
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
        ...NO_FILE,
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

    setDrivingCycle: (elementId, cycleId, caseId) => {
      const cycle = get().cycles.find((c) => c.id === cycleId);
      const resized: string[] = [];
      updateProject((draft) => {
        if (caseId) {
          const c = draft.cases.find((cc) => cc.id === caseId);
          if (c)
            c.parameterOverrides = {
              ...c.parameterOverrides,
              [elementId]: { ...c.parameterOverrides?.[elementId], cycle: cycleId },
            };
        } else {
          for (const s of draft.systems) {
            const el = s.elements.find((e) => e.id === elementId);
            if (el) el.parameterOverrides.cycle = cycleId;
          }
        }
        // the case length follows the cycle only when it is clear which task
        // a case drives: the model's only one, or the one the case names
        const tasks = draft.systems
          .flatMap((sy) => sy.elements)
          .filter((e) => e.componentDefId === "signal.driving_task");
        if (!cycle || (!caseId && tasks.length > 1)) return;
        for (const c of draft.cases) {
          const own = c.parameterOverrides?.[elementId] ?? {};
          const drivesIt = caseId ? c.id === caseId : !("cycle" in own) && !("profile" in own);
          // a performance case runs to its target, not to a cycle's end
          if (drivesIt && (c.kind ?? "cycle") === "cycle" && c.duration !== cycle.duration_s) {
            c.duration = cycle.duration_s;
            resized.push(`'${c.name}'`);
          }
        }
      });
      if (cycle && resized.length)
        get().log(
          "info",
          `${resized.join(", ")} now run${resized.length > 1 ? "" : "s"} ${cycle.duration_s.toLocaleString("en")} s, the length of ${cycle.name}.`,
        );
    },

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
        // a summary, not a problem: the problems are in the Problems list
        log("info", `Data checks: ${countOf(errors, "error")}, ${countOf(warnings, "warning")}.`);
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
      // The runs the user overlaid stay overlaid (RES-19).
      set({ running: true });
      if (!(await get().passesRunGate(activeCaseId))) {
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

    runAccelerationTest: async () => {
      const { project, running } = get();
      if (!project || running) return;
      const found = project.cases.find((c) => c.kind === "acceleration");
      const id = found?.id ?? uid("case");
      if (!found) {
        // FS Rules 2026 v1.1 (FSG): 75 m from the start line (D 5.1.1), staged
        // 0.30 m behind it (D 5.2.3); runs over 25 s are disqualified in
        // driverless runs only (D 9.2.1)
        updateProject((draft) => {
          draft.cases.push({
            id,
            name: "Acceleration 75 m",
            duration: 25,
            timeStep: 0.01,
            kind: "acceleration",
            endDistance: 75,
            startLine: 0.3,
          });
        });
      }
      set({ activeCaseId: id });
      await get().run();
    },

    runFsEvents: async () => {
      const { project, running, log } = get();
      if (!project || running) return;
      const track = project.systems.flatMap((sys) => sys.elements).find((e) => e.componentDefId === "track.lap");
      if (!track) {
        log("error", "Formula Student events need a Race Track: add one from Driver & Signals.");
        return;
      }
      const layoutOf = (c: SimCase) =>
        String(c.parameterOverrides?.[track.id]?.layout ?? track.parameterOverrides.layout ?? "Autocross");
      const lapsOf = (c: SimCase) => Number(c.parameterOverrides?.[track.id]?.laps ?? track.parameterOverrides.laps ?? 1);
      // an event's case: the one marked for it, else a case of its shape (the
      // FS example's own cases), else a new one
      const shapes: Record<FsEvent, (c: SimCase) => boolean> = {
        acceleration: (c) => c.kind === "acceleration",
        skidpad: (c) => c.kind === "lap" && layoutOf(c) === "Skidpad",
        autocross: (c) => c.kind === "lap" && layoutOf(c) === "Autocross" && lapsOf(c) <= 2,
        endurance: (c) => c.kind === "lap" && lapsOf(c) >= 15,
      };
      const ids: Record<FsEvent, string> = { acceleration: "", skidpad: "", autocross: "", endurance: "" };
      const taken = new Set<string>();
      for (const ev of FS_EVENTS) {
        const c =
          project.cases.find((x) => x.fsEvent === ev && (x.kind ?? "cycle") !== "cycle") ??
          project.cases.find((x) => !x.fsEvent && !taken.has(x.id) && shapes[ev](x));
        if (c) {
          ids[ev] = c.id;
          taken.add(c.id);
        }
      }
      updateProject((draft) => {
        for (const ev of FS_EVENTS) {
          const found = draft.cases.find((c) => c.id === ids[ev]);
          if (found) {
            found.fsEvent = ev;
            continue;
          }
          const id = uid("case");
          ids[ev] = id;
          // FS Rules 2026 v1.1 (FSG): acceleration 75 m from 0.30 m behind the
          // line (D 5.1.1, D 5.2.3); skidpad a right and a left circle after an
          // establishing lap (D 4.2.1); autocross one lap (D 6.2.1); endurance
          // about 22 km (D 7.1.3) on the Autocross layout (979 m: 22 laps)
          const lapCase = (name: string, layout: string, laps: number, every = 1): SimCase => ({
            id, name, duration: 600, timeStep: 1, kind: "lap", outputEvery: every, fsEvent: ev,
            parameterOverrides: { [track.id]: { layout, laps } },
          });
          draft.cases.push(
            ev === "acceleration"
              ? { id, name: "Acceleration 75 m", duration: 25, timeStep: 0.01, kind: "acceleration",
                  endDistance: 75, startLine: 0.3, fsEvent: ev }
              : ev === "skidpad"
                ? lapCase("Skidpad", "Skidpad", 2)
                : ev === "autocross"
                  ? lapCase("Autocross", "Autocross", 1)
                  : lapCase("Endurance", "Autocross", 22, 5),
          );
        }
      });
      for (const ev of FS_EVENTS) {
        set({ activeCaseId: ids[ev] });
        await get().run();
        if (get().running) return;
        const last = get().runs.find((r) => r.caseId === ids[ev]);
        if (!last || last.status === "failed") break; // (its messages say why)
      }
      const ui = useUIStore.getState();
      ui.setRibbonTab("home");
      ui.focusPanel("cases");
      log("info", "Formula Student events run: their points are in Cases & Parameters → Formula Student points.");
    },

    addImportedLap: (lap, name, sha256) => {
      const { project, log } = get();
      if (!project) return;
      const all = project.systems.flatMap((sys) => sys.elements.map((e) => ({ e, sys })));
      const driver = all.find(({ e }) => e.componentDefId === "driver.driver");
      const task = all.find(({ e }) => e.componentDefId === "signal.driving_task")?.e;
      if (!task && !driver) {
        log("error", "An imported lap needs a Driver to follow it: add one from Driver & Signals.");
        return;
      }
      const taskId = task?.id ?? uid("el");
      const caseId = uid("case");
      updateProject((draft) => {
        if (!task && driver) {
          const sys = draft.systems.find((x) => x.id === driver.sys.id);
          sys?.elements.push({
            id: taskId,
            componentDefId: "signal.driving_task",
            label: "Imported lap",
            position: { x: driver.e.position.x - 200, y: driver.e.position.y },
            parameterOverrides: {},
          });
          // its target into the Driver, unless something already feeds it
          const fed = draft.dataBusConnections.some(
            (d) =>
              (d.element2Id === driver.e.id && d.port2Id === "sig_target_in") ||
              (d.element1Id === driver.e.id && d.port1Id === "sig_target_in"),
          );
          if (!fed)
            draft.dataBusConnections.push({
              id: uid("dbc"),
              element1Id: taskId,
              port1Id: "sig_demand",
              element2Id: driver.e.id,
              port2Id: "sig_target_in",
            });
        }
        draft.cases.push({
          id: caseId,
          name,
          duration: Math.ceil(lap.duration_s * 10) / 10,
          timeStep: 0.1,
          kind: "cycle",
          outputEvery: lap.duration_s > 600 ? 10 : 1,
          ...(lap.repeated > 1 ? { fsEvent: "endurance" as const } : {}),
          parameterOverrides: { [taskId]: { profile: lap.profile, cycle: "" } },
        });
      });
      set({ activeCaseId: caseId });
      log(
        "info",
        `Imported lap added as case '${name}': ${lap.duration_s.toLocaleString("en", { maximumFractionDigits: 1 })} s, ` +
          `${(lap.distance_m / 1000).toLocaleString("en", { maximumFractionDigits: 3 })} km` +
          `${lap.repeated > 1 ? ` (${lap.repeated} laps)` : ""}; source file SHA-256 ${sha256}. ` +
          "Its speed comes from the file: LightSim gives the energy and the loads for that speed, not the cornering.",
      );
    },

    runEnduranceStudy: async ({ caseId, batteryId, packs, caps }) => {
      const { project, log, libraryById, running } = get();
      if (!project || running) return;
      const simCase = project.cases.find((c) => c.id === caseId);
      const battery = project.systems.flatMap((sy) => sy.elements).find((e) => e.id === batteryId);
      if (!simCase || !battery || !packs.length || !caps.length) return;
      const pdefs = libraryById[battery.componentDefId]?.parameters ?? [];
      const factor = (key: string, values: number[]) => {
        const d = pdefs.find((p) => p.key === key);
        return {
          elementId: batteryId,
          paramKey: key,
          elementLabel: battery.label,
          paramLabel: d?.label ?? key,
          unit: d && d.unit !== "-" ? d.unit : "",
          values,
        };
      };
      const sweepId = uid("sweep");
      const startedAt = Date.now();
      const points: StudyPoint[] = [];
      const kpiUnits = new Map<string, string>();
      set({ running: true });
      if (!(await get().passesRunGate(caseId))) {
        set({ running: false });
        return;
      }
      log("info", `Endurance energy study: ${packs.length} × ${caps.length} = ${packs.length * caps.length} runs of '${simCase.name}' …`);
      // every pack × cap point, run side by side in the engine's worker
      // processes as a sweep's are (ENG-05)
      const grid = packs.flatMap((pack) => caps.map((cap) => [pack, cap]));
      const byIndex = new Map<number, StudyPoint>();
      let totals: api.StudyTotals | undefined;
      set({ livePct: 0 });
      try {
        const handle = api.runStudyLive(
          {
            project: structuredClone(project),
            caseId,
            points: grid.map(([pack, cap]) => ({
              overrides: { [batteryId]: { capacity_kWh: pack, output_power_limit_kW: cap } },
              values: [pack, cap],
              label: `${simCase.name} · ${pack} kWh, ${cap} kW`,
            })),
            sweepId,
            sweepParam: "Pack × power cap",
            sweepUnit: "kWh",
          },
          {
            onPoint: (e) => {
              const notValid = (e.summary ?? []).filter((v) => v.notValid);
              byIndex.set(e.index, {
                values: e.values,
                ...(e.runId ? { runId: e.runId } : {}),
                status: e.status,
                ...(e.incomplete ? { incomplete: e.incomplete } : {}),
                ...(e.wallS != null ? { wallS: e.wallS } : {}),
                kpis: Object.fromEntries(
                  (e.summary ?? []).filter((v) => Number.isFinite(v.value)).map((v) => [v.label, v.value]),
                ),
                ...(notValid.length ? { notValid: Object.fromEntries(notValid.map((v) => [v.label, v.notValid!])) } : {}),
              });
              for (const v of e.summary ?? []) if (!kpiUnits.has(v.label)) kpiUnits.set(v.label, v.unit);
              if (e.status === "failed") log("error", `Study point ${e.values[0]} kWh, ${e.values[1]} kW failed.`);
              set({ livePct: (100 * byIndex.size) / grid.length });
            },
          },
        );
        activeStudy = handle;
        totals = await handle.done;
      } catch (e) {
        log("error", `Endurance energy study failed: ${(e as Error).message}`);
      } finally {
        activeStudy = null;
        set({ running: false });
      }
      if (get().project?.id === project.id) await loadRunHistory(project.id);
      points.push(...[...byIndex.entries()].sort((a, b) => a[0] - b[0]).map(([, pt]) => pt));
      if (get().project?.id !== project.id || !points.length) return;
      const ran = new Set(points.map((p) => p.values.join(",")));
      const notRun = packs.flatMap((pack) =>
        caps.filter((cap) => !ran.has(`${pack},${cap}`)).map((cap): StudyPoint => ({ values: [pack, cap], status: "not run", kpis: {} })),
      );
      const study: Study = {
        id: sweepId,
        startedAt,
        caseId,
        caseName: simCase.name,
        factors: [factor("capacity_kWh", packs), factor("output_power_limit_kW", caps)],
        kpis: [...kpiUnits].map(([label, unit]) => ({ label, unit })),
        points: [...points, ...notRun],
        ...(totals ? { workers: totals.workers, wallS: totals.wallS } : {}),
      };
      // kept with the project's runs, not in the model file (PLT-34)
      set((s) => ({ studies: [...s.studies, study] }));
      try {
        await api.storeStudy(project.id, study);
        log("info", `Endurance energy study saved with the runs (Cases & Parameters → Endurance energy study and Saved studies).`);
      } catch (e) {
        log("warning", `The study could not be stored with the runs (${(e as Error).message}); it is kept for this session only.`);
      }
    },

    applyLapCalibration: (muScale, cza) => {
      const { project, libraryById, log } = get();
      if (!project) return;
      const def = (id: string, key: string) =>
        Number(libraryById[id]?.parameters.find((p) => p.key === key)?.default ?? 0);
      updateProject((draft) => {
        for (const sys of draft.systems)
          for (const e of sys.elements) {
            if (e.componentDefId === "propulsion.wheel") {
              for (const key of ["mu", "mu_lateral", "mu_load_sensitivity_per_kN"]) {
                const v = Number(e.parameterOverrides[key] ?? def(e.componentDefId, key));
                if (v) e.parameterOverrides[key] = Math.round(v * muScale * 1e4) / 1e4;
              }
            } else if (e.componentDefId === "vehicle.body") {
              e.parameterOverrides.downforce_cza_m2 = cza;
            }
          }
      });
      log("info", `Lap mode calibration applied: the wheels' grip × ${muScale}, the Vehicle's CzA ${cza} m².`);
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
      if (!(await get().passesRunGate(caseId))) {
        set({ running: false });
        return;
      }

      // the points run side by side in the engine's worker processes
      // (ENG-05); each is stored as a run of the project as it ends, and
      // only its summary comes back here
      log(
        "info",
        `Sweep: ${el?.label ?? elementId} · ${paramLabel} over ${values.length} value(s) …`,
      );
      const byIndex = new Map<number, StudyPoint>();
      let totals: api.StudyTotals | undefined;
      set({ livePct: 0 });
      try {
        const handle = api.runStudyLive(
          {
            project: structuredClone(project),
            caseId,
            points: values.map((value) => ({
              overrides: { [elementId]: { [paramKey]: value } },
              values: [value],
              label: `${simCase.name} · ${paramLabel}=${value}${unit}`,
            })),
            sweepId,
            sweepParam: paramLabel,
            sweepUnit: paramUnit,
          },
          {
            onStarted: (workers) =>
              log("info", `Sweep running ${countOf(workers, "point")} at a time, one per processor core.`),
            onPoint: (e) => {
              const notValid = (e.summary ?? []).filter((v) => v.notValid);
              byIndex.set(e.index, {
                values: e.values,
                ...(e.runId ? { runId: e.runId } : {}),
                status: e.status,
                ...(e.incomplete ? { incomplete: e.incomplete } : {}),
                ...(e.wallS != null ? { wallS: e.wallS } : {}),
                // (a value JSON cannot carry would make the project unsavable)
                // and each part's duty (RES-39)
                kpis: Object.fromEntries(
                  [...(e.summary ?? []), ...dutyKpis({ duty: e.duty } as SimResult)]
                    .filter((v) => Number.isFinite(v.value))
                    .map((v) => [v.label, v.value]),
                ),
                ...(notValid.length ? { notValid: Object.fromEntries(notValid.map((v) => [v.label, v.notValid!])) } : {}),
              });
              for (const v of [...(e.summary ?? []), ...dutyKpis({ duty: e.duty } as SimResult)])
                if (!kpiUnits.has(v.label)) kpiUnits.set(v.label, v.unit);
              if (e.status === "failed") log("error", `Sweep point ${paramLabel}=${e.values[0]}${unit} failed.`);
              if (e.pruned?.length) {
                log("warning", `Stored runs reached the disk budget: deleted the ${e.pruned.length} oldest run(s).`);
              }
              set({ livePct: (100 * byIndex.size) / values.length });
            },
          },
        );
        activeStudy = handle;
        totals = await handle.done;
      } catch (e) {
        log("error", `Sweep failed: ${(e as Error).message}`);
      } finally {
        activeStudy = null;
        set({ running: false });
      }
      // the points' runs, from the run store
      if (get().project?.id === project.id) await loadRunHistory(project.id);
      points.push(
        ...values.map((v, i): StudyPoint => byIndex.get(i) ?? { values: [v], status: "not run", kpis: {} }),
      );
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
          const study: Study = {
            id: sweepId,
            startedAt,
            caseId,
            caseName: simCase.name,
            factors: [
              { elementId, paramKey, elementLabel: el?.label ?? elementId, paramLabel, unit: paramUnit, values },
            ],
            kpis: [...kpiUnits].map(([label, kpiUnit]) => ({ label, unit: kpiUnit })),
            points,
            ...(totals ? { workers: totals.workers, wallS: totals.wallS } : {}),
          };
          set((s) => ({ studies: [...s.studies, study] }));
          try {
            await api.storeStudy(project.id, study);
            log(
              "info",
              `Sweep saved as a study of '${project.name}' (Cases & Parameters → Saved studies), ` +
                "with its runs; the model file is unchanged.",
            );
          } catch (e) {
            log("warning", `The study could not be stored with the runs (${(e as Error).message}); it is kept for this session only.`);
          }
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

    removeStudy: (studyId) => {
      const { project, log } = get();
      set((s) => ({ studies: s.studies.filter((st) => st.id !== studyId) }));
      if (!project) return;
      api.deleteStudy(project.id, studyId)?.catch?.((e: Error) => {
        if (!e.message.startsWith("404")) log("error", `Could not delete the study: ${e.message}`);
      });
    },

    /** Error-level data-check gate shared by run + runSweep. */
    passesRunGate: async (caseId) => {
      const { project, log } = get();
      if (!project) return false;
      if (!(await trustsCode(project))) {
        log("warning", "Run cancelled: the project's code was not trusted. Run again to be asked again.");
        return false;
      }
      try {
        const checks = await api.validateProject(project);
        set({ dataChecks: checks });
        if (get().project !== project) scheduleRecheck(); // edited while it checked
        const errors = runBlockers(checks, caseId);
        if (errors.length > 0) {
          log("error", `Run blocked — fix ${countOf(errors.length, "data-check error")} first.`);
          const ui = useUIStore.getState();
          if (ui.ribbonTab === "results") ui.setRibbonTab("home");
          ui.focusPanel("data-checks");
          return false;
        }
      } catch (e) {
        // backend unreachable — the run's own connection will surface the failure
        log("warning", `Pre-flight data checks unavailable (${(e as Error).message}); running anyway.`);
      }
      return get().reviewScripts("run");
    },

    reviewScripts: async (when) => {
      const { project, log } = get();
      if (!project) return true;
      let report: Awaited<ReturnType<typeof api.checkScripts>>;
      try {
        report = await api.checkScripts(project);
      } catch {
        return true; // engine unreachable: it refuses unapproved code itself
      }
      let pending = report.scripts.filter((s) => !s.approved);
      // code typed in this window is the user's own: no need to ask
      const typed = pending.filter((s) => typedScripts.has(s.code));
      pending = pending.filter((s) => !typedScripts.has(s.code));
      try {
        const codes = typed.map((s) => s.code);
        if (codes.length) await api.approveScripts(codes);
      } catch (e) {
        log("warning", `Could not record the scripts you typed as approved: ${(e as Error).message}`);
      }
      if (pending.length === 0) return true;
      const n = pending.length;
      const scripts = countOf(n, "script");
      const ok = await scriptTrustDialog({
        title: when === "open" ? `This project contains ${scripts} you have not approved` : `Run ${scripts} you have not approved?`,
        message:
          "Scripts are Python code that runs while the model simulates. " +
          (n === 1 ? "This one came" : "These came") +
          " with the project, from another computer or changed since you last approved them. " +
          "Read the code and run it only if you trust where the project came from.",
        note:
          report.mode === "always-prompt"
            ? "Managed by your organisation: LightSim asks every time it opens a project."
            : undefined,
        scripts: pending.map((s) => ({ label: s.label, code: s.code })),
        confirmLabel: "Run scripts",
        cancelLabel: when === "open" ? "Open without running scripts" : "Don't run",
      });
      if (!ok) {
        log(
          "warning",
          when === "open"
            ? `Opened without running its scripts: ${pending.map((s) => `'${s.label}'`).join(", ")}. Run asks again.`
            : "Run cancelled: its scripts are not approved.",
        );
        return false;
      }
      try {
        await api.approveScripts(pending.map((s) => s.code));
      } catch (e) {
        log("error", `Could not approve the scripts: ${(e as Error).message}`);
        return false;
      }
      log("info", `Approved ${scripts} to run.`);
      return true;
    },

    stopRun: () => {
      activeStudy?.cancel();
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
    editRun: (runId, edit) => {
      const { runs, running, project } = get();
      const run = runs.find((r) => r.id === runId);
      // (while a run is going its first store may still be on its way)
      if (!run || running || !project) return;
      const next = { ...run };
      for (const key of ["name", "note"] as const) {
        const v = edit[key]?.trim();
        if (v) next[key] = v;
        else if (v === "") delete next[key];
      }
      if (next.name === run.name && next.note === run.note) return;
      set((s) => ({ runs: s.runs.map((r) => (r.id === runId ? next : r)) }));
      // ponytail: stores the whole run again (about 1.5 s for 18,001 points ×
      // 45 channels); a PATCH of the name and note if renames get frequent
      void storeRun(project.id, next);
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

// The checks follow the model: a quiet re-check runs RECHECK_MS after a
// project is opened, made or imported and after every edit, so part badges,
// the Problems list and the status-bar count show a problem, and clear it, as
// soon as it is made or fixed. A re-check due while a run is in progress
// waits for the run to end.
const RECHECK_MS = 600;
let recheckTimer: ReturnType<typeof setTimeout> | undefined;
let rechecksStopped = false;
const unsubscribeRechecks = useProjectStore.subscribe((s, prev) => {
  if (s.project !== prev.project && s.project) scheduleRecheck();
});

function scheduleRecheck(): void {
  clearTimeout(recheckTimer);
  if (!rechecksStopped) recheckTimer = setTimeout(recheck, RECHECK_MS);
}

/** Stop this store's quiet re-checks for good: for tests, so that a store
 *  instance left behind by `vi.resetModules()` cannot check its model (and
 *  call the shared api mock) during a later test. The app never calls it. */
export function stopRechecks(): void {
  rechecksStopped = true;
  clearTimeout(recheckTimer);
  recheckTimer = undefined;
  unsubscribeRechecks();
}

async function recheck(): Promise<void> {
  const { project, running } = useProjectStore.getState();
  if (!project || rechecksStopped) return;
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

/** "1 error", "2 errors". */
export function countOf(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? "" : "s"}`;
}

/** The Data Checks' errors that stop a run of `caseId`: those about the
 *  model, and those about that case (an error about another case's own
 *  values or kind does not stop it). */
export function runBlockers(checks: DataCheck[], caseId: string): DataCheck[] {
  return checks.filter((c) => c.level === "error" && (c.caseId == null || c.caseId === caseId));
}

/** A row of the Problems list. */
export interface Problem {
  level: "info" | "warning" | "error";
  text: string;
  fix?: string | null;
  /** the parts it is about (in the open project) */
  elementIds: string[];
  /** Data Checks, or the run it came from */
  source: "check" | { caseName: string; startedAt: number };
}

/** The latest run that has finished (runs are newest first). */
function lastRunOf(runs: SimRun[]): SimRun | undefined {
  return runs.find((r) => r.status !== "running");
}

/** The parts a run message names: run messages quote part labels
 *  ('HV Battery Pack', 'E-Motor.torque'). */
// ponytail: label match; a renamed or repeated label misses or doubles, exact
// targets come with VAL-10's message format
function partsNamed(text: string, elements: ElementInstance[]): string[] {
  return elements.filter((e) => text.includes(`'${e.label}'`) || text.includes(`'${e.label}.`)).map((e) => e.id);
}

/** The warnings and errors of a finished run that the Data Checks do not
 *  already list: the engine repeats the model's own warnings in every run,
 *  and a lap case's without the "Case '…': " the checks put in front. */
function runProblemsOf(dataChecks: DataCheck[] | null, run: SimRun | undefined) {
  if (!run || run.status === "running") return [];
  const checked = new Set((dataChecks ?? []).map((c) => c.text));
  return run.result.messages.filter(
    (m) => m.level !== "info" && !checked.has(m.text) && !checked.has(`Case '${run.caseName}': ${m.text}`),
  );
}

/** Every current problem: the latest Data Checks, then the warnings and
 *  errors of the latest finished run. Errors first. */
export function problemsOf(dataChecks: DataCheck[] | null, run: SimRun | undefined, project: Project | null): Problem[] {
  const elements = project?.systems.flatMap((s) => s.elements) ?? [];
  const ids = new Set(elements.map((e) => e.id));
  const out: Problem[] = (dataChecks ?? []).map((c) => ({
    level: c.level,
    text: c.text,
    fix: c.fix,
    // engines before 0.3 name one part at most
    elementIds: (c.elementIds?.length ? c.elementIds : c.elementId ? [c.elementId] : []).filter((id) => ids.has(id)),
    source: "check" as const,
  }));
  if (run) {
    const source = { caseName: run.caseName, startedAt: run.startedAt };
    for (const m of runProblemsOf(dataChecks, run)) {
      out.push({ level: m.level, text: m.text, elementIds: partsNamed(m.text, elements), source });
    }
  }
  const rank = { error: 0, warning: 1, info: 2 };
  return out.sort((a, b) => rank[a.level] - rank[b.level]); // stable: checks before run messages
}

export function useProblems(): Problem[] {
  const dataChecks = useProjectStore((s) => s.dataChecks);
  const run = useProjectStore((s) => lastRunOf(s.runs));
  const project = useProjectStore((s) => s.project);
  return useMemo(() => problemsOf(dataChecks, run, project), [dataChecks, run, project]);
}

/** Errors and warnings in the Problems list, for the status bar and the tab
 *  badge. */
export function problemCounts(s: Pick<ProjectState, "dataChecks" | "runs">): { errors: number; warnings: number } {
  let errors = 0;
  let warnings = 0;
  for (const m of [...(s.dataChecks ?? []), ...runProblemsOf(s.dataChecks, lastRunOf(s.runs))]) {
    if (m.level === "error") errors++;
    else if (m.level === "warning") warnings++;
  }
  return { errors, warnings };
}

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
