import { useRef, useState } from "react";
import {
  AArrowDown,
  AArrowUp,
  Bot,
  CheckCircle2,
  ClipboardCopy,
  Copy,
  Download,
  EyeOff,
  FileDown,
  FileInput,
  FilePlus2,
  FileSpreadsheet,
  FileUp,
  FolderOpen,
  Gauge,
  History,
  Fuel,
  IdCard,
  LayoutGrid,
  ListChecks,
  Moon,
  Paperclip,
  Play,
  Plus,
  Redo2,
  RotateCcw,
  Save,
  SaveAll,
  Settings2,
  Sliders,
  Square,
  Sun,
  Timer,
  Trophy,
  Crosshair,
  Trash2,
  Undo2,
  Upload,
  X,
} from "lucide-react";
import * as api from "../api";
import type { Project } from "../types";
import { resetDockLayout } from "./DockLayout";
import { ExampleCardDialog } from "./ExampleCardEditor";
import { useDismiss } from "./useDismiss";
import { LapImportDialog } from "./LapImportDialog";
import { CalibrateDialog } from "./CalibrateDialog";
import { LabelEstimateDialog } from "./LabelEstimate";
import { VehicleTestsDialog } from "./VehicleTests";
import { TemplatesDialog } from "./TemplatesDialog";
import { downloadParameterTemplate, useParameterSheet } from "./ParameterSheet";
import { confirmDialog } from "../dialog";
import { desktop } from "../desktop";
import { HelpMenu } from "./HelpMenu";
import { confirmReplaceProject, useProjectStore } from "../store/projectStore";
import {
  FONT_SCALE_MAX,
  FONT_SCALE_MIN,
  FONT_SCALE_STEP,
  useUIStore,
  type RibbonTab,
} from "../store/uiStore";

// Optimization is not implemented yet — kept out of the ribbon (its RibbonTab
// id/stub render remain) until it ships as a real feature. Parameters is real
// (per-case overrides + sweeps, driven from the Cases & Parameters panel).
const TABS: { id: RibbonTab; label: string }[] = [
  { id: "start", label: "Start" },
  { id: "project", label: "Project" },
  { id: "home", label: "Home" },
  { id: "simulations", label: "Simulations" },
  { id: "parameters", label: "Parameters" },
  { id: "results", label: "Results" },
];

function RibbonGroup({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col border-r border-[color:var(--ss-border)] px-2 last:border-r-0">
      <div className="flex flex-1 items-center gap-1">{children}</div>
      <div className="pb-0.5 text-center text-[10px] leading-3 text-[color:var(--ss-text-dim)]">
        {label}
      </div>
    </div>
  );
}

function BigButton({
  icon: Icon,
  label,
  onClick,
  disabled,
  accent,
  title,
  ref,
}: {
  icon: React.ComponentType<{ size?: number; className?: string }>;
  label: string;
  onClick?: () => void;
  disabled?: boolean;
  accent?: boolean;
  title?: string;
  ref?: React.Ref<HTMLButtonElement>;
}) {
  return (
    <button
      ref={ref}
      className={`flex h-[52px] w-[58px] flex-col items-center justify-center gap-0.5 rounded text-[11px] leading-tight
        ${accent ? "text-[color:var(--ss-accent)]" : ""}
        hover:bg-[color:var(--ss-hover)] active:bg-[color:var(--ss-active)] disabled:opacity-40 disabled:hover:bg-transparent`}
      onClick={onClick}
      disabled={disabled}
      title={title ?? label}
    >
      <Icon size={20} />
      <span>{label}</span>
    </button>
  );
}

/** Open a project file the user picked or dropped as a new project: a zip
 *  bundle (with its attached files) or a project's JSON. */
export async function importFile(f: File): Promise<void> {
  if (!(await confirmReplaceProject(`Importing '${f.name}'`))) return;
  const store = useProjectStore.getState();
  if (/\.zip$/i.test(f.name)) await store.importBundle(f);
  else await store.importProject(await f.text());
}

/** A menu heading: the Open menu lists the user's projects and the examples
 *  apart. */
function MenuHeading({ children }: { children: React.ReactNode }) {
  return (
    <div className="px-3 pb-1 pt-1.5 text-[10px] font-semibold uppercase tracking-wide text-[color:var(--ss-text-dim)]">
      {children}
    </div>
  );
}

function MenuNote({ children }: { children: React.ReactNode }) {
  return <div className="px-3 py-1.5 text-[12px] text-[color:var(--ss-text-dim)]">{children}</div>;
}

/** A project or example in the Open menu: its name, id and description. */
function ProjectMenuItem({ entry, onClick }: { entry: api.ProjectEntry; onClick: () => void }) {
  return (
    <button
      role="menuitem"
      className="block w-full min-w-0 flex-1 px-3 py-2 text-left hover:bg-[color:var(--ss-accent-soft)]"
      onClick={onClick}
    >
      <div className="flex items-baseline justify-between gap-2">
        <span className="text-[12px] font-medium text-[color:var(--ss-text)]">{entry.name}</span>
        <span className="shrink-0 text-[10px] text-[color:var(--ss-text-dim)]">{entry.id}</span>
      </div>
      {entry.description && (
        <p className="mt-0.5 whitespace-pre-line text-[11px] leading-snug text-[color:var(--ss-text-dim)]">
          {entry.description}
        </p>
      )}
    </button>
  );
}

/** Open: the user's saved projects, then the examples shipped with the app.
 *  An example opens as an unsaved copy (the example itself is read-only);
 *  one the user does not want listed is hidden, not deleted, and "Restore
 *  hidden examples" lists it again. */
