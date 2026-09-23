// Shared data model — mirrors the backend pydantic schemas (spec §4).

export type ScalarValue = number | string | boolean;
// Tabular parameter data keyed by the independent variable (numeric text keys).
export type Table1D = Record<string, number>;
export type Table2D = Record<string, Record<string, number>>;
export type ParamValue = ScalarValue | Table1D | Table2D;

export type PortKind =
  | "power"
  | "signal"
  | "mechanical"
  | "electrical"
  | "thermal"
  | "fluid";

export type Domain = "mechanical" | "electrical" | "thermal" | "signal" | "fluid";

export interface PortDef {
  id: string;
  name: string;
  direction: "input" | "output" | "bidirectional";
  kind: PortKind;
  unitGroup?: string;
  side?: "left" | "right"; // canvas layout hint
  /** Electrical polarity: positive (supply/+) or negative (return/−). */
  polarity?: "positive" | "negative";
}

export interface AxisDef {
  name: string;
  unit: string;
}

export interface ParameterDef {
  key: string;
  label: string;
  /** Required; "-" marks dimensionless. For tables: unit of the dependent value. */
  unit: string;
  default: ParamValue;
  type: "number" | "enum" | "boolean" | "string" | "code" | "table1d" | "table2d";
  options?: string[];
  /** table1d: one axis; table2d: [outer, inner]. */
  axes?: AxisDef[];
  /** FMI-style variability: "fixed" params are baked in at model build (live
   *  edits take effect on the next run); "tunable" (default) apply live. */
  variability?: "fixed" | "tunable";
}

export interface ComponentDef {
  id: string;
  category: string;
  name: string;
  icon: string;
  domain: Domain;
  description?: string;
  ports: PortDef[];
  parameters: ParameterDef[];
  /** Elements of this type may carry per-instance signal ports (Script, Monitor). */
  allowDynamicPorts?: boolean;
}

export type PortSide = "left" | "right" | "top" | "bottom";

export interface ElementInstance {
  id: string;
  componentDefId: string;
  label: string;
  position: { x: number; y: number };
  parameterOverrides: Record<string, ParamValue>;
  /** Per-instance signal ports; only honored when the def allows them. */
  dynamicPorts?: PortDef[];
  /** Per-instance canvas pin placement (Shift+drag a pin to move it). */
  portSides?: Record<string, PortSide>;
  /** Per-instance pin offset along its side, 0..1 (set by Shift+drag). */
  portOffsets?: Record<string, number>;
  /** Per-instance canvas node size in flow units (drag a node's edges to resize);
   *  the engine sends null for elements left at the default size. */
  size?: { width: number; height: number } | null;
  isSubSystem?: boolean;
  subSystemId?: string | null;
}

export interface Connection {
  id: string;
  sourceElementId: string;
  sourcePortId: string;
  targetElementId: string;
  targetPortId: string;
}

export interface DataBusConnection {
  id: string;
  element1Id: string;
  port1Id: string;
  element2Id: string;
  port2Id: string;
}

export interface SystemNode {
  id: string;
  name: string;
  parentId: string | null;
  elements: ElementInstance[];
  connections: Connection[];
}

export interface SimCase {
  id: string;
  name: string;
  duration: number;
  /** Output step in seconds (results stored, live edits applied); the solver runs at ≤10 ms. */
  timeStep: number;
  /** Record a data point every N output steps (further decimation); 1 = every step. */
  outputEvery?: number;
  /** 0 = as fast as possible; N > 0 = pace at N× real time (live tuning). */
  realtimeFactor?: number;
  /**
   * Per-case parameter overrides: { elementId: { paramKey: value } }. Layered
   * on top of each element's own parameterOverrides at solve time, so a case
   * can tweak values — and a parameter sweep can vary one — without editing the
   * shared topology.
   */
  parameterOverrides?: Record<string, Record<string, ParamValue>>;
}

/** One swept parameter of a study, with its labels as they were when it ran
 *  (the element may be renamed or removed since). */
export interface StudyFactor {
  elementId: string;
  paramKey: string;
  elementLabel: string;
  paramLabel: string;
  unit: string;
  values: number[];
}

