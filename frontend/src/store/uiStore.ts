import { create } from "zustand";
import type { DockviewApi } from "dockview-react";

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

const THEME_KEY = "simstudio-theme";
const FONT_SCALE_KEY = "simstudio-font-scale";

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
  /** Bring a dockview panel to the front of its group. */
  focusPanel: (id: string) => void;

  /** Layer Configurations: per-kind edge visibility on the canvas. */
  visibleKinds: Record<EdgeKindFilter, boolean>;
  toggleKind: (kind: EdgeKindFilter) => void;

  /** Live-value overlay chips on canvas nodes (fed from the live run stream). */
  showLiveValues: boolean;
  toggleLiveValues: () => void;

  /** Element whose parameters are open in the modal dialog (double-click). */
  paramDialogId: string | null;
  openParamDialog: (elementId: string) => void;
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
    const panel = get().dockApi?.getPanel(id);
    panel?.api.setActive();
  },

  visibleKinds: { electrical: true, mechanical: true, signal: false },
  toggleKind: (kind) =>
    set((s) => ({
      visibleKinds: { ...s.visibleKinds, [kind]: !s.visibleKinds[kind] },
    })),

  showLiveValues: true,
  toggleLiveValues: () => set((s) => ({ showLiveValues: !s.showLiveValues })),

  paramDialogId: null,
  openParamDialog: (elementId) => set({ paramDialogId: elementId }),
  closeParamDialog: () => set({ paramDialogId: null }),

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
