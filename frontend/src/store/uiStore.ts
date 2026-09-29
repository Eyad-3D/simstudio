import { create } from "zustand";
import type { DockviewApi } from "dockview-react";
import type { XAxisMode, YAxisCfg } from "../components/panels/chartUtils";
import { FONT_SCALE_KEY, OPEN_LAST_KEY, RESULTS_VIEW_KEY, THEME_KEY } from "../storageKeys";

export type RibbonTab =
  | "start"
  | "project"
  | "home"
  | "simulations"
  | "results"
  | "optimization"
  | "parameters";

// Canvas connection layers. Electrical/mechanical are real React Flow edges;
// "signal" toggles a dashed data-bus overlay (drawn from dataBusConnections).
export type EdgeKindFilter = "electrical" | "mechanical" | "signal";

export type Theme = "light" | "dark";

/** What the Results page shows for one case; an unset field takes the page's
 *  default. Kept per project and case across runs and reloads (RES-19). */
export interface PlotView {
  /** ticked channel keys, in the order ticked (their colours follow it) */
  channels?: string[];
  view?: "chart" | "table" | "xy" | "sweep";
  /** the X-Y view's X channel */
  xKey?: string;
  /** the summary value the Sweep view plots */
  sweepMetric?: string;
  /** the chart's x axis and each unit's y axis (RES-18) */
  xAxis?: XAxisMode;
  yAxes?: Record<string, YAxisCfg>;
  /** the chart's zoom, in s or m; unset: the whole run */
  zoom?: { kind: "t" | "distance"; min: number; max: number };
  /** the run the numbers are compared with: unset, the previous run of the
   *  case; null, none (RES-10) */
  baselineRunId?: string | null;
}

/** A project's Results choices: a PlotView per case, and whether the
 *  baseline run (the previous run of the case, unless another is picked) is
 *  drawn faint under the lines (unset: yes). */
export interface ResultsView {
  comparePrevious?: boolean;
  cases?: Record<string, PlotView>;
}

// ponytail: the 50 projects changed last keep their choices (about 120 B a
// case); plenty for one person, and it keeps the recovery draft's storage free
const RESULTS_VIEWS_KEPT = 50;

/** A request to show the app's styled confirm/prompt modal (replaces the
 *  native window.confirm/prompt). `resolve` settles the caller's promise. */
export interface DialogRequest {
  kind: "confirm" | "prompt";
  title: string;
  message?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  /** confirm only: a third button, left of Cancel; resolves with "alt" */
  altLabel?: string;
  danger?: boolean;
  defaultValue?: string; // prompt only
  placeholder?: string; // prompt only
  resolve: (value: boolean | string | null) => void;
}

/** UI scale steps for the font-size setting. Applied as CSS `zoom` on the
 *  chrome/panels (never the canvas — see index.css / DockLayout). */
export const FONT_SCALE_MIN = 0.85;
export const FONT_SCALE_MAX = 1.4;
export const FONT_SCALE_STEP = 0.1;

function loadTheme(): Theme {
  try {
    const saved = window.localStorage.getItem(THEME_KEY);
    if (saved === "dark" || saved === "light") return saved;
  } catch {
    /* storage unavailable */
  }
  return "light";
}

function applyTheme(theme: Theme) {
  document.documentElement.classList.toggle("dark", theme === "dark");
}

function clampScale(n: number): number {
  const v = Math.round(n * 100) / 100;
  return Math.min(FONT_SCALE_MAX, Math.max(FONT_SCALE_MIN, v));
}

function loadFontScale(): number {
  try {
    const saved = Number(window.localStorage.getItem(FONT_SCALE_KEY));
    if (Number.isFinite(saved) && saved > 0) return clampScale(saved);
  } catch {
    /* storage unavailable */
  }
  return 1;
}

function applyFontScale(scale: number) {
  document.documentElement.style.setProperty("--ss-ui-scale", String(scale));
}

function loadResultsViews(): Record<string, ResultsView> {
  try {
    const saved: unknown = JSON.parse(window.localStorage.getItem(RESULTS_VIEW_KEY) ?? "{}");
    if (saved && typeof saved === "object" && !Array.isArray(saved)) return saved as Record<string, ResultsView>;
  } catch {
    /* storage unavailable or unreadable: the page's defaults */
  }
  return {};
}

