import { create } from "zustand";
import type { DockviewApi } from "dockview-react";
import { FONT_SCALE_KEY, THEME_KEY } from "../storageKeys";

export type RibbonTab =
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

interface UIState {
  ribbonTab: RibbonTab;
  setRibbonTab: (tab: RibbonTab) => void;

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
}

const initialTheme = loadTheme();
applyTheme(initialTheme);
const initialFontScale = loadFontScale();
applyFontScale(initialFontScale);

export const useUIStore = create<UIState>((set, get) => ({
  ribbonTab: "home",
  setRibbonTab: (tab) => set({ ribbonTab: tab }),

  dockApi: null,
  setDockApi: (api) => set({ dockApi: api }),
  focusPanel: (id) => {
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
}));
