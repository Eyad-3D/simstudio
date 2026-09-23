import { useEffect, useRef, useState } from "react";
import {
  AArrowDown,
  AArrowUp,
  CheckCircle2,
  Copy,
  Download,
  FilePlus2,
  FolderOpen,
  Gauge,
  LayoutGrid,
  ListChecks,
  Moon,
  Play,
  Plus,
  Redo2,
  RotateCcw,
  Save,
  Settings2,
  Sliders,
  Square,
  Sun,
  Trash2,
  Undo2,
  Upload,
} from "lucide-react";
import * as api from "../api";
import { resetDockLayout } from "./DockLayout";
import { confirmDialog, confirmReplaceProject } from "../dialog";
import { useProjectStore } from "../store/projectStore";
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
}: {
  icon: React.ComponentType<{ size?: number; className?: string }>;
  label: string;
  onClick?: () => void;
  disabled?: boolean;
  accent?: boolean;
  title?: string;
}) {
  return (
    <button
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

function OpenProjectButton() {
  const [open, setOpen] = useState(false);
  const [items, setItems] = useState<
    { id: string; name: string; description?: string | null }[]
  >([]);
  const ref = useRef<HTMLDivElement>(null);
  const openProject = useProjectStore((s) => s.openProject);
  const log = useProjectStore((s) => s.log);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);

  return (
    <div className="relative" ref={ref}>
      <BigButton
        icon={FolderOpen}
        label="Open"
        onClick={async () => {
          if (!open) {
            try {
              setItems(await api.listProjects());
            } catch (e) {
              log("error", `Cannot list projects: ${(e as Error).message}`);
              setItems([]);
            }
          }
          setOpen(!open);
        }}
      />
      {open && (
        <div
          role="menu"
          aria-label="Open project"
          className="absolute left-0 top-[54px] z-50 max-h-[60vh] w-[320px] overflow-auto rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] py-1 shadow-lg"
        >
          <div className="px-3 pb-1 pt-0.5 text-[10px] font-semibold uppercase tracking-wide text-[color:var(--ss-text-dim)]">
            Example & saved projects
          </div>
          {items.length === 0 && (
            <div className="px-3 py-1.5 text-[12px] text-[color:var(--ss-text-dim)]">
              No projects on server
            </div>
          )}
          {items.map((p) => (
            <button
              key={p.id}
              role="menuitem"
              className="block w-full px-3 py-2 text-left hover:bg-[color:var(--ss-accent-soft)]"
              onClick={() => {
                setOpen(false);
                void confirmReplaceProject(`Opening '${p.name}'`).then((ok) => {
                  if (ok) void openProject(p.id);
                });
              }}
            >
              <div className="flex items-baseline justify-between gap-2">
                <span className="text-[12px] font-medium text-[color:var(--ss-text)]">{p.name}</span>
                <span className="shrink-0 text-[10px] text-[color:var(--ss-text-dim)]">{p.id}</span>
              </div>
              {p.description && (
                <p className="mt-0.5 text-[11px] leading-snug text-[color:var(--ss-text-dim)]">
                  {p.description}
                </p>
              )}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function HomeTab() {
  const store = useProjectStore();
  const fileRef = useRef<HTMLInputElement>(null);
  return (
    <>
      <RibbonGroup label="Project">
        <BigButton
          icon={FilePlus2}
          label="New"
          onClick={() => void confirmReplaceProject("Creating a new project").then((ok) => ok && store.newProject())}
        />
        <OpenProjectButton />
        <BigButton icon={Save} label="Save" onClick={() => void store.saveRemote()} />
        <BigButton icon={Download} label="Export" onClick={store.exportProject} title="Download project as JSON" />
        <BigButton icon={Upload} label="Import" onClick={() => fileRef.current?.click()} title="Import project JSON" />
        <input
          ref={fileRef}
          type="file"
          accept=".json,application/json"
          className="hidden"
          onChange={async (e) => {
            const input = e.target;
            const f = input.files?.[0];
            input.value = "";
            if (!f) return;
            const text = await f.text();
            if (await confirmReplaceProject(`Importing '${f.name}'`)) store.importProject(text);
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
  const cases = store.project?.cases ?? [];
  const activeCase = cases.find((c) => c.id === store.activeCaseId);
  return (
    <>
      <RibbonGroup label="Cases">
        <div className="flex flex-col justify-center gap-1 py-1">
          <div className="flex items-center gap-1">
            <select
              className="ss-input w-[150px]"
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
                ? `${activeCase.duration}s · step ${activeCase.timeStep}s${
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
      </RibbonGroup>
    </>
  );
}

function ResultsTab() {
  const running = useProjectStore((s) => s.running);
  const run = useProjectStore((s) => s.run);
  const stopRun = useProjectStore((s) => s.stopRun);
  const project = useProjectStore((s) => s.project);
  const clearRuns = useProjectStore((s) => s.clearRuns);
  const count = useProjectStore((s) => s.runs.length);
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
          <span className="flex items-center gap-1">
            <Gauge size={13} />
            {count === 0 ? "No stored runs yet" : `${count} stored run${count > 1 ? "s" : ""}`}
            {count >= 20 && <span className="text-[10px]">(max)</span>}
          </span>
          <button
            className="ss-toolbtn border border-[color:var(--ss-border)] px-1.5 text-[11px] disabled:opacity-40"
            disabled={count === 0 || running}
            onClick={() => {
              void confirmDialog({
                title: "Clear results history?",
                message: "This removes all stored runs. Their data cannot be recovered.",
                confirmLabel: "Clear history",
                danger: true,
              }).then((ok) => ok && clearRuns());
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
    </>
  );
}

function ProjectTab() {
  const project = useProjectStore((s) => s.project);
  const renameSystem = useProjectStore((s) => s.renameSystem);
  const root = project?.systems.find((s) => s.parentId === null);
  return (
    <RibbonGroup label="Project Settings">
      <div className="flex items-center gap-2 px-1 py-2">
        <span className="text-[11px] text-[color:var(--ss-text-dim)]">Project name</span>
        <input
          className="ss-input w-[220px]"
          value={project?.name ?? ""}
          onChange={(e) => root && renameSystem(root.id, e.target.value)}
        />
        <span className="text-[11px] text-[color:var(--ss-text-dim)]">
          {project ? `${project.systems.length} system(s), ${project.cases.length} case(s)` : ""}
        </span>
      </div>
    </RibbonGroup>
  );
}

function StubTab({ name }: { name: string }) {
  return (
    <div className="flex items-center gap-2 px-3 text-[12px] text-[color:var(--ss-text-dim)]">
      <CheckCircle2 size={14} />
      {name} is not part of SimStudio v1 — this ribbon tab is a visual stub.
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
          className="flex items-center gap-1 rounded bg-[color:var(--ss-accent)] px-2 py-[3px] text-[11px] font-semibold text-white hover:brightness-110 disabled:opacity-40"
          onClick={() => void run()}
          disabled={!project}
          title="Run the active case (Ctrl+Enter)"
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
        <div className="mr-1 flex items-center gap-1.5 rounded bg-[color:var(--ss-accent)] px-2 py-0.5 text-[12px] font-semibold text-white">
          SimStudio
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
      <div className="flex h-[72px] items-stretch border-t border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] px-1">
        {tab === "home" && <HomeTab />}
        {tab === "simulations" && <SimulationsTab />}
        {tab === "results" && <ResultsTab />}
        {tab === "parameters" && <ParametersTab />}
        {tab === "project" && <ProjectTab />}
        {tab === "optimization" && <StubTab name="Optimization" />}
      </div>
    </div>
  );
}
