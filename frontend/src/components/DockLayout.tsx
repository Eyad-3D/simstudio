import { useEffect, useRef, useState } from "react";
import {
  DockviewReact,
  themeLight,
  type DockviewApi,
  type DockviewReadyEvent,
  type DockviewTheme,
  type IDockviewHeaderActionsProps,
  type IDockviewPanelHeaderProps,
  type IDockviewPanelProps,
} from "dockview-react";
import { ChevronDown, ChevronUp } from "lucide-react";
import { LAYOUT_KEY } from "../storageKeys";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { ComponentsPanel } from "./panels/ComponentsPanel";
import { ElementsPanel } from "./panels/ElementsPanel";
import { PropertiesPanel } from "./panels/PropertiesPanel";
import { MessagesPanel } from "./panels/MessagesPanel";
import { DataChecksPanel } from "./panels/DataChecksPanel";
import { LayerConfigPanel } from "./panels/LayerConfigPanel";
import { DataBusPanel } from "./panels/DataBusPanel";
import { MonitorsPanel } from "./panels/MonitorsPanel";
import { CasePanel } from "./panels/CasePanel";
import { MiniChartPanel } from "./panels/MiniChartPanel";
import { TopologyCanvas } from "./canvas/TopologyCanvas";

// the tray collapses to its tab strip (--dv-tabs-and-actions-container-height)
const TAB_STRIP = 34;

const ssTheme: DockviewTheme = {
  ...themeLight,
  name: "lightsim",
  className: `${themeLight.className} dockview-theme-ss`,
  edgeGroupCollapsedSize: TAB_STRIP,
};

// `zoom: false` opts a panel out of the font-size/UI-scale magnification. Only
// the topology canvas does so — React Flow's pointer math assumes an unscaled
// ancestor (CSS zoom would offset drags); every other panel scales via .ss-zoom.
const wrap = (Component: React.ComponentType, opts: { zoom?: boolean } = {}) => {
  const Panel = (_props: IDockviewPanelProps) => (
    <div
      className={`h-full w-full overflow-hidden bg-[color:var(--ss-panel)]${
        opts.zoom === false ? "" : " ss-zoom"
      }`}
    >
      <Component />
    </div>
  );
  // Named so React DevTools shows the panel rather than a row of anonymous
  // wrappers (and so no display-name lint directive is needed).
  Panel.displayName = `Panel(${Component.displayName || Component.name || "Anonymous"})`;
  return Panel;
};

/** Warnings + errors waiting in Messages or the last Data Checks, as
 *  "level:count" ("" when there are none) so the tab re-renders only when the
 *  badge changes. */
function useAttention(panelId: string): string {
  return useProjectStore((s) => {
    const items =
      panelId === "messages" ? s.messages : panelId === "data-checks" ? (s.dataChecks ?? []) : [];
    let errors = 0;
    let warnings = 0;
    for (const m of items) {
      if (m.level === "error") errors++;
      else if (m.level === "warning") warnings++;
    }
    return errors + warnings ? `${errors ? "error" : "warning"}:${errors + warnings}` : "";
  });
}

// Grid group sizes from before a maximise: dockview's restore squeezes the last
// column it re-shows down to its minimum, so onReady puts them back whenever
// the maximise ends (double-click again, or focusPanel leaving it).
const sizesBeforeMaximise = new Map<string, { width: number; height: number }>();

function toggleMaximise({ api, containerApi }: IDockviewPanelHeaderProps) {
  if (api.group.api.location.type !== "grid") return;
  if (api.isMaximized()) {
    api.exitMaximized();
  } else {
    sizesBeforeMaximise.clear();
    for (const g of containerApi.groups) {
      if (g.api.location.type === "grid") sizesBeforeMaximise.set(g.id, { width: g.width, height: g.height });
    }
    api.maximize();
  }
}

// Panels whose tab shows a shorter title, so the side columns fit their tabs
// on a 1366-px screen; the tooltip and the accessible name keep the full one.
const FULL_TITLES: Record<string, string> = { cases: "Cases & Parameters" };