/** `views` with one project's entry changed by `fn` and moved last (the
 *  newest), the oldest beyond RESULTS_VIEWS_KEPT dropped, and saved. */
function changeResultsView(
  views: Record<string, ResultsView>,
  projectId: string,
  fn: (view: ResultsView) => ResultsView,
): Record<string, ResultsView> {
  const { [projectId]: view = {}, ...others } = views;
  const next = Object.fromEntries([...Object.entries(others).slice(-(RESULTS_VIEWS_KEPT - 1)), [projectId, fn(view)]]);
  try {
    window.localStorage.setItem(RESULTS_VIEW_KEY, JSON.stringify(next));
  } catch {
    /* storage unavailable: the choices last for this session */
  }
  return next;
}

function loadOpenLast(): boolean {
  try {
    return window.localStorage.getItem(OPEN_LAST_KEY) === "1";
  } catch {
    return false;
  }
}

interface UIState {
  ribbonTab: RibbonTab;
  setRibbonTab: (tab: RibbonTab) => void;
  /** Skip the Start page: open on Home with the last project (UX-16). */
  openLastAtStart: boolean;
  setOpenLastAtStart: (on: boolean) => void;

  dockApi: DockviewApi | null;
  setDockApi: (api: DockviewApi) => void;
  /** Bring a dockview panel to the front of its group (opening the bottom
   *  tray or leaving a maximised diagram when needed). */
  focusPanel: (id: string) => void;

  /** Layer Configurations: per-kind edge visibility on the canvas. */
  visibleKinds: Record<EdgeKindFilter, boolean>;
  toggleKind: (kind: EdgeKindFilter) => void;

  /** Library part armed for click-to-place: the next click on the empty
   *  diagram adds it there (Components panel: click a part, then the canvas). */
  placingComponentId: string | null;
  setPlacingComponent: (defId: string | null) => void;
  /** Registered by the topology canvas: add a library part in the middle of the
   *  visible diagram and select it; returns the new element's label. */
  insertComponent: ((defId: string) => string | null) | null;
  setInsertComponent: (fn: ((defId: string) => string | null) | null) => void;
  /** Registered by the topology canvas: select these parts and pan and zoom
   *  the diagram to them, opening the sub-system they are in. */
  revealElements: ((ids: string[]) => void) | null;
  setRevealElements: (fn: ((ids: string[]) => void) | null) => void;
  /** Data Bus Connections: list only the selected part's signals. */
  busSelectedOnly: boolean;
  setBusSelectedOnly: (on: boolean) => void;

  /** Live-value overlay chips on canvas nodes (fed from the live run stream). */
  showLiveValues: boolean;
  toggleLiveValues: () => void;

  /** Element whose parameters are open in the modal dialog (double-click),
   *  and the table it opens at (a narrow panel's table button). */
  paramDialogId: string | null;
  paramDialogKey: string | null;
  openParamDialog: (elementId: string, paramKey?: string) => void;
  closeParamDialog: () => void;

  /** Styled confirm/prompt modal (see dialog.ts helpers). */
  dialog: DialogRequest | null;
  openDialog: (req: DialogRequest) => void;
  closeDialog: () => void;

  theme: Theme;
  toggleTheme: () => void;

  /** UI/font scale (CSS zoom on chrome + panels). 1 = default. */
  fontScale: number;
  setFontScale: (scale: number) => void;
  nudgeFontScale: (delta: number) => void;

  /** Results' measurement cursors A and B per run, as times (s) on the
   *  run's samples; a run without an entry has them off. Kept for the
   *  session, not saved (RES-06). */
  cursors: Record<string, [number, number]>;
  setCursors: (runId: string, ab: [number, number] | null) => void;

  /** The Results page's choices by project id, saved in localStorage (RES-19). */
  resultsViews: Record<string, ResultsView>;
  setPlotView: (projectId: string, caseId: string, patch: Partial<PlotView>) => void;
  setComparePrevious: (projectId: string, on: boolean) => void;
}