function OpenProjectButton() {
  const [open, setOpen] = useState(false);
  const [projects, setProjects] = useState<api.ProjectEntry[]>([]);
  const [examples, setExamples] = useState<api.ExampleEntry[]>([]);
  const [files, setFiles] = useState<api.RecentFile[]>([]);
  const ref = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const openProject = useProjectStore((s) => s.openProject);
  const openFile = useProjectStore((s) => s.openFile);
  const forgetFile = useProjectStore((s) => s.forgetFile);
  const openExample = useProjectStore((s) => s.openExample);
  const hideExample = useProjectStore((s) => s.hideExample);
  const restoreExamples = useProjectStore((s) => s.restoreExamples);
  const log = useProjectStore((s) => s.log);
  useDismiss(open, () => setOpen(false), ref, button);

  const listExamples = async () => {
    try {
      setExamples(await api.listExamples());
    } catch (e) {
      log("error", `Cannot list the examples: ${(e as Error).message}`);
      setExamples([]);
    }
  };
  const shown = examples.filter((e) => !e.hidden);
  const hidden = examples.length - shown.length;
  const choose = (name: string, go: () => Promise<void>) => {
    setOpen(false);
    void confirmReplaceProject(`Opening '${name}'`).then((ok) => {
      if (ok) void go();
    });
  };

  return (
    <div className="relative" ref={ref}>
      <BigButton
        ref={button}
        icon={FolderOpen}
        label="Open"
        onClick={async () => {
          if (!open) {
            try {
              setProjects(await api.listProjects());
            } catch (e) {
              log("error", `Cannot list projects: ${(e as Error).message}`);
              setProjects([]);
            }
            await listExamples();
            setFiles((await api.listFiles().catch(() => [])) ?? []);
          }
          setOpen(!open);
        }}
      />
      {open && (
        <div
          role="menu"
          aria-label="Open project"
          className="absolute left-0 top-[54px] z-50 max-h-[60vh] w-[400px] overflow-auto rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] py-1 shadow-lg"
        >
          {desktop() && (
            <button
              role="menuitem"
              className="flex w-full items-center gap-2 px-3 py-2 text-left text-[12px] font-medium hover:bg-[color:var(--ss-accent-soft)]"
              onClick={() => choose("a project file", () => openFile())}
            >
              <FileInput size={14} /> Open file… <span className="ml-auto text-[10px] text-[color:var(--ss-text-dim)]">Ctrl+O</span>
            </button>
          )}
          {files.length > 0 && (
            <div role="group" aria-label="Recent files" className="border-b border-[color:var(--ss-border)]">
              <MenuHeading>Recent files</MenuHeading>
              {files.map((f) => (
                <div key={f.id} className="flex items-stretch">
                  <ProjectMenuItem
                    entry={{ ...f, id: f.path, description: f.exists ? f.description : "File not found" }}
                    onClick={() => f.exists && choose(f.name, () => openProject(f.id))}
                  />
                  <button
                    role="menuitem"
                    className="shrink-0 px-2 text-[color:var(--ss-text-dim)] hover:bg-[color:var(--ss-accent-soft)] hover:text-[color:var(--ss-text)]"
                    title="Remove from Recent files (the file stays where it is)"
                    aria-label={`Remove '${f.name}' from Recent files`}
                    onClick={async () => {
                      await forgetFile(f.id);
                      setFiles((await api.listFiles().catch(() => [])) ?? []);
                    }}
                  >
                    <X size={13} />
                  </button>
                </div>
              ))}
            </div>
          )}
          <div role="group" aria-label="Your projects">
            <MenuHeading>Your projects</MenuHeading>
            {projects.length === 0 && <MenuNote>None saved yet</MenuNote>}
            {projects.map((p) => (
              <ProjectMenuItem key={p.id} entry={p} onClick={() => choose(p.name, () => openProject(p.id))} />
            ))}
          </div>
          <div role="group" aria-label="Examples" className="mt-1 border-t border-[color:var(--ss-border)]">
            <MenuHeading>Examples · open as a copy</MenuHeading>
            {shown.length === 0 && <MenuNote>{hidden ? "All examples are hidden" : "No examples"}</MenuNote>}
            {shown.map((e) => (
              <div key={e.id} className="flex items-stretch">
                <ProjectMenuItem entry={e} onClick={() => choose(e.name, () => openExample(e.id))} />
                <button
                  role="menuitem"
                  className="shrink-0 px-2 text-[color:var(--ss-text-dim)] hover:bg-[color:var(--ss-accent-soft)] hover:text-[color:var(--ss-text)]"
                  title="Hide this example (Restore hidden examples lists it again)"
                  aria-label={`Hide example '${e.name}'`}
                  onClick={async () => {
                    if (await hideExample(e.id, e.name)) await listExamples();
                  }}
                >
                  <EyeOff size={13} />
                </button>
              </div>
            ))}
            {hidden > 0 && (
              <button
                role="menuitem"
                className="block w-full px-3 py-1.5 text-left text-[12px] text-[color:var(--ss-accent)] hover:bg-[color:var(--ss-accent-soft)]"
                onClick={async () => {
                  await restoreExamples();
                  await listExamples();
                }}
              >
                Restore hidden examples ({hidden})
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function HomeTab() {
  const store = useProjectStore();
  const fileRef = useRef<HTMLInputElement>(null);
  const [templatesOpen, setTemplatesOpen] = useState(false);
  return (
    <>
      {templatesOpen && <TemplatesDialog onClose={() => setTemplatesOpen(false)} />}
      <RibbonGroup label="Project">
        <BigButton
          icon={FilePlus2}
          label="New"
          title="New project: from an example or blank (the Start page)"
          onClick={() => useUIStore.getState().setRibbonTab("start")}
        />
        <BigButton
          icon={LayoutGrid}
          label="Templates"
          title="Start from a pre-wired vehicle template with a short form, or save this model as a template"
          onClick={() => setTemplatesOpen(true)}
        />
        <OpenProjectButton />
        <BigButton
          icon={Save}
          label="Save"
          title={store.exampleId ? "Save this copy of the example as a new project (the example stays as it is)" : "Save"}
          onClick={() => void store.saveRemote()}
        />
        {desktop() && (
          <BigButton
            icon={SaveAll}
            label="Save As…"
            title="Save the project as a .lightsim file in a folder you choose (Ctrl+Shift+S)"
            onClick={() => void store.saveAs()}
          />
        )}
        <BigButton
          icon={Download}
          label="Export"
          onClick={() => void store.exportProject()}
          title="Download the project as a .lightsim file (a .zip with its attached files, if it has any)"
        />
        <BigButton
          icon={Upload}
          label="Import"
          onClick={() => fileRef.current?.click()}
          title="Open a project file (.lightsim, .json or a .zip bundle) as a new project"
        />
        <input
          ref={fileRef}
          type="file"
          accept=".lightsim,.json,.zip,application/json,application/zip"
          className="hidden"
          onChange={async (e) => {
            const input = e.target;
            const f = input.files?.[0];
            input.value = "";
            if (!f) return;
            await importFile(f);
          }}
        />
      </RibbonGroup>
      <RibbonGroup label="Edit">
        <BigButton icon={Undo2} label="Undo" onClick={store.undo} disabled={store.past.length === 0} />
        <BigButton icon={Redo2} label="Redo" onClick={store.redo} disabled={store.future.length === 0} />
        <BigButton
          icon={Trash2}
          label="Delete"
          onClick={() => store.selectedElementId && store.removeElements([store.selectedElementId])}
          disabled={!store.selectedElementId}
        />
      </RibbonGroup>
      <RibbonGroup label="Workspace">
        <BigButton
          icon={Settings2}
          label="Properties"
          onClick={() => useUIStore.getState().focusPanel("properties")}
        />
        <BigButton
          icon={LayoutGrid}
          label="Topology"
          onClick={() => useUIStore.getState().focusPanel("topology")}
        />
        <BigButton
          icon={RotateCcw}
          label="Reset UI"
          title="Reset the panel layout to default"
          onClick={resetDockLayout}
        />
      </RibbonGroup>
    </>
  );
}

function SimulationsTab() {
  const store = useProjectStore();
  const [importing, setImporting] = useState(false);
  const [calibrating, setCalibrating] = useState(false);
  const cases = store.project?.cases ?? [];
  const activeCase = cases.find((c) => c.id === store.activeCaseId);
  const [labelOpen, setLabelOpen] = useState(false);
  const [testsOpen, setTestsOpen] = useState(false);
  return (
    <>
      <RibbonGroup label="Cases">
        <div className="flex flex-col justify-center gap-1 py-1">
          <div className="flex items-center gap-1">
            <select
              className="ss-input w-[150px]"
              aria-label="Case"
              value={store.activeCaseId ?? ""}
              onChange={(e) => store.setActiveCase(e.target.value)}
            >
              {cases.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
            <button className="ss-toolbtn" title="Add case" onClick={store.addCase}>
              <Plus size={14} />
            </button>
            <button
              className="ss-toolbtn"
              title="Duplicate this case"
              disabled={!store.activeCaseId}
              onClick={() => store.activeCaseId && store.duplicateCase(store.activeCaseId)}
            >
              <Copy size={14} />
            </button>
            <button
              className="ss-toolbtn"
              title="Delete this case"
              disabled={cases.length <= 1 || !store.activeCaseId}
              onClick={() => store.activeCaseId && store.removeCase(store.activeCaseId)}
            >
              <Trash2 size={14} />
            </button>
          </div>
          <div className="flex items-center gap-2 text-[11px] text-[color:var(--ss-text-dim)]">
            <span title="Solver settings for this case">
              {activeCase
                ? activeCase.kind === "lap" // the Race Track sets a lap case's run
                  ? `lap mode${(activeCase.outputEvery ?? 1) > 1 ? ` · store ×${activeCase.outputEvery}` : ""}`
                  : `${activeCase.duration}s · step ${activeCase.timeStep}s${
                      (activeCase.outputEvery ?? 1) > 1 ? ` · store ×${activeCase.outputEvery}` : ""
                    }${(activeCase.realtimeFactor ?? 0) > 0 ? ` · ${activeCase.realtimeFactor}× pacing` : ""}`
                : "—"}
            </span>
            <button
              className="ss-toolbtn border border-[color:var(--ss-border)] px-1.5 text-[11px]"
              title="Edit duration, step, decimation and pacing in the Cases & Parameters panel"
              onClick={() => useUIStore.getState().focusPanel("cases")}
            >
              <Settings2 size={12} /> Settings…
            </button>
          </div>
        </div>
      </RibbonGroup>
      <RibbonGroup label="Simulation">
        <BigButton
          icon={Play}
          label={store.running ? "Running…" : "Run"}
          accent
          disabled={store.running || !store.project}
          onClick={() => void store.run()}
        />
        <BigButton
          icon={Square}
          label="Stop"
          title="Cancel the running simulation (partial results are kept)"
          disabled={!store.running}
          onClick={store.stopRun}
        />
        <BigButton
          icon={ListChecks}
          label="Checks"
          title="Run Data Checks"
          disabled={store.checking || !store.project}
          onClick={() => void store.runDataChecks()}
        />
        <BigButton
          icon={Timer}
          label="Acceleration test"
          title="Formula Student acceleration test: runs the first Acceleration case, or adds one (75 m, staged 0.30 m behind the start line, 25 s time limit) and runs it. FS Rules 2026 v1.1 (FSG): D 5.1.1 75 m, D 5.2.3 0.30 m staging, D 9.2.1 25 s (driverless runs only). FSUK and FSAE may differ: check the current season's rules."
          disabled={store.running || !store.project}
          onClick={() => void store.runAccelerationTest()}
        />
        <BigButton
          icon={Trophy}
          label="FS events"
          title="Formula Student dynamic events: marks (or adds) an Acceleration, a Skidpad, an Autocross and an Endurance case (22 laps of the Autocross layout, about 22 km, with the driver change), runs the four and shows their points in Cases & Parameters → Formula Student points. Set each case's reference values (the best teams' time and energy) for the points. Scoring: FS Rules 2026 v1.1 (FSG) D 9, estimates, not official results; FSUK and FSAE score differently."
          disabled={store.running || !store.project}
          onClick={() => void store.runFsEvents()}
        />
        <BigButton
          icon={FileUp}
          label="Import lap"
          title="Import a lap from a data logger or a lap simulator (CSV: speed against time or distance; layouts for GPS loggers, MoTeC i2, AiM Race Studio, OpenLAP and TUM laptime-simulation) and add it as a drive cycle case, optionally repeated to a 22 km endurance with a driver change stop. LightSim gives the energy and the loads for that speed."
          disabled={store.running || !store.project}
          onClick={() => setImporting(true)}
        />
        {importing && <LapImportDialog onClose={() => setImporting(false)} />}
        <BigButton
          icon={Crosshair}
          label="Calibrate lap"
          title="Calibrate lap mode on a logged lap: LightSim builds the track from the lap's speed and lateral acceleration, fits the tyres' grip scale and the downforce (CzA) to the logged speed, and checks the prediction on a second lap, blind (lap time, speed RMS and energy errors). The files stay on your computer."
          disabled={store.running || !store.project}
          onClick={() => setCalibrating(true)}
        />
        {calibrating && <CalibrateDialog onClose={() => setCalibrating(false)} />}
      </RibbonGroup>
      <RibbonGroup label="Standard figures">
        <BigButton
          icon={Gauge}
          label="Vehicle tests"
          title="One-click vehicle tests: 0-100 and 80-120 km/h, top speed, consumption and range at constant speed, steepest grade, virtual coast-down"
          disabled={store.running || !store.project}
          onClick={() => setTestsOpen(true)}
        />
        <BigButton
          icon={Fuel}
          label="US label"
          title="US window-sticker estimate (not certified): runs the model on EPA's city and highway cycles and shows each step to the label figures"
          disabled={store.running || !store.project}
          onClick={() => setLabelOpen(true)}
        />
      </RibbonGroup>
      {labelOpen && <LabelEstimateDialog onClose={() => setLabelOpen(false)} />}
      {testsOpen && <VehicleTestsDialog onClose={() => setTestsOpen(false)} />}
    </>
  );
}

function ResultsTab() {
  const running = useProjectStore((s) => s.running);
  const run = useProjectStore((s) => s.run);
  const stopRun = useProjectStore((s) => s.stopRun);
  const project = useProjectStore((s) => s.project);
  const clearRuns = useProjectStore((s) => s.clearRuns);
  const shown = useProjectStore((s) => s.runs.length);
  const stored = useProjectStore((s) => s.storedRunCount);
  return (
    <>
      <RibbonGroup label="Simulation">
        <BigButton
          icon={Play}
          label={running ? "Running…" : "Run"}
          accent
          disabled={running || !project}
          onClick={() => void run()}
        />
        <BigButton icon={Square} label="Stop" disabled={!running} onClick={stopRun} />
      </RibbonGroup>
      <RibbonGroup label="Results">
        <div className="flex h-full flex-col justify-center gap-1 px-2 text-[11px] text-[color:var(--ss-text-dim)]">
          <span className="flex items-center gap-1" title="Runs are stored on disk with the project">
            <Gauge size={13} />
            {stored > 0
              ? `${stored} stored run${stored > 1 ? "s" : ""}`
              : shown > 0
                ? `${shown} run${shown > 1 ? "s" : ""} (not stored on disk)`
                : "No stored runs yet"}
            {stored > shown && shown > 0 && <span className="text-[10px]">· newest {shown} shown</span>}
          </span>
          <button
            className="ss-toolbtn border border-[color:var(--ss-border)] px-1.5 text-[11px] disabled:opacity-40"
            disabled={(stored === 0 && shown === 0) || running}
            onClick={() => {
              const n = Math.max(stored, shown);
              const what = n === 1 ? "the stored run" : `all ${n} stored runs`;
              void confirmDialog({
                title: "Clear results history?",
                message: `This deletes ${what} of '${project?.name ?? "this project"}' from disk. ${n === 1 ? "It" : "They"} cannot be recovered.`,
                confirmLabel: "Clear history",
                danger: true,
              }).then((ok) => {
                if (ok) void clearRuns();
              });
            }}
          >
            <Trash2 size={12} /> Clear history
          </button>
        </div>
      </RibbonGroup>
    </>
  );
}

function ParametersTab() {
  const sheet = useParameterSheet();
  const running = useProjectStore((s) => s.running);
  const run = useProjectStore((s) => s.run);
  const stopRun = useProjectStore((s) => s.stopRun);
  const project = useProjectStore((s) => s.project);
  const cases = project?.cases ?? [];
  const activeCaseId = useProjectStore((s) => s.activeCaseId);
  const setActiveCase = useProjectStore((s) => s.setActiveCase);
  const activeCase = cases.find((c) => c.id === activeCaseId);
  const overrideCount = activeCase?.parameterOverrides
    ? Object.values(activeCase.parameterOverrides).reduce((n, m) => n + Object.keys(m).length, 0)
    : 0;
  return (
    <>
      <RibbonGroup label="Case">
        <div className="flex flex-col justify-center gap-1 px-1 py-1">
          <select
            className="ss-input w-[160px]"
            value={activeCaseId ?? ""}
            onChange={(e) => setActiveCase(e.target.value)}
          >
            {cases.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
          <span className="text-[11px] text-[color:var(--ss-text-dim)]">
            {overrideCount === 0
              ? "No overrides — base parameters"
              : `${overrideCount} override${overrideCount > 1 ? "s" : ""} active`}
          </span>
        </div>
      </RibbonGroup>
      <RibbonGroup label="Parameter Studies">
        <BigButton
          icon={Sliders}
          label="Case Setup"
          title="Open the Cases & Parameters panel: per-case overrides and sweeps"
          onClick={() => useUIStore.getState().focusPanel("cases")}
        />
        <BigButton
          icon={Play}
          label={running ? "Running…" : "Run case"}
          accent
          disabled={running || !project}
          onClick={() => void run()}
        />
        <BigButton icon={Square} label="Stop" disabled={!running} onClick={stopRun} />
      </RibbonGroup>
      <RibbonGroup label="Parameter Sheet">
        <BigButton
          icon={FileDown}
          label="Export sheet"
          title="Every parameter of every part in one Excel workbook (.xlsx), tables and maps on sheets of their own"
          disabled={!project}
          onClick={sheet.exportXlsx}
        />
        <BigButton
          icon={FileSpreadsheet}
          label="Export CSV"
          title="Every parameter in one CSV file (tables as text in their cell; use .xlsx to edit tables)"
          disabled={!project}
          onClick={sheet.exportCsv}
        />
        <BigButton
          icon={FileUp}
          label="Import sheet"
          title="Read a parameter sheet back: you see every change before it is applied"
          disabled={!project || running}
          onClick={sheet.pickFile}
        />
        {sheet.elements}
        <BigButton
          icon={Download}
          label="FS template"
          title="The Formula Student example's parameter sheet, to fill in with your car's numbers"
          onClick={downloadParameterTemplate}
        />
      </RibbonGroup>
    </>
  );
}

/** Earlier versions of the open project, kept each time a save replaced one
 *  (the engine keeps the last 20). Picking one opens it as an unsaved copy:
 *  the project file on disk is never touched. */
function RestoreVersionButton() {
  const [open, setOpen] = useState(false);
  const [items, setItems] = useState<api.BackupInfo[]>([]);
  const ref = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const project = useProjectStore((s) => s.project);
  const offline = useProjectStore((s) => s.offline);
  const log = useProjectStore((s) => s.log);
  const openAsCopy = useProjectStore((s) => s.openAsCopy);
  useDismiss(open, () => setOpen(false), ref, button);

  const restore = async (backup: api.BackupInfo) => {
    setOpen(false);
    if (!project) return;
    const when = new Date(backup.savedAt).toLocaleString();
    let version: Project;
    try {
      // read it before a Save in the prompt below adds a backup (and drops the oldest)
      version = await api.fetchBackup(project.id, backup.id);
    } catch (e) {
      log("error", `Could not open the version saved ${when}: ${(e as Error).message}`);
      return;
    }
    if (!(await confirmReplaceProject(`Restoring the version saved ${when}`))) return;
    const name = `${version.name} (version of ${when})`;
    openAsCopy(
      version,
      name,
      `Opened the version of '${version.name}' saved ${when} as an unsaved copy, '${name}'. ` +
        "The project on disk is unchanged; Save keeps the copy as a new project.",
    );
  };

  return (
    <div className="relative" ref={ref}>
      <BigButton
        ref={button}
        icon={History}
        label="Restore…"
        title="Restore an earlier version of this project (opens it as an unsaved copy)"
        disabled={!project || offline}
        onClick={async () => {
          if (!open && project) {
            try {
              setItems(await api.listBackups(project.id));
            } catch (e) {
              log("error", `Cannot list earlier versions: ${(e as Error).message}`);
              setItems([]);
            }
          }
          setOpen(!open);
        }}
      />
      {open && (
        <div
          role="menu"
          aria-label="Earlier versions"
          className="absolute left-0 top-[54px] z-50 max-h-[60vh] w-[320px] overflow-auto rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] py-1 shadow-lg"
        >
          <div className="px-3 pb-1 pt-0.5 text-[10px] font-semibold uppercase tracking-wide text-[color:var(--ss-text-dim)]">
            Earlier versions, newest first
          </div>
          {items.length === 0 && (
            <div className="px-3 py-1.5 text-[12px] text-[color:var(--ss-text-dim)]">
              None yet. Each save over this project keeps the version it replaces (the last 20).
            </div>
          )}
          {items.map((b) => (
            <button
              key={b.id}
              role="menuitem"
              className="block w-full px-3 py-2 text-left hover:bg-[color:var(--ss-accent-soft)]"
              onClick={() => void restore(b)}
            >
              <div className="flex items-baseline justify-between gap-2">
                <span className="text-[12px] font-medium text-[color:var(--ss-text)]">
                  {new Date(b.savedAt).toLocaleString()}
                </span>
                <span className="shrink-0 text-[10px] text-[color:var(--ss-text-dim)]">
                  {Math.max(1, Math.round(b.bytes / 1024))} kB
                </span>
              </div>
              <p className="mt-0.5 text-[11px] leading-snug text-[color:var(--ss-text-dim)]">
                {b.name === null ? "Unreadable file" : `${b.name} · ${b.elements} element(s)`}
              </p>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/** Copy for AI (AI-30): a short Markdown summary of the model and its last
 *  run on the clipboard, to paste into any chatbot. Nothing is sent. */
function CopyForAi() {
  const [hide, setHide] = useState(false);
  const [done, setDone] = useState<string | null>(null);
  const copy = async () => {
    const s = useProjectStore.getState();
    if (!s.project) return;
    const finished = s.runs.filter((r) => r.status !== "running");
    const run = finished.find((r) => r.id === s.activeRunId) ?? finished.find((r) => !r.sweepId);
    try {
      const { text, bytes } = await api.aiOverview(s.project, run, hide);
      await copyText(text);
      const kb = (bytes / 1024).toFixed(1);
      setDone(`Copied (${kb} KB)`);
      s.log("info", `Copy for AI: a ${kb} KB summary of the model${run ? ` and its run of '${run.caseName}'` : ""} is on the clipboard${hide ? ", numbers hidden" : ""}. Paste it into any chatbot; LightSim sent nothing.`);
      window.setTimeout(() => setDone(null), 4000);
    } catch (e) {
      s.log("error", `Copy for AI failed: ${(e as Error).message ?? e}`);
    }
  };
  return (
    <div className="flex items-center gap-1">
      <BigButton
        icon={ClipboardCopy}
        label={done ? "Copied" : "Copy for AI"}
        title="Copy a short summary of the model and its last run, to paste into an AI chatbot (nothing is sent)"
        onClick={() => void copy()}
      />
      <label className="flex flex-col items-start gap-0.5 text-[11px] text-[color:var(--ss-text-dim)]">
        <span className="flex items-center gap-1">
          <input type="checkbox" checked={hide} onChange={(e) => setHide(e.target.checked)} />
          Hide values
        </span>
        <span role="status" className="min-h-[14px]">{done ?? ""}</span>
      </label>
      <BigButton
        icon={Bot}
        label="Connect AI"
        title="Connect an AI assistant (Claude, Copilot, Codex, Gemini, Cursor) to LightSim"
        onClick={() => useUIStore.getState().setAiConnectOpen(true)}
      />
    </div>
  );
}

/** The clipboard API, with the old copy command where it is not allowed. */
async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    return;
  } catch {
    const area = document.createElement("textarea");
    area.value = text;
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.appendChild(area);
    area.select();
    const ok = document.execCommand("copy");
    area.remove();
    if (!ok) throw new Error("the clipboard is not available");
  }
}

function ProjectTab() {
  const project = useProjectStore((s) => s.project);
  const renameSystem = useProjectStore((s) => s.renameSystem);
  const root = project?.systems.find((s) => s.parentId === null);
  const [cardOpen, setCardOpen] = useState(false);
  return (
    <>
      <RibbonGroup label="Project Settings">
        <div className="flex items-center gap-2 px-1 py-2">
          <span className="text-[11px] text-[color:var(--ss-text-dim)]">Project name</span>
          <input
            className="ss-input w-[220px]"
            aria-label="Project name"
            value={project?.name ?? ""}
            onChange={(e) => root && renameSystem(root.id, e.target.value)}
          />
          <span className="text-[11px] text-[color:var(--ss-text-dim)]">
            {project ? `${project.systems.length} system(s), ${project.cases.length} case(s)` : ""}
          </span>
        </div>
      </RibbonGroup>
      <RibbonGroup label="Versions">
        <RestoreVersionButton />
      </RibbonGroup>
      <RibbonGroup label="Example">
        <BigButton
          icon={IdCard}
          label="Card…"
          title="The project's card: the question it answers, its difficulty, what you learn, what happens when and how far its results are checked"
          disabled={!project}
          onClick={() => setCardOpen(true)}
        />
        {cardOpen && <ExampleCardDialog onClose={() => setCardOpen(false)} />}
      </RibbonGroup>
      <RibbonGroup label="Files">
        <AttachmentsButton />
      </RibbonGroup>
      <ProjectFileNote />
      <RibbonGroup label="AI assistants">
        <CopyForAi />
      </RibbonGroup>
    </>
  );
}

/** Where the open project is saved, and whether it is read-only. */
function ProjectFileNote() {
  const filePath = useProjectStore((s) => s.filePath);
  const readOnly = useProjectStore((s) => s.readOnly);
  const revision = useProjectStore((s) => s.revision);
  const exampleId = useProjectStore((s) => s.exampleId);
  const where = filePath
    ? filePath
    : exampleId
      ? "A copy of an example, not saved yet"
      : revision
        ? "LightSim's projects folder (File → Open Projects Folder)"
        : "Not saved yet";
  return (
    <div className="flex max-w-[420px] flex-col justify-center gap-0.5 px-2 text-[11px] text-[color:var(--ss-text-dim)]">
      <span>
        Saved in: <span className="text-[color:var(--ss-text)] [overflow-wrap:anywhere]">{where}</span>
      </span>
      {readOnly && (
        <span role="status" className="font-semibold text-[color:var(--ss-warn)]">
          Read-only: {readOnly}
        </span>
      )}
    </div>
  );
}

/** Files kept with the project (STD-02): FMUs, AI models, measured data. Each
 *  is copied into the project's resources folder; the list shows whether a
 *  file is missing or changed since it was attached. */
function AttachmentsButton() {
  const [open, setOpen] = useState(false);
  const [onDisk, setOnDisk] = useState<api.AttachedFile[]>([]);
  const ref = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const project = useProjectStore((s) => s.project);
  const offline = useProjectStore((s) => s.offline);
  const readOnly = useProjectStore((s) => s.readOnly);
  const attachFile = useProjectStore((s) => s.attachFile);
  const detachFile = useProjectStore((s) => s.detachFile);
  useDismiss(open, () => setOpen(false), ref, button);
  const listed = project?.attachments ?? [];
  const refresh = async () => {
    if (project) setOnDisk((await api.listAttachments(project.id).catch(() => [])) ?? []);
  };
  const status = (path: string, sha256: string) => {
    const file = onDisk.find((f) => f.path === path);
    return !file ? "missing" : file.sha256 !== sha256 ? "changed since attached" : "";
  };

  return (
    <div className="relative" ref={ref}>
      <BigButton
        ref={button}
        icon={Paperclip}
        label="Attached"
        title="Files kept with the project: other tools' models (FMUs), AI models, measured data"
        disabled={!project || offline}
        onClick={async () => {
          if (!open) await refresh();
          setOpen(!open);
        }}
      />
      {open && (
        <div
          role="menu"
          aria-label="Attached files"
          className="absolute left-0 top-[54px] z-50 max-h-[60vh] w-[360px] overflow-auto rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] py-1 shadow-lg"
        >
          <MenuHeading>Attached files</MenuHeading>
          {listed.length === 0 && (
            <MenuNote>
              None. An attached file is copied into the project&apos;s resources folder and goes with
              the project when it is saved, exported or moved.
            </MenuNote>
          )}
          {listed.map((a) => {
            const problem = status(a.path, a.sha256);
            return (
              <div key={a.path} className="flex items-center gap-2 px-3 py-1.5 text-[12px]">
                <Paperclip size={12} className="shrink-0 text-[color:var(--ss-text-dim)]" />
                <span className="min-w-0 flex-1 [overflow-wrap:anywhere]">
                  {a.path.replace(/^resources\//, "")}
                  <span className="ml-1 text-[10px] text-[color:var(--ss-text-dim)]">
                    {Math.max(1, Math.round(a.bytes / 1024)).toLocaleString("en")} kB
                  </span>
                  {problem && <span className="ml-1 text-[10px] font-semibold text-[color:var(--ss-warn)]">{problem}</span>}
                </span>
                <button
                  role="menuitem"
                  className="shrink-0 px-1 text-[color:var(--ss-text-dim)] hover:text-[color:var(--ss-text)]"
                  aria-label={`Remove '${a.path}'`}
                  title="Remove it from the project and delete it from the resources folder"
                  disabled={Boolean(readOnly)}
                  onClick={async () => {
                    const ok = await confirmDialog({
                      title: "Remove the attached file?",
                      message: `'${a.path}' is taken off the project and deleted from its resources folder.`,
                      confirmLabel: "Remove",
                      danger: true,
                    });
                    if (!ok) return;
                    await detachFile(a.path);
                    await refresh();
                  }}
                >
                  <Trash2 size={12} />
                </button>
              </div>
            );
          })}
          <button
            role="menuitem"
            className="mt-1 block w-full border-t border-[color:var(--ss-border)] px-3 py-1.5 text-left text-[12px] text-[color:var(--ss-accent)] hover:bg-[color:var(--ss-accent-soft)] disabled:opacity-40"
            disabled={Boolean(readOnly)}
            onClick={() => input.current?.click()}
          >
            Attach a file…
          </button>
          <input
            ref={input}
            type="file"
            aria-label="Attach a file"
            className="hidden"
            onChange={async (e) => {
              const el = e.target;
              const f = el.files?.[0];
              el.value = "";
              if (!f) return;
              await attachFile(f);
              await refresh();
            }}
          />
        </div>
      )}
    </div>
  );
}

function StubTab({ name }: { name: string }) {
  return (
    <div className="flex items-center gap-2 px-3 text-[12px] text-[color:var(--ss-text-dim)]">
      <CheckCircle2 size={14} />
      {name} is not part of LightSim v1 — this ribbon tab is a visual stub.
    </div>
  );
}

/** Always-visible run control (active case + Run/Stop + live progress) pinned
 *  to the ribbon header so a run is one click away from any tab. */
function GlobalRunControl() {
  const running = useProjectStore((s) => s.running);
  const run = useProjectStore((s) => s.run);
  const stopRun = useProjectStore((s) => s.stopRun);
  const project = useProjectStore((s) => s.project);
  const cases = project?.cases ?? [];
  const activeCaseId = useProjectStore((s) => s.activeCaseId);
  const setActiveCase = useProjectStore((s) => s.setActiveCase);
  const livePct = useProjectStore((s) => s.livePct);
  return (
    <div className="flex items-center gap-1">
      <select
        className="ss-input max-w-[150px] py-[3px] text-[11px]"
        value={activeCaseId ?? ""}
        onChange={(e) => setActiveCase(e.target.value)}
        title="Active simulation case"
        disabled={running}
      >
        {cases.map((c) => (
          <option key={c.id} value={c.id}>
            {c.name}
          </option>
        ))}
      </select>
      {running ? (
        <button
          className="flex items-center gap-1 rounded bg-red-600 px-2 py-[3px] text-[11px] font-semibold text-white hover:bg-red-700"
          onClick={stopRun}
          title="Stop the running simulation"
        >
          <Square size={11} /> Stop {livePct.toFixed(0)}%
        </button>
      ) : (
        <button
          className="flex items-center gap-1 rounded bg-[color:var(--ss-accent-fill)] px-2 py-[3px] text-[11px] font-semibold text-white hover:brightness-110 disabled:opacity-40"
          onClick={() => void run()}
          disabled={!project}
          title="Run the active case (Ctrl+Enter)"
          data-tour="run"
        >
          <Play size={11} /> Run
        </button>
      )}
    </div>
  );
}

/** Interface font-size / UI-scale stepper. Applies CSS zoom to chrome + panels
 *  (never the canvas); the click-to-reset readout doubles as the value display. */
function FontSizeControl() {
  const fontScale = useUIStore((s) => s.fontScale);
  const nudge = useUIStore((s) => s.nudgeFontScale);
  const setScale = useUIStore((s) => s.setFontScale);
  return (
    <div className="flex items-center" role="group" aria-label="Interface font size">
      <button
        className="rounded p-1 hover:bg-[color:var(--ss-hover)] disabled:opacity-40 disabled:hover:bg-transparent"
        title="Decrease interface size"
        aria-label="Decrease interface size"
        disabled={fontScale <= FONT_SCALE_MIN + 1e-6}
        onClick={() => nudge(-FONT_SCALE_STEP)}
      >
        <AArrowDown size={14} />
      </button>
      <button
        className="min-w-[38px] rounded px-1 py-0.5 text-center text-[11px] tabular-nums hover:bg-[color:var(--ss-hover)]"
        title="Reset interface size to 100%"
        aria-label={`Interface size ${Math.round(fontScale * 100)} percent. Activate to reset to 100 percent.`}
        onClick={() => setScale(1)}
      >
        {Math.round(fontScale * 100)}%
      </button>
      <button
        className="rounded p-1 hover:bg-[color:var(--ss-hover)] disabled:opacity-40 disabled:hover:bg-transparent"
        title="Increase interface size"
        aria-label="Increase interface size"
        disabled={fontScale >= FONT_SCALE_MAX - 1e-6}
        onClick={() => nudge(FONT_SCALE_STEP)}
      >
        <AArrowUp size={14} />
      </button>
    </div>
  );
}

export function Ribbon() {
  const tab = useUIStore((s) => s.ribbonTab);
  const setTab = useUIStore((s) => s.setRibbonTab);
  const theme = useUIStore((s) => s.theme);
  const toggleTheme = useUIStore((s) => s.toggleTheme);
  const projectName = useProjectStore((s) => s.project?.name);
  const dirty = useProjectStore((s) => s.dirty);

  return (
    <div className="ss-zoom shrink-0 border-b border-[color:var(--ss-border)] bg-[color:var(--ss-chrome)]">
      <div className="flex items-center gap-1 px-2 pt-1">
        <div className="mr-1 flex items-center gap-1.5 rounded bg-[color:var(--ss-accent-fill)] px-2 py-0.5 text-[12px] font-semibold text-white">
          LightSim
        </div>
        {TABS.map((t) => (
          <button
            key={t.id}
            className={`rounded-t px-3 py-1 text-[12px] ${
              tab === t.id
                ? "border border-b-0 border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] font-semibold text-[color:var(--ss-accent)]"
                : "text-[color:var(--ss-text)] hover:bg-[color:var(--ss-hover)]"
            }`}
            onClick={() => setTab(t.id)}
            data-tour={`tab-${t.id}`}
          >
            {t.label}
          </button>
        ))}
        <div className="ml-auto flex items-center gap-2 pr-1 text-[11px] text-[color:var(--ss-text-dim)]">
          <GlobalRunControl />
          <div className="h-4 w-px bg-[color:var(--ss-border)]" />
          <span className="max-w-[160px] truncate">
            {projectName}
            {dirty ? " •" : ""}
          </span>
          <div className="h-4 w-px bg-[color:var(--ss-border)]" />
          <FontSizeControl />
          <HelpMenu />
          <button
            className="rounded p-1 hover:bg-[color:var(--ss-hover)]"
            title={theme === "dark" ? "Switch to light mode" : "Switch to dark mode"}
            aria-label={theme === "dark" ? "Switch to light mode" : "Switch to dark mode"}
            onClick={toggleTheme}
          >
            {theme === "dark" ? <Sun size={13} /> : <Moon size={13} />}
          </button>
        </div>
      </div>
      {/* the Start page has no commands of its own: no band */}
      {tab !== "start" && (
        <div className="flex h-[72px] items-stretch border-t border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] px-1">
          {tab === "home" && <HomeTab />}
          {tab === "simulations" && <SimulationsTab />}
          {tab === "results" && <ResultsTab />}
          {tab === "parameters" && <ParametersTab />}
          {tab === "project" && <ProjectTab />}
          {tab === "optimization" && <StubTab name="Optimization" />}
        </div>
      )}
    </div>
  );
}