// Panel tabs, drawn with dockview's default-tab markup and styles. dockview's
// own tab element (the "tab" in each group's tab list) is named after the
// panel title; name it in full instead, with the problem count that Messages /
// Data Checks also show as a badge (drawn by CSS from data-badge) so problems
// show while the tray is collapsed. dockview names its tab element after the
// panel title when it creates it (a moved panel gets a new one) and again when
// the title changes; both are followed by a layout change, so the full name is
// set again after every layout change. The close X is a plain span, as in
// dockview 7, not the button dockview-react 8's default tab draws: a button
// inside the tab is a second Tab stop that screen readers do not announce
// (axe nested-interactive). Double-clicking a tab in the main grid maximises
// its group (e.g. the diagram) and double-clicking again restores.
function SsTab(props: IDockviewPanelHeaderProps) {
  const { api } = props;
  const [shown, setShown] = useState(api.title ?? "");
  useEffect(() => {
    // onReady may retitle the panel between the first render and this effect
    setShown(api.title ?? "");
    const d = api.onDidTitleChange((e) => setShown(e.title));
    return () => d.dispose();
  }, [api]);
  const title = FULL_TITLES[api.id] ?? shown;
  const [level, count] = useAttention(api.id).split(":");
  const label = count ? `${title} (${count} ${count === "1" ? "warning or error" : "warnings or errors"})` : title;
  const tab = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const name = () => tab.current?.closest(".dv-tab")?.setAttribute("aria-label", label);
    name();
    const d = props.containerApi.onDidLayoutChange(name);
    return () => d.dispose();
  }, [props.containerApi, label]);
  // preventDefault: dockview's tab then ignores the press or click (it would
  // activate the panel, or open or close the tray)
  const close = (e: React.MouseEvent) => {
    e.preventDefault();
    api.close();
  };
  return (
    <div
      ref={tab}
      className="dv-default-tab"
      title={api.group.api.location.type === "grid" ? `${label} — double-click to maximise` : label}
      data-badge={count || undefined}
      data-badge-level={level || undefined}
      onDoubleClick={() => toggleMaximise(props)}
      // a middle-click closes the panel, as on dockview's default tab
      onAuxClick={(e) => e.button === 1 && close(e)}
    >
      <span className="dv-default-tab-content">{shown}</span>
      <span className="dv-default-tab-action" onPointerDown={(e) => e.preventDefault()} onClick={close}>
        {/* dockview's own close icon, so the tab looks as it did */}
        <svg className="dv-svg" width="11" height="11" viewBox="0 0 28 28">
          <path d="M2.1 27.3L0 25.2L11.55 13.65L0 2.1L2.1 0L13.65 11.55L25.2 0L27.3 2.1L15.75 13.65L27.3 25.2L25.2 27.3L13.65 15.75L2.1 27.3Z" />
        </svg>
      </span>
    </div>
  );
}