/** A row of a study's results table: one point and its answers. */
export interface StudyPoint {
  /** the factor values of this point, in factor order */
  values: number[];
  /** the run that produced it (it may since have left the run history) */
  runId?: string | null;
  status: SimResult["status"] | "not run";
  /** why the point's run did not finish normally */
  incomplete?: string | null;
  /** summary value label → value */
  kpis: Record<string, number>;
  /** summary value label → why the run's checks rule that value out */
  notValid?: Record<string, string>;
}

/** A parameter study saved with its project (STU-03): what was swept on which
 *  case, and its compact results table (it outlives the runs it came from). */
export interface Study {
  id: string;
  /** epoch ms */
  startedAt: number;
  caseId: string;
  caseName: string;
  factors: StudyFactor[];
  /** the table's KPI columns: run summary values */
  kpis: { label: string; unit: string }[];
  points: StudyPoint[];
}

export interface Project {
  id: string;
  name: string;
  /** Project-file format version (files from before versioning are v1). */
  schemaVersion?: number;
  /** Short human-readable summary (example projects set this; usually omitted). */
  description?: string | null;
  systems: SystemNode[];
  dataBusConnections: DataBusConnection[];
  cases: SimCase[];
  /** Parameter studies run on this project, oldest first. */
  studies?: Study[];
}

export interface SimMessage {
  level: "info" | "warning" | "error";
  text: string;
}

export interface Channel {
  elementId: string;
  portId: string;
  label: string;
  unit: string;
  /** value is null where the channel has no data yet (a gap, not a zero). */
  timeSeries: { t: number; value: number | null }[];
}

export interface SummaryValue {
  label: string;
  value: number;
  unit: string;
  /** Why the run verdict says this number is not valid, e.g. "cycle not followed". */
  notValid?: string | null;
}

export interface SimResult {
  caseId: string;
  status: "success" | "failed" | "warning";
  messages: SimMessage[];
  channels: Channel[];
  summary: SummaryValue[];
}

/** A scalar parameter change sent to the engine while a run was going. */
export interface LiveEdit {
  /** simulated time the run had reached when the edit was sent, s */
  t: number;
  elementId: string;
  key: string;
  value: ScalarValue;
}

/** What made a run (RES-09): enough to tell which model and settings
 *  produced a result, and to open that model again. */
export interface RunSnapshot {
  /** the project exactly as it was run (a sweep's value included) */
  project: Project;
  /** the case settings it ran with */
  case: SimCase;
  /** the SimStudio version that ran it (null: the engine did not say) */
  appVersion: string | null;
  /** SHA-256 of the project as canonical JSON, hex (absent where the
   *  browser offers no Web Crypto) */
  modelHash?: string;
  /** scalar parameter edits made while it ran, in order */
  liveEdits: LiveEdit[];
}

/** One recorded simulation run. Finished runs are stored on disk with their
 *  project by the engine and listed again when the project is opened. */
export interface SimRun {
  id: string;
  caseId: string;
  caseName: string;
  /** epoch ms when the run started (used for the run label / ordering). */
  startedAt: number;
  status: SimResult["status"] | "running";
  result: SimResult;
  /** Set on runs produced by a parameter sweep — groups the family and carries
   *  the swept value so Results can overlay them and plot metric-vs-value. */
  sweepId?: string;
  sweepParam?: string; // display label of the swept parameter
  sweepValue?: number; // the value used for this run
  sweepUnit?: string;
  /** Why the run did not finish normally (stopped, failed, connection lost).
   *  Its numbers are partial, so sweeps leave it out of their curve and
   *  tables unless the user asks to see it. */
  incomplete?: string;
  /** The model, settings and version that made it; absent on runs stored
   *  before runs kept one. */
  snapshot?: RunSnapshot;
}

/** A project's stored run as the engine lists it: the run without its
 *  channel data or snapshot, plus its summary values and its compressed size
 *  on disk. */
export interface StoredRunInfo extends Omit<SimRun, "status" | "result" | "snapshot"> {
  status: SimResult["status"];
  summary: SummaryValue[];
  bytes: number;
}

export interface DataCheck {
  level: "info" | "warning" | "error";
  elementId?: string | null;
  elementLabel?: string | null;
  text: string;
}

export interface LogMessage {
  level: "info" | "warning" | "error";
  text: string;
  time: string; // HH:MM:SS
}
