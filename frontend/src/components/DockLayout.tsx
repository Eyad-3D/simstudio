import {
  DockviewDefaultTab,
  DockviewReact,
  themeLight,
  type DockviewReadyEvent,
  type DockviewTheme,
  type IDockviewPanelHeaderProps,
  type IDockviewPanelProps,
} from "dockview-react";
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

const ssTheme: DockviewTheme = {
  ...themeLight,
  name: "simstudio",
  className: `${themeLight.className} dockview-theme-ss`,
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

// Accessible panel tabs: the default dockview tab is a plain div with no role or
// discernible name. Wrap it so assistive tech announces each tab by its title.
function SsTab(props: IDockviewPanelHeaderProps) {
  const title = props.api.title ?? "";
  return <DockviewDefaultTab {...props} role="tab" aria-label={title} title={title} />;
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
const LAYOUT_KEY = "simstudio-layout-v1";
const LAYOUT_VERSION = 3;

function buildDefaultLayout(api: DockviewReadyEvent["api"]) {
  const componentsPanel = api.addPanel({
    id: "components",
    component: "components",
    title: "Components",
  });
  api.addPanel({
    id: "elements",
    component: "elements",
    title: "Elements",
    position: { referencePanel: "components", direction: "within" },
  });
  api.addPanel({
    id: "topology",
    component: "topology",
    title: "Topology",
    position: { referencePanel: "components", direction: "right" },
  });
  api.addPanel({
    id: "monitors",
    component: "monitors",
    title: "Monitors",
    position: { referencePanel: "topology", direction: "within" },
  });
  const propertiesPanel = api.addPanel({
    id: "properties",
    component: "properties",
    title: "Properties",
    position: { referencePanel: "topology", direction: "right" },
  });
  api.addPanel({
    id: "cases",
    component: "cases",
    title: "Cases & Parameters",
    position: { referencePanel: "properties", direction: "within" },
  });
  const messagesPanel = api.addPanel({
    id: "messages",
    component: "messages",
    title: "Messages",
    position: { direction: "below" },
  });
  api.addPanel({
    id: "data-checks",
    component: "data-checks",
    title: "Data Checks",
    position: { referencePanel: "messages", direction: "within" },
  });
  api.addPanel({
    id: "layer-config",
    component: "layer-config",
    title: "Layer Configurations",
    position: { referencePanel: "messages", direction: "within" },
  });
  api.addPanel({
    id: "data-bus",
    component: "data-bus",
    title: "Data Bus Connections",
    position: { referencePanel: "messages", direction: "within" },
  });
  // Dockable signal plot directly beneath the canvas (own group so it is visible
  // alongside the topology). Users can drag it elsewhere; it is a normal panel.
  const miniChartPanel = api.addPanel({
    id: "mini-chart",
    component: "mini-chart",
    title: "Signal Plot",
    position: { referencePanel: "topology", direction: "below" },
  });

  componentsPanel.api.setSize({ width: 265 });
  propertiesPanel.api.setSize({ width: 305 });
  messagesPanel.api.setSize({ height: 235 });
  miniChartPanel.api.setSize({ height: 200 });

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

  // persist rearrangements (debounced) so the workspace survives reloads
  let timer: ReturnType<typeof setTimeout> | undefined;
  api.onDidLayoutChange(() => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      try {
        window.localStorage.setItem(
          LAYOUT_KEY,
          JSON.stringify({ version: LAYOUT_VERSION, layout: api.toJSON() }),
        );
      } catch {
        /* storage unavailable / quota */
      }
    }, 500);
  });
}

export function DockLayout() {
  return (
    <div className="h-full w-full" role="region" aria-label="Model workspace panels">
      <DockviewReact
        components={components}
        defaultTabComponent={SsTab}
        onReady={onReady}
        theme={ssTheme}
      />
    </div>
  );
}