const initialTheme = loadTheme();
applyTheme(initialTheme);
const initialFontScale = loadFontScale();
applyFontScale(initialFontScale);
const initialOpenLast = loadOpenLast();

export const useUIStore = create<UIState>((set, get) => ({
  // every launch starts on the Start page unless the user chose to skip it
  ribbonTab: initialOpenLast ? "home" : "start",
  setRibbonTab: (tab) => set({ ribbonTab: tab }),
  openLastAtStart: initialOpenLast,
  setOpenLastAtStart: (on) => {
    try {
      window.localStorage.setItem(OPEN_LAST_KEY, on ? "1" : "0");
    } catch {
      /* storage unavailable: this session only */
    }
    set({ openLastAtStart: on });
  },

  dockApi: null,
  setDockApi: (api) => set({ dockApi: api }),
  focusPanel: (id) => {
    // the Start page hides the workspace: a panel asked for (an error, a
    // blocked run, an empty state's button) brings the workspace back
    if (get().ribbonTab === "start") set({ ribbonTab: "home" });
    const dock = get().dockApi;
    const panel = dock?.getPanel(id);
    if (!dock || !panel) return;
    const group = panel.group.api;
    // a maximised diagram would hide the other grid panels
    if (group.location.type === "grid" && dock.hasMaximizedGroup() && !group.isMaximized()) {
      dock.exitMaximizedGroup();
    }
    panel.api.setActive();
    // panels in the bottom tray: open the tray if it is collapsed to its tabs
    if (group.location.type === "edge" && group.isCollapsed()) group.expand();
  },

  visibleKinds: { electrical: true, mechanical: true, signal: false },
  toggleKind: (kind) =>
    set((s) => ({
      visibleKinds: { ...s.visibleKinds, [kind]: !s.visibleKinds[kind] },
    })),

  placingComponentId: null,
  setPlacingComponent: (defId) => set({ placingComponentId: defId }),
  insertComponent: null,
  setInsertComponent: (fn) => set({ insertComponent: fn }),
  revealElements: null,
  setRevealElements: (fn) => set({ revealElements: fn }),
  busSelectedOnly: false,
  setBusSelectedOnly: (on) => set({ busSelectedOnly: on }),

  showLiveValues: true,
  toggleLiveValues: () => set((s) => ({ showLiveValues: !s.showLiveValues })),

  paramDialogId: null,
  paramDialogKey: null,
  openParamDialog: (elementId, paramKey) =>
    set({ paramDialogId: elementId, paramDialogKey: paramKey ?? null }),
  closeParamDialog: () => set({ paramDialogId: null, paramDialogKey: null }),

  dialog: null,
  openDialog: (req) => set({ dialog: req }),
  closeDialog: () => set({ dialog: null }),

  theme: initialTheme,
  toggleTheme: () => {
    const next: Theme = get().theme === "dark" ? "light" : "dark";
    applyTheme(next);
    try {
      window.localStorage.setItem(THEME_KEY, next);
    } catch {
      /* storage unavailable */
    }
    set({ theme: next });
  },

  fontScale: initialFontScale,
  setFontScale: (scale) => {
    const next = clampScale(scale);
    applyFontScale(next);
    try {
      window.localStorage.setItem(FONT_SCALE_KEY, String(next));
    } catch {
      /* storage unavailable */
    }
    set({ fontScale: next });
  },
  nudgeFontScale: (delta) => get().setFontScale(get().fontScale + delta),

  cursors: {},
  setCursors: (runId, ab) =>
    set((s) => {
      const cursors = { ...s.cursors };
      if (ab) cursors[runId] = ab;
      else delete cursors[runId];
      return { cursors };
    }),

  resultsViews: loadResultsViews(),
  setPlotView: (projectId, caseId, patch) =>
    set((s) => ({
      resultsViews: changeResultsView(s.resultsViews, projectId, (v) => ({
        ...v,
        cases: { ...v.cases, [caseId]: { ...v.cases?.[caseId], ...patch } },
      })),
    })),
  setComparePrevious: (projectId, on) =>
    set((s) => ({
      resultsViews: changeResultsView(s.resultsViews, projectId, (v) => ({ ...v, comparePrevious: on })),
    })),
}));