// Open/close control on the bottom tray (clicking the active tab does the same).
function TrayToggle({ group }: IDockviewHeaderActionsProps) {
  const isEdge = group.api.location.type === "edge";
  const [collapsed, setCollapsed] = useState(() => isEdge && group.api.isCollapsed());
  useEffect(() => {
    const d = group.api.onDidCollapsedChange((e) => setCollapsed(e.isCollapsed));
    return () => d.dispose();
  }, [group]);
  if (!isEdge) return null;
  return (
    <button
      className="ss-toolbtn mx-1"
      title={collapsed ? "Open the panel" : "Collapse to tabs"}
      aria-label={collapsed ? "Open the panel" : "Collapse to tabs"}
      aria-expanded={!collapsed}
      onClick={() => (collapsed ? group.api.expand() : group.api.collapse())}
    >
      {collapsed ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
    </button>
  );
}

// Results has its own full-page workspace (Results ribbon tab), so it is not
// registered as a dock panel here.
const components = {
  components: wrap(ComponentsPanel),
  elements: wrap(ElementsPanel),
  topology: wrap(TopologyCanvas, { zoom: false }),
  monitors: wrap(MonitorsPanel),
  properties: wrap(PropertiesPanel),
  messages: wrap(MessagesPanel),
  "data-checks": wrap(DataChecksPanel),
  "layer-config": wrap(LayerConfigPanel),
  "data-bus": wrap(DataBusPanel),
  cases: wrap(CasePanel),
  "mini-chart": wrap(MiniChartPanel),
};

// bump when the panel set / default arrangement changes so stale saved layouts
// are discarded rather than restored into a broken state.
const LAYOUT_VERSION = 4;

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

// An open tray takes at most this share of the dock's height. dockview keeps
// an edge group's pixel size when the window shrinks (the grid above takes
// the change) and restores a saved layout's pixel size at any window size.
const TRAY_MAX_SHARE = 0.35;
const TRAY_MIN = 120;

/** Shrink an open tray that is taller than TRAY_MAX_SHARE of the dock (the
 *  grid plus the tray). */
function clampTray(api: DockviewApi) {
  const tray = api.getEdgeGroup("bottom");
  if (!tray || tray.isCollapsed()) return;
  const max = Math.max(TRAY_MIN, Math.round((api.height + tray.height) * TRAY_MAX_SHARE));
  if (tray.height <= max) return;
  tray.setSize({ height: max });
}

// Default arrangement, built so the diagram gets most of the window at every
// size: the library (left) and properties (right) columns take a share of the
// width with a floor, and the log, checks, layers, data bus and signal plot
// share a bottom tray that starts collapsed to its tab strip. dockview keeps
// the grid proportional when the window is resized.
function buildDefaultLayout(api: DockviewReadyEvent["api"]) {
  // the dock can still be unmeasured on the first frame; the window is a
  // close enough stand-in for proportioning the default layout
  const width = api.width || window.innerWidth;
  const height = api.height || window.innerHeight;
  const leftWidth = clamp(Math.round(width * 0.13), 200, 300);
  const rightWidth = clamp(Math.round(width * 0.15), 240, 360);

  const componentsPanel = api.addPanel({
    id: "components",
    component: "components",
    title: "Components",
    minimumWidth: 160,
  });
  api.addPanel({
    id: "elements",
    component: "elements",
    title: "Elements",
    position: { referencePanel: "components", direction: "within" },
    minimumWidth: 160,
  });
  api.addPanel({
    id: "topology",
    component: "topology",
    title: "Topology",
    position: { referencePanel: "components", direction: "right" },
    minimumWidth: 320,
  });
  api.addPanel({
    id: "monitors",
    component: "monitors",
    title: "Monitors",
    position: { referencePanel: "topology", direction: "within" },
    minimumWidth: 320,
  });
  const propertiesPanel = api.addPanel({
    id: "properties",
    component: "properties",
    title: "Properties",
    position: { referencePanel: "topology", direction: "right" },
    minimumWidth: 200,
  });
  api.addPanel({
    id: "cases",
    component: "cases",
    title: "Cases",
    position: { referencePanel: "properties", direction: "within" },
    minimumWidth: 200,
  });

  // Bottom tray. Clicking a tab opens it; the Signal Plot opens by itself on
  // the session's first run (see DockLayout). A failed restore can leave an
  // (emptied) tray behind: start from a fresh one.
  if (api.getEdgeGroup("bottom")) api.removeEdgeGroup("bottom");
  api.addEdgeGroup("bottom", {
    id: "tray",
    initialSize: clamp(Math.round(height * 0.3), 180, 360),
    minimumSize: TRAY_MIN,
    collapsed: true,
  });
  const tray = [
    ["messages", "Messages"],
    ["data-checks", "Data Checks"],
    ["layer-config", "Layer Configurations"],
    ["data-bus", "Data Bus Connections"],
    ["mini-chart", "Signal Plot"],
  ];
  for (const [id, title] of tray) {
    api.addPanel({ id, component: id, title, position: { referenceGroup: "tray" }, inactive: true });
  }

  componentsPanel.api.setSize({ width: leftWidth });
  propertiesPanel.api.setSize({ width: rightWidth });

  api.getPanel("components")?.api.setActive();
  api.getPanel("messages")?.api.setActive();
  api.getPanel("topology")?.api.setActive();
  // Properties is the default tab in the right group (Cases sits behind it)
  api.getPanel("properties")?.api.setActive();
}

/** Reset the workspace to the default panel arrangement. */
export function resetDockLayout() {
  try {
    window.localStorage.removeItem(LAYOUT_KEY);
  } catch {
    /* storage unavailable */
  }
  window.location.reload();
}

function onReady(event: DockviewReadyEvent) {
  const api = event.api;
  useUIStore.getState().setDockApi(api);

  // restore the user's saved arrangement; fall back to the default on any
  // failure (missing/renamed panels, corrupt data, or a version bump).
  let restored = false;
  try {
    const raw = window.localStorage.getItem(LAYOUT_KEY);
    if (raw) {
      const saved = JSON.parse(raw) as { version?: number; layout?: unknown };
      if (saved.version === LAYOUT_VERSION && saved.layout) {
        api.fromJSON(saved.layout as Parameters<typeof api.fromJSON>[0]);
        // a layout saved before GUI-34 titles the tab in full
        api.getPanel("cases")?.api.setTitle("Cases");
        restored = api.panels.length > 0;
      }
    }
  } catch {
    restored = false;
  }
  if (!restored) {
    try {
      api.clear();
    } catch {
      /* nothing to clear */
    }
    buildDefaultLayout(api);
  }

  // persist rearrangements (debounced) so the workspace survives reloads;
  // opening/collapsing the tray is not a layout change, so watch it too. A
  // maximised group is not saved: the sizes to go back to live in memory
  // only, so the last layout from before the maximise is kept.
  let timer: ReturnType<typeof setTimeout> | undefined;
  const scheduleSave = () => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      if (api.hasMaximizedGroup()) return;
      try {
        window.localStorage.setItem(
          LAYOUT_KEY,
          JSON.stringify({ version: LAYOUT_VERSION, layout: api.toJSON() }),
        );
      } catch {
        /* storage unavailable / quota */
      }
    }, 500);
  };
  api.onDidLayoutChange(scheduleSave);
  api.getEdgeGroup("bottom")?.onDidCollapsedChange((e) => {
    if (!e.isCollapsed) clampTray(api);
    scheduleSave();
  });

  api.onDidMaximizedGroupChange((e) => {
    if (e.isMaximized) return;
    for (const g of api.groups) {
      const size = sizesBeforeMaximise.get(g.id);
      if (size) g.api.setSize(size);
    }
    sizesBeforeMaximise.clear();
    scheduleSave();
  });
}

export function DockLayout() {
  // Open the Signal Plot when the session's first run starts, so the live
  // trace shows beside the diagram without a permanent strip under it.
  useEffect(() => {
    let shown = false;
    return useProjectStore.subscribe((s, prev) => {
      const head = s.runs[0];
      if (shown || !head || head.status !== "running" || head.id === prev.runs[0]?.id) return;
      shown = true;
      useUIStore.getState().focusPanel("mini-chart");
    });
  }, []);

  // keep an open tray within its share of the dock as the window is resized
  // (and after a saved layout is restored); a frame later, once dockview has
  // laid itself out for the new size
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    let frame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const api = useUIStore.getState().dockApi;
        if (api) clampTray(api);
      });
    });
    observer.observe(el);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  }, []);

  return (
    <div ref={ref} className="h-full w-full" role="region" aria-label="Model workspace panels">
      <DockviewReact
        components={components}
        defaultTabComponent={SsTab}
        rightHeaderActionsComponent={TrayToggle}
        onReady={onReady}
        theme={ssTheme}
      />
    </div>
  );
}
