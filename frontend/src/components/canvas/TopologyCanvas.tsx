import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  Background,
  BackgroundVariant,
  BaseEdge,
  getSmoothStepPath,
  MiniMap,
  ReactFlow,
  ReactFlowProvider,
  useReactFlow,
  useStore,
  useStoreApi,
  ViewportPortal,
  type Connection as RFConnection,
  type Edge,
  type EdgeChange,
  type EdgeProps,
  type IsValidConnection,
  type NodeChange,
  type Viewport,
} from "@xyflow/react";
import {
  BoxSelect,
  Cable,
  ChevronRight,
  ClipboardPaste,
  Copy,
  CopyPlus,
  Eye,
  EyeOff,
  Grid2x2,
  Magnet,
  Map,
  Maximize,
  MousePointerClick,
  PackagePlus,
  Pencil,
  Plus,
  Redo2,
  RotateCcw,
  Scissors,
  Settings2,
  Trash2,
  Undo2,
  Waypoints,
  Zap,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import {
  portsOf,
  systemBreadcrumb,
  useActiveRun,
  useActiveSystem,
  useProjectStore,
} from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { useDismiss } from "../useDismiss";
import { dropFmuFile } from "../../fmu";
import type { ComponentDef, PortDef, PortKind } from "../../types";
import { AddPartBox } from "./AddPartBox";
import { freeSpot, partSpot, wirePort, type Rect } from "./addPart";
import { ElementNode, KIND_COLOR, KIND_LINE, type ElementFlowNode } from "./ElementNode";
import { EnergyBars } from "./EnergyOverlay";
import { useReportsStore } from "../../store/reportsStore";
import { useStaleness } from "../panels/StaleBanner";
import {
  copySelection,
  cutSelection,
  deleteSelection,
  focusProperties,
  renamePart,
  renameSelection,
  selectAll,
  useCanvasSelection,
} from "../../store/canvasSelection";
import { canvasAction, diagramHasKeys, keyBelongsToTarget } from "./canvasKeys";
import { fitMenu } from "./menuPlace";
import { fitPadding, refitAfterResize } from "./view";
import { clearEnergyFigures } from "./energyRoom";
import { JUNCTIONS, asDrawn, partDef, physicalPorts, portInUse, routingInput, wiredPorts } from "./portPlaces";
import { fitEnds, pathMiddle, roundedPath, routeWires, type Pt } from "./route";

const nodeTypes = { element: ElementNode };

type RoutedFlowEdge = Edge<{ points?: Pt[] }, "routed">;

/** A wire along the route the router made for it (route.ts); while a part or
 *  port it joins is being moved, a plain stepped line until it is let go. */
function RoutedEdge(p: EdgeProps<RoutedFlowEdge>) {
  const pts = p.data?.points && fitEnds(p.data.points, { x: p.sourceX, y: p.sourceY }, { x: p.targetX, y: p.targetY });
  let path: string;
  let mid: Pt;
  if (pts) {
    path = roundedPath(pts);
    mid = pathMiddle(pts);
  } else {
    const [d, x, y] = getSmoothStepPath({ ...p, borderRadius: 6 });
    path = d;
    mid = { x, y };
  }
  return (
    <BaseEdge
      id={p.id}
      path={path}
      labelX={mid.x}
      labelY={mid.y}
      label={p.label}
      labelStyle={p.labelStyle}
      labelBgStyle={p.labelBgStyle}
      labelBgPadding={p.labelBgPadding}
      style={p.style}
      interactionWidth={p.interactionWidth}
    />
  );
}

const edgeTypes = { routed: RoutedEdge };

const GRID = 22; // background grid gap; snap uses the same pitch
const ALIGN_THRESH = 5; // flow-unit tolerance for alignment guides
const DEFAULT_W = 92;
const DEFAULT_H = 78;
// the room a new part keeps round the parts already there
const PLACE_GAP = 2 * GRID;

// Automatic fits (opening a project or subsystem, the dock settling) stop at
// 100 % so a small model is not blown up, and never go below a readable zoom:
// a model too big for that opens at its centre with the overview map shown.
// Every fit leaves room round the parts (view.ts fitPadding).
const AUTO_FIT = { maxZoom: 1, minZoom: 0.5 };

// a right-click on a part, or on one of its ports (`portId`)
type CtxMenu = { x: number; y: number; nodeId: string | null; portId?: string };

function MenuBtn({
  icon: Icon,
  label,
  onClick,
  kbd,
  disabled,
  danger,
  title,
  indent,
}: {
  icon: React.ComponentType<{ size?: number; className?: string }>;
  label: string;
  onClick: () => void;
  kbd?: string;
  disabled?: boolean;
  danger?: boolean;
  /** what it does, or why it cannot */
  title?: string;
  /** one of the item above's choices */
  indent?: boolean;
}) {
  // one that cannot, with its reason, stays under the pointer so the reason shows
  const soft = !!disabled && !!title;
  return (
    <button
      className={`flex w-full items-center gap-2 py-1 pr-2.5 text-left hover:bg-[color:var(--ss-hover)] disabled:opacity-40 disabled:hover:bg-transparent ${
        indent ? "pl-6" : "pl-2.5"
      } ${danger ? "text-[color:var(--ss-err)]" : ""} ${soft ? "cursor-default opacity-40 hover:bg-transparent" : ""}`}
      disabled={disabled && !soft}
      aria-disabled={soft || undefined}
      title={title}
      onClick={soft ? undefined : onClick}
    >
      <Icon size={13} className="shrink-0" />
      <span className="flex-1">{label}</span>
      {kbd && <span className="text-meta text-[color:var(--ss-text-dim)]">{kbd}</span>}
    </button>
  );
}

/** The zoom as a percentage, with Fit and fixed zoom levels to pick. Its own
 *  component, so a zoom re-renders only this. */
function ZoomMenu({ onFit }: { onFit: () => void }) {
  const zoom = useStore((s) => Math.round(s.transform[2] * 100));
  const { zoomTo } = useReactFlow();
  return (
    <select
      className="ss-input w-[64px] py-[3px] text-meta"
      aria-label="Zoom"
      title="Zoom"
      value="now"
      onChange={(e) => (e.target.value === "fit" ? onFit() : void zoomTo(Number(e.target.value), { duration: 200 }))}
      // Its value is always the readout, so an arrow key on the closed list
      // would pick Fit every time: open the list instead.
      onKeyDown={(e) => {
        if ((e.key === "ArrowDown" || e.key === "ArrowUp") && !e.altKey) {
          e.preventDefault();
          e.currentTarget.showPicker();
        }
      }}
    >
      <option value="now" hidden>
        {zoom}%
      </option>
      <option value="fit">Fit</option>
      <option value="0.5">50%</option>
      <option value="1">100%</option>
      <option value="2">200%</option>
    </select>
  );
}

function TopologyCanvasInner() {
  const project = useProjectStore((s) => s.project);
  const libraryById = useProjectStore((s) => s.libraryById);
  const activeSystemId = useProjectStore((s) => s.activeSystemId);
  const system = useActiveSystem();
  const store = useProjectStore;
  // the run shown in Results: wires added since it (UX-41), and its energy (RES-22)
  const staleWires = useStaleness(useActiveRun()).wireIds;
  const showEnergy = useReportsStore((s) => s.showEnergy);
  const visibleKinds = useUIStore((s) => s.visibleKinds);
  const {
    fitView,
    zoomIn,
    zoomOut,
    screenToFlowPosition,
    getViewport,
    setViewport,
    setCenter,
    getNodes,
    getNodesBounds,
    getInternalNode,
  } = useReactFlow();
  const rfStore = useStoreApi();

  // the selection lives in its own store, shared with the ribbon (UX-19)
  const selectedNodes = useCanvasSelection((s) => s.nodes);
  const selectedEdges = useCanvasSelection((s) => s.edges);
  const { setNodes: setSelectedNodes, setEdges: setSelectedEdges } = useCanvasSelection.getState();
  const [showMiniMap, setShowMiniMap] = useState(false);
  const [showGrid, setShowGrid] = useState(true);
  const [snap, setSnap] = useState(false);
  const [menu, setMenu] = useState<CtxMenu | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const menuEl = menu?.nodeId ? system?.elements.find((e) => e.id === menu.nodeId) : undefined;
  // its ports as the project has them (a node's own terminals), its hidden ones
  const menuLib = menuEl && libraryById[menuEl.componentDefId];
  const menuPorts = menuEl && menuLib ? physicalPorts(partDef(menuEl, menuLib)) : [];
  const menuHidden = menuPorts.filter((p) => menuEl?.hiddenPorts?.includes(p.id));
  const menuJunction = !!menuLib && !!JUNCTIONS[menuLib.id];
  const menuPort = menu?.portId ? menuPorts.find((p) => p.id === menu.portId) : undefined;
  const menuPortUsed = !!menuPort && !!project && !!menuEl && portInUse(project, menuEl.id, menuPort.id);
  // a Monitor or Script without ports yet can still get signals
  const menuSignals =
    !!menuEl &&
    (!!libraryById[menuEl.componentDefId]?.allowDynamicPorts ||
      portsOf(menuEl, libraryById).some((p) => p.kind === "signal"));
  const [guides, setGuides] = useState<{ x: number[]; y: number[] } | null>(null);
  // node sizes React Flow measured, kept on the controlled nodes (as
  // applyNodeChanges would) so the minimap can draw them. Keyed by element id
  // and component type: React Flow keeps a measured node's handle positions,
  // so an id reused by another project for a different part must be measured
  // afresh.
  const [measured, setMeasured] = useState<Record<string, { width: number; height: number }>>({});
  const theme = useUIStore((s) => s.theme);

  const placingId = useUIStore((s) => s.placingComponentId);
  const placingDef = placingId ? libraryById[placingId] : undefined;
  const clipboard = useProjectStore((s) => s.clipboard);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const wrapperRef = useRef<HTMLDivElement | null>(null);
  // the floating project name and toolbar over the diagram's top edge
  const overlayRef = useRef<HTMLDivElement | null>(null);
  const hovered = useRef(false);
  const pointer = useRef<{ x: number; y: number } | null>(null);
  // whether the last click was on the diagram (its toolbar and menu count)
  const clickedHere = useRef(false);
  // the pointer on the screen, for Shift+A
  const pointerAt = useRef<{ x: number; y: number } | null>(null);
  const library = useProjectStore((s) => s.library);
  // the add-a-part box: where it is (in the diagram, and on it), and the
  // port of a wire let go there
  const [adding, setAdding] = useState<{
    x: number;
    y: number;
    at: { x: number; y: number };
    from?: { elementId: string; port: PortDef };
  } | null>(null);

  // (a part selected elsewhere, in Properties, Elements or Problems, becomes
  // the selection in canvasSelection.ts)
  // freshly pasted/duplicated elements become the canvas selection
  const selectNew = useCallback(
    (ids: string[]) => {
      if (ids.length) setSelectedNodes(new Set(ids));
    },
    [setSelectedNodes],
  );
  // pan and zoom to parts (the "." key and the Problems list)
  const frame = useCallback(
    (ids: string[]) => void fitView({ nodes: ids.map((id) => ({ id })), padding: 0.3, maxZoom: 1, duration: 200 }),
    [fitView],
  );
  // parts to frame once the sub-system they are in is shown
  const pendingReveal = useRef<string[] | null>(null);
  // select parts and frame them, opening their sub-system (the parts of the
  // first one's sub-system: a problem's parts are in one)
  const reveal = useCallback(
    (ids: string[]) => {
      const st = store.getState();
      const sys = st.project?.systems.find((sy) => sy.elements.some((e) => e.id === ids[0]));
      if (!sys) return;
      const here = ids.filter((id) => sys.elements.some((e) => e.id === id));
      if (sys.id !== st.activeSystemId) {
        pendingReveal.current = here;
        st.setActiveSystem(sys.id);
      } else {
        frame(here);
      }
      st.select(here[here.length - 1]);
      setSelectedNodes(new Set(here));
    },
    [frame, store, setSelectedNodes],
  );
  useEffect(() => {
    const ui = useUIStore.getState();
    ui.setRevealElements(reveal);
    return () => ui.setRevealElements(null);
  }, [reveal]);

  // the view the last automatic fit left, while the user has not moved it
  const autoView = useRef("");
  const autoFit = useCallback(
    (duration = 0, showMap = true) => {
      const { project: p, activeSystemId: sysId } = store.getState();
      const sys = p?.systems.find((sy) => sy.id === sysId);
      if (!sys || sys.elements.length === 0) {
        // Nothing to fit. React Flow would keep a fit request queued until the
        // first part appears and then zoom that one part to the maximum.
        rfStore.setState({ fitViewQueued: false });
        void setViewport({ x: 0, y: 0, zoom: 1 }, { duration });
        return;
      }
      void fitView({ ...AUTO_FIT, padding: fitPadding(overlayRef.current?.offsetHeight ?? 0), duration }).then(() => {
        autoView.current = rfStore.getState().transform.join();
        const { width, height } = rfStore.getState();
        const bounds = getNodesBounds(getNodes());
        const { zoom } = getViewport();
        if (showMap && (bounds.width * zoom > width || bounds.height * zoom > height)) setShowMiniMap(true);
      });
    },
    [fitView, getNodes, getNodesBounds, getViewport, rfStore, setViewport, store],
  );

  // Count project loads (New, Open, Import). A load replaces the project
  // together with a fresh, empty undo history, while edits, undo and redo
  // always leave one. The project id alone misses re-opening the open project
  // (to revert it) and importing a file with the same id; both examples also
  // share the root system id "sys-root".
  const [loads, setLoads] = useState(0);
  useEffect(
    () =>
      store.subscribe((s, prev) => {
        if (s.project !== prev.project && s.past !== prev.past && s.past.length === 0 && s.future.length === 0)
          setLoads((n) => n + 1);
      }),
    [store],
  );

  // Fit when a project is loaded or a subsystem entered. Returning to a
  // subsystem already visited since the load restores its view; one entered
  // to show a problem frames its parts. An armed library part belongs to the
  // diagram it was armed on.
  const shown = useRef<{ loads?: number; systemId?: string | null }>({});
  const views = useRef<Record<string, Viewport>>({});
  useEffect(() => {
    const prev = shown.current;
    shown.current = { loads, systemId: activeSystemId };
    if (prev.loads !== loads) views.current = {};
    else if (prev.systemId && prev.systemId !== activeSystemId) views.current[prev.systemId] = getViewport();
    useUIStore.getState().setPlacingComponent(null);
    const saved = activeSystemId ? views.current[activeSystemId] : undefined;
    const t = setTimeout(() => {
      const ids = pendingReveal.current;
      pendingReveal.current = null;
      if (ids) frame(ids);
      else if (saved) void setViewport(saved, { duration: 200 });
      else autoFit(200);
    }, 120);
    return () => clearTimeout(t);
  }, [loads, activeSystemId, autoFit, frame, getViewport, setViewport]);

  // When the diagram changes size (the bottom tray opens or closes, the
  // window is resized), re-fit once the size settles, or not: view.ts
  // refitAfterResize (the tray the first run opens keeps the zoom).
  useEffect(() => {
    let before: { width: number; height: number } | null = null;
    let settle: ReturnType<typeof setTimeout> | undefined;
    const unsubscribe = rfStore.subscribe((s, prev) => {
      if (s.width === prev.width && s.height === prev.height) return;
      if (!before && prev.width > 0 && prev.height > 0) before = { width: prev.width, height: prev.height };
      clearTimeout(settle);
      settle = setTimeout(() => {
        const was = before;
        before = null;
        const { width, height, transform } = rfStore.getState();
        const nodes = getNodes();
        if (!was || width === 0 || height === 0 || nodes.length === 0) return;
        const [x, y, zoom] = transform;
        const b = getNodesBounds(nodes);
        const inView = (w: number, h: number) =>
          b.x * zoom + x >= 0 &&
          b.y * zoom + y >= 0 &&
          (b.x + b.width) * zoom + x <= w &&
          (b.y + b.height) * zoom + y <= h;
        // the overview map is shown again only when this change cut the model off
        const cut = inView(was.width, was.height) && !inView(width, height);
        const held = Date.now() < useUIStore.getState().diagramViewHeldUntil;
        if (refitAfterResize(cut, transform.join() === autoView.current, held)) autoFit(200, cut);
      }, 150);
    });
    return () => {
      unsubscribe();
      clearTimeout(settle);
    };
  }, [rfStore, getNodes, getNodesBounds, autoFit]);

  // the zoom as a CSS variable for the part names (.ss-node-label), set on
  // the DOM so a zoom does not re-render the diagram
  useEffect(() => {
    const set = (zoom: number) => wrapperRef.current?.style.setProperty("--ss-zoom", String(zoom));
    set(rfStore.getState().transform[2]);
    return rfStore.subscribe((s, prev) => {
      if (s.transform[2] !== prev.transform[2]) set(s.transform[2]);
    });
  }, [rfStore]);

  // With Energy on, the figures that have no room under the names give way:
  // worked out again after each change of the drawing and of the view
  const energyRun = useActiveRun()?.id;
  useEffect(() => {
    if (!showEnergy) return;
    let frame = 0;
    const clear = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => wrapperRef.current && clearEnergyFigures(wrapperRef.current));
    };
    clear();
    const unsubscribe = rfStore.subscribe((s, prev) => {
      if (s.transform !== prev.transform || s.nodeLookup !== prev.nodeLookup) clear();
    });
    return () => {
      unsubscribe();
      cancelAnimationFrame(frame);
    };
  }, [showEnergy, energyRun, system, rfStore]);

  // re-fit while the dock layout settles after initial mount (panel widths are
  // applied a few frames after the flow instance measures itself)
  useEffect(() => {
    const timers = [400, 900].map((ms) => setTimeout(() => autoFit(), ms));
    return () => timers.forEach(clearTimeout);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // What a new part must not cover, in the diagram's units: every part with
  // room round it for its name, and the floating project name and toolbar.
  const takenSpots = useCallback((): Rect[] => {
    const st = store.getState();
    const sys = st.project?.systems.find((sy) => sy.id === st.activeSystemId);
    const gap = PLACE_GAP;
    const parts = (sys?.elements ?? []).map((el) => {
      const m = getInternalNode(el.id)?.measured;
      return {
        x: el.position.x - gap / 2,
        y: el.position.y - gap / 2,
        w: (m?.width ?? el.size?.width ?? DEFAULT_W) + gap,
        h: (m?.height ?? el.size?.height ?? DEFAULT_H) + gap,
      };
    });
    const floating = [...(overlayRef.current?.children ?? [])].flatMap((c) => {
      const r = c.getBoundingClientRect();
      if (r.width === 0 || r.height === 0) return [];
      const a = screenToFlowPosition({ x: r.left, y: r.top });
      const b = screenToFlowPosition({ x: r.right, y: r.bottom });
      return [{ x: a.x, y: a.y, w: b.x - a.x, h: b.y - a.y }];
    });
    return [...parts, ...floating];
  }, [getInternalNode, screenToFlowPosition, store]);

  // Parts added from the library by keyboard or double-click go in the middle
  // of the visible diagram, on the nearest free spot, so repeated inserts do
  // not pile up; the view pans if that spot is off screen.
  const insertAtCentre = useCallback(
    (defId: string): string | null => {
      const st = store.getState();
      const sys = st.project?.systems.find((sy) => sy.id === st.activeSystemId);
      const rect = wrapperRef.current?.getBoundingClientRect();
      if (!sys || !rect || rect.width === 0) return null;
      const centre = screenToFlowPosition({ x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 });
      const snap = (v: number) => Math.round(v / GRID) * GRID;
      const pos = freeSpot(
        { x: snap(centre.x - DEFAULT_W / 2), y: snap(centre.y - DEFAULT_H / 2) },
        { w: DEFAULT_W, h: DEFAULT_H },
        takenSpots(),
        PLACE_GAP,
        snap,
      );
      st.addElement(defId, pos);
      const topLeft = screenToFlowPosition({ x: rect.left, y: rect.top });
      const bottomRight = screenToFlowPosition({ x: rect.right, y: rect.bottom });
      if (pos.x < topLeft.x || pos.y < topLeft.y || pos.x + DEFAULT_W > bottomRight.x || pos.y + DEFAULT_H > bottomRight.y) {
        void setCenter(pos.x + DEFAULT_W / 2, pos.y + DEFAULT_H / 2, { zoom: getViewport().zoom, duration: 200 });
      }
      const next = store.getState();
      return next.project?.systems.flatMap((sy) => sy.elements).find((el) => el.id === next.selectedElementId)?.label ?? null;
    },
    [getViewport, screenToFlowPosition, setCenter, store, takenSpots],
  );
  useEffect(() => {
    const ui = useUIStore.getState();
    ui.setInsertComponent(insertAtCentre);
    return () => ui.setInsertComponent(null);
  }, [insertAtCentre]);

  // click-to-place: Esc cancels an armed library part
  useEffect(() => {
    if (!placingId) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && useUIStore.getState().setPlacingComponent(null);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [placingId]);

  /** Fit everything on request (toolbar / context menu); a no-op when empty. */
  const fitAll = useCallback(() => {
    if (getNodes().length > 0)
      void fitView({ padding: fitPadding(overlayRef.current?.offsetHeight ?? 0), duration: 200 });
  }, [fitView, getNodes]);

  const nodes: ElementFlowNode[] = useMemo(() => {
    if (!system) return [];
    const wired = wiredPorts(system);
    return system.elements
      .filter((el) => libraryById[el.componentDefId])
      .map((el) => ({
        id: el.id,
        type: "element" as const,
        position: el.position,
        data: { element: asDrawn(el, wired), def: partDef(el, libraryById[el.componentDefId]) },
        selected: selectedNodes.has(el.id),
        measured: measured[`${el.id}|${el.componentDefId}`],
      }));
  }, [system, libraryById, selectedNodes, measured]);

  // The wires' routes, made again when the diagram changes but not while a
  // part is being dragged: then the routes made before it stay, and the
  // dragged part's wires are drawn as plain stepped lines until it is let go.
  const [dragging, setDragging] = useState(false);
  const [routedSystem, setRoutedSystem] = useState(system);
  if (!dragging && routedSystem !== system) setRoutedSystem(system);
  const routes = useMemo(() => {
    if (!routedSystem) return {};
    const shown = (k: PortKind) => k !== "signal" && visibleKinds[k === "mechanical" ? "mechanical" : "electrical"];
    const { parts, wires } = routingInput(routedSystem, libraryById, undefined, shown);
    return routeWires(parts, wires);
  }, [routedSystem, libraryById, visibleKinds]);

  const edges: RoutedFlowEdge[] = useMemo(() => {
    if (!system || !project) return [];
    const portKind = (elId: string, portId: string): PortKind => {
      const el = system.elements.find((e) => e.id === elId);
      return (el && portsOf(el, libraryById).find((p) => p.id === portId)?.kind) ?? "power";
    };
    // signal / data-bus wiring is not drawn on the canvas — see the
    // Data Bus Connections panel
    const out: RoutedFlowEdge[] = [];
    for (const c of system.connections) {
      const kind = portKind(c.sourceElementId, c.sourcePortId);
      if (kind === "signal") continue;
      const filterKey = kind === "mechanical" ? "mechanical" : "electrical";
      if (!visibleKinds[filterKey]) continue;
      out.push({
        id: c.id,
        source: c.sourceElementId,
        sourceHandle: c.sourcePortId,
        target: c.targetElementId,
        targetHandle: c.targetPortId,
        type: "routed",
        data: { points: routes[c.id] },
        // its kind's colour and line; a selected wire a pixel thicker
        style: {
          stroke: KIND_COLOR[kind],
          ...KIND_LINE[kind],
          ...(selectedEdges.has(c.id) ? { strokeWidth: KIND_LINE[kind].strokeWidth + 1 } : {}),
        },
        selected: selectedEdges.has(c.id),
        // a wire added since the run shown in Results (UX-41)
        ...(staleWires.has(c.id)
          ? {
              label: "●",
              ariaLabel: "Wire added since the results shown",
              labelStyle: { fill: "var(--ss-accent)", fontSize: 12 },
              labelBgStyle: { fill: "var(--ss-panel)" },
              labelBgPadding: [2, 0] as [number, number],
            }
          : {}),
      });
    }
    return out;
  }, [system, project, libraryById, visibleKinds, selectedEdges, staleWires, routes]);

  // Signal / data-bus links as a dashed overlay (render-only). These are stored
  // globally in project.dataBusConnections, not as canvas edges; we draw a
  // border-to-border dashed line between the two node boxes so control loops
  // are visible. Toggled via the "signal" layer in Layer Configurations.
  const signalEdges = useMemo(() => {
    if (!system || !project || !visibleKinds.signal) return [];
    // NB: `Map` is shadowed by the lucide-react Map icon in this file — use a record.
    const inSys: Record<string, (typeof system.elements)[number]> = {};
    for (const e of system.elements) inSys[e.id] = e;
    const box = (el: (typeof system.elements)[number]) => {
      const w = el.size?.width ?? DEFAULT_W;
      const h = el.size?.height ?? 64;
      return { cx: el.position.x + w / 2, cy: el.position.y + h / 2, hw: w / 2, hh: h / 2 };
    };
    const out: { id: string; x1: number; y1: number; x2: number; y2: number }[] = [];
    for (const d of project.dataBusConnections) {
      const a = inSys[d.element1Id];
      const b = inSys[d.element2Id];
      if (!a || !b) continue;
      const ba = box(a);
      const bb = box(b);
      const dx = bb.cx - ba.cx;
      const dy = bb.cy - ba.cy;
      if (dx === 0 && dy === 0) continue;
      const adx = Math.abs(dx) || 1e-6;
      const ady = Math.abs(dy) || 1e-6;
      const t0 = Math.min(ba.hw / adx, ba.hh / ady); // exit source box
      const t1 = Math.min(bb.hw / adx, bb.hh / ady); // enter target box
      out.push({
        id: d.id,
        x1: ba.cx + dx * t0,
        y1: ba.cy + dy * t0,
        x2: bb.cx - dx * t1,
        y2: bb.cy - dy * t1,
      });
    }
    return out;
  }, [system, project, visibleKinds]);

  const onNodesChange = useCallback(
    (changes: NodeChange<ElementFlowNode>[]) => {
      const sel = new Set(useCanvasSelection.getState().nodes);
      let selChanged = false;
      const sizes: Record<string, { width: number; height: number }> = {};
      for (const ch of changes) {
        if (ch.type === "position" && ch.position) {
          store.getState().moveElement(ch.id, ch.position);
        } else if (ch.type === "dimensions" && ch.dimensions) {
          // NodeResizer-driven resize (auto-measure changes have no `resizing` flag)
          if ("resizing" in ch) store.getState().resizeElement(ch.id, ch.dimensions);
          sizes[ch.id] = ch.dimensions;
        } else if (ch.type === "select") {
          selChanged = true;
          if (ch.selected) sel.add(ch.id);
          else sel.delete(ch.id);
        }
      }
      if (selChanged) {
        setSelectedNodes(sel);
        const st = store.getState();
        const single = sel.size >= 1 ? [...sel][sel.size - 1] : null;
        if (st.selectedElementId !== single) st.select(single);
      }
      if (Object.keys(sizes).length > 0) {
        const defOf: Record<string, string> = {};
        for (const sy of store.getState().project?.systems ?? [])
          for (const el of sy.elements) defOf[el.id] = el.componentDefId;
        setMeasured((prev) => {
          const next = { ...prev };
          for (const [id, size] of Object.entries(sizes)) next[`${id}|${defOf[id]}`] = size;
          return next;
        });
      }
    },
    [store, setSelectedNodes],
  );

  const onEdgesChange = useCallback(
    (changes: EdgeChange<Edge>[]) => {
      if (!changes.some((ch) => ch.type === "select")) return;
      setSelectedEdges((prev) => {
        const sel = new Set(prev);
        for (const ch of changes) {
          if (ch.type === "select") {
            if (ch.selected) sel.add(ch.id);
            else sel.delete(ch.id);
          }
        }
        return sel;
      });
    },
    [setSelectedEdges],
  );

  const portOf = useCallback(
    (elId: string | null, portId: string | null | undefined) => {
      if (!elId || !portId || !system) return null;
      const el = system.elements.find((e) => e.id === elId);
      return (el && portsOf(el, libraryById).find((p) => p.id === portId)) ?? null;
    },
    [system, libraryById],
  );

  const isValidConnection: IsValidConnection = useCallback(
    (conn) => {
      if (!conn.source || !conn.target || conn.source === conn.target) return false;
      const a = portOf(conn.source, conn.sourceHandle);
      const b = portOf(conn.target, conn.targetHandle);
      if (!a || !b) return false;
      // signals are wired in the Data Bus Connections panel, not on canvas
      if (a.kind === "signal" || b.kind === "signal") return false;
      return a.kind === b.kind;
    },
    [portOf],
  );

  const onConnect = useCallback(
    (conn: RFConnection) => {
      if (!conn.source || !conn.target || !conn.sourceHandle || !conn.targetHandle) return;
      store
        .getState()
        .addConnection(conn.source, conn.sourceHandle, conn.target, conn.targetHandle);
    },
    [store],
  );

  // the add-a-part box at a spot on the screen (a double-click on empty
  // space, Shift+A, or a wire let go there)
  const openAdd = useCallback(
    (clientX: number, clientY: number, from?: { elementId: string; port: PortDef }) => {
      const rect = wrapperRef.current?.getBoundingClientRect();
      if (!rect) return;
      setMenu(null);
      setAdding({ x: clientX - rect.left, y: clientY - rect.top, at: screenToFlowPosition({ x: clientX, y: clientY }), from });
    },
    [screenToFlowPosition],
  );
  const closeAdd = useCallback(() => setAdding(null), []);
  // the part chosen goes where the box opened (on the grid when Snap is on),
  // or the nearest free spot when a part or the floating name and toolbar
  // are there; a dragged wire goes to its fitting port, in one undo step
  const placeChosen = (def: ComponentDef) => {
    if (!adding) return;
    const to = adding.from ? wirePort(def, adding.from.port) : null;
    const grid = (v: number) => (snap ? Math.round(v / GRID) * GRID : v);
    store
      .getState()
      .addElement(
        def.id,
        freeSpot(partSpot(adding.at, snap ? GRID : null), { w: DEFAULT_W, h: DEFAULT_H }, takenSpots(), PLACE_GAP, grid),
        adding.from && to ? { elementId: adding.from.elementId, portId: adding.from.port.id, toPortId: to } : undefined,
      );
    setAdding(null);
    const id = store.getState().selectedElementId;
    if (id) selectNew([id]);
  };

  // reconnect an existing edge to a different port (same kind only)
  const onReconnect = useCallback(
    (oldEdge: Edge, conn: RFConnection) => {
      if (!conn.source || !conn.target || !conn.sourceHandle || !conn.targetHandle) return;
      const a = portOf(conn.source, conn.sourceHandle);
      const b = portOf(conn.target, conn.targetHandle);
      if (!a || !b || a.kind === "signal" || a.kind !== b.kind) return; // invalid → keep old edge
      store
        .getState()
        .addConnection(conn.source, conn.sourceHandle, conn.target, conn.targetHandle, oldEdge.id);
    },
    [portOf, store],
  );

  // --- alignment guides while dragging (visual; snapping stays on the grid) ---
  const onNodeDrag = useCallback(
    (_: unknown, node: ElementFlowNode) => {
      if (!system) return;
      const w = node.measured?.width ?? DEFAULT_W;
      const h = node.measured?.height ?? DEFAULT_H;
      const aXs = [node.position.x, node.position.x + w / 2, node.position.x + w];
      const aYs = [node.position.y, node.position.y + h / 2, node.position.y + h];
      const gx = new Set<number>();
      const gy = new Set<number>();
      for (const el of system.elements) {
        if (el.id === node.id) continue;
        const ew = el.size?.width ?? DEFAULT_W;
        const eh = el.size?.height ?? DEFAULT_H;
        const bXs = [el.position.x, el.position.x + ew / 2, el.position.x + ew];
        const bYs = [el.position.y, el.position.y + eh / 2, el.position.y + eh];
        for (const a of aXs) for (const b of bXs) if (Math.abs(a - b) <= ALIGN_THRESH) gx.add(b);
        for (const a of aYs) for (const b of bYs) if (Math.abs(a - b) <= ALIGN_THRESH) gy.add(b);
      }
      setGuides(gx.size || gy.size ? { x: [...gx], y: [...gy] } : null);
    },
    [system],
  );
  const onNodeDragStop = useCallback(() => {
    setGuides(null);
    setDragging(false);
  }, []);

  // context-menu helpers -----------------------------------------------------
  // opened at the pointer; once drawn, fitted inside the diagram (menuPlace.ts)
  const openMenuAt = (clientX: number, clientY: number, nodeId: string | null, portId?: string) => {
    const rect = wrapperRef.current?.getBoundingClientRect();
    setMenu({ x: clientX - (rect?.left ?? 0), y: clientY - (rect?.top ?? 0), nodeId, portId });
  };
  const closeMenu = () => setMenu(null);
  useLayoutEffect(() => {
    if (menu && menuRef.current && wrapperRef.current) fitMenu(menuRef.current, wrapperRef.current, menu.x, menu.y);
  }, [menu]);

  // the diagram has the keyboard while the focus is in it, or nowhere after
  // a click on it (or with the pointer over it): see canvasKeys.ts (UX-19)
  useEffect(() => {
    const onDown = (e: PointerEvent) => {
      clickedHere.current = !!rootRef.current?.contains(e.target as Node);
    };
    document.addEventListener("pointerdown", onDown, true);
    return () => document.removeEventListener("pointerdown", onDown, true);
  }, []);

  // select all, copy, cut, paste, duplicate, delete, rename and Properties
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const action = canvasAction(e);
      if (!action) return;
      const ui = useUIStore.getState();
      if (ui.ribbonTab === "results" || ui.ribbonTab === "start" || ui.paramDialogId || ui.dialog) return;
      if (keyBelongsToTarget(e.target instanceof Element ? e.target : null, action)) return;
      const focused = document.activeElement;
      if (!diagramHasKeys(rootRef.current, focused, document.body, clickedHere.current, hovered.current)) return;
      const st = store.getState();
      let done = false;
      switch (action) {
        case "selectAll":
          done = selectAll();
          // the page's own select-all would highlight every text on it
          e.preventDefault();
          break;
        case "copy":
          done = copySelection();
          break;
        case "cut":
          done = cutSelection();
          break;
        case "paste":
          if (st.clipboard) {
            selectNew(st.pasteClipboard(hovered.current ? (pointer.current ?? undefined) : undefined));
            done = true;
          }
          break;
        case "duplicate":
          if (selectedNodes.size > 0) {
            selectNew(st.duplicateElements([...selectedNodes]));
            done = true;
          }
          // the browser's own Ctrl+D adds a bookmark
          e.preventDefault();
          break;
        case "delete":
          done = deleteSelection();
          break;
        case "rename":
          done = renameSelection();
          break;
        case "properties":
          done = focusProperties();
          break;
        case "addPart": {
          // at the pointer when it is over the diagram, else in its middle
          const rect = wrapperRef.current?.getBoundingClientRect();
          const p = hovered.current && pointerAt.current ? pointerAt.current : rect && { x: rect.left + rect.width / 2, y: rect.top + rect.height / 3 };
          if (p && !placingId) {
            openAdd(p.x, p.y);
            done = true;
          }
          break;
        }
      }
      if (done) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selectedNodes, store, selectNew, openAdd, placingId]);

  // "." frames the selection (the whole model when nothing is selected)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "." || e.ctrlKey || e.metaKey || e.altKey) return;
      const t = e.target as HTMLElement;
      if (["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName) || t.isContentEditable) return;
      const ui = useUIStore.getState();
      if (ui.ribbonTab === "results" || ui.ribbonTab === "start" || ui.paramDialogId || ui.dialog) return;
      e.preventDefault();
      if (selectedNodes.size > 0) {
        frame([...selectedNodes]);
      } else {
        autoFit(200);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selectedNodes, frame, autoFit]);

  // close the context menu on Escape / outside interactions
  useDismiss(menu !== null, closeMenu, menuRef);

  const breadcrumb = project && activeSystemId ? systemBreadcrumb(project, activeSystemId) : [];
  const past = useProjectStore((s) => s.past.length);
  const future = useProjectStore((s) => s.future.length);

  return (
    <div ref={rootRef} className="relative flex h-full flex-col">
      {/* the toolbar floats over the diagram's top edge as two pills, so the
          diagram gets the panel's full height; in a narrow panel the tools
          wrap onto a second row */}
      <div
        ref={overlayRef}
        className="pointer-events-none absolute inset-x-0 top-0 z-20 flex items-start justify-between gap-2 p-1.5"
      >
        {/* the name of the diagram shown lets clicks through to the parts
            under it; only the names of the diagrams above it (inside a
            sub-system) are buttons that go back up */}
        <div
          data-diagram-name
          className="flex min-w-0 items-center gap-0.5 rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-1 py-0.5 text-body shadow-sm empty:hidden"
        >
          {breadcrumb.map((sys, i) => (
            <span key={sys.id} className="flex min-w-0 items-center gap-0.5">
              {i > 0 && <ChevronRight size={12} className="shrink-0 text-[color:var(--ss-text-dim)]" />}
              {i === breadcrumb.length - 1 ? (
                <span className="truncate px-1 py-0.5 font-semibold text-[color:var(--ss-accent)]">{sys.name}</span>
              ) : (
                <button
                  className="pointer-events-auto truncate rounded px-1 py-0.5 text-[color:var(--ss-text-dim)] hover:bg-[color:var(--ss-hover)]"
                  onClick={() => store.getState().setActiveSystem(sys.id)}
                >
                  {sys.name}
                </button>
              )}
            </span>
          ))}
        </div>
        <div className="pointer-events-auto flex flex-wrap items-center justify-end gap-0.5 rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-1 py-0.5 shadow-sm">
          <button className="ss-toolbtn" title="Zoom in" onClick={() => void zoomIn()}>
            <ZoomIn size={14} />
          </button>
          <button className="ss-toolbtn" title="Zoom out" onClick={() => void zoomOut()}>
            <ZoomOut size={14} />
          </button>
          <ZoomMenu onFit={fitAll} />
          <button
            className="ss-toolbtn"
            title="Fit to screen (press . to frame the selection)"
            onClick={fitAll}
          >
            <Maximize size={14} />
          </button>
          <div className="mx-1 h-4 w-px bg-[color:var(--ss-border)]" />
          <button
            className="ss-toolbtn"
            title="Undo (Ctrl+Z)"
            disabled={past === 0}
            onClick={() => store.getState().undo()}
          >
            <Undo2 size={14} />
          </button>
          <button
            className="ss-toolbtn"
            title="Redo (Ctrl+Y)"
            disabled={future === 0}
            onClick={() => store.getState().redo()}
          >
            <Redo2 size={14} />
          </button>
          <div className="mx-1 h-4 w-px bg-[color:var(--ss-border)]" />
          <button
            className="ss-toolbtn"
            title="Delete selection (Del)"
            disabled={selectedNodes.size === 0 && selectedEdges.size === 0}
            onClick={() => deleteSelection()}
          >
            <Trash2 size={14} />
          </button>
          <button
            className={`ss-toolbtn ${showGrid ? "bg-[color:var(--ss-active)]" : ""}`}
            title="Toggle background grid"
            onClick={() => setShowGrid((v) => !v)}
          >
            <Grid2x2 size={14} />
          </button>
          <button
            className={`ss-toolbtn ${snap ? "bg-[color:var(--ss-active)]" : ""}`}
            title="Snap elements to the grid while dragging"
            onClick={() => setSnap((v) => !v)}
          >
            <Magnet size={14} />
          </button>
          <button
            className={`ss-toolbtn ${showEnergy ? "bg-[color:var(--ss-active)]" : ""}`}
            title="Energy: label each part with its energy in, out and lost in the run shown in Results, and chart the losses"
            aria-pressed={showEnergy}
            onClick={() => useReportsStore.getState().setShowEnergy(!showEnergy)}
          >
            <Zap size={14} />
          </button>
          <button
            className={`ss-toolbtn ${showMiniMap ? "bg-[color:var(--ss-active)]" : ""}`}
            title="Toggle minimap"
            onClick={() => setShowMiniMap((v) => !v)}
          >
            <Map size={14} />
          </button>
        </div>
      </div>
      <div
        ref={wrapperRef}
        className={`relative min-h-0 flex-1${placingDef ? " ss-placing" : ""}`}
        // on the wrapper, so a part dropped on the empty diagram's card lands too
        onDragOver={(e) => {
          e.preventDefault();
          e.dataTransfer.dropEffect = "copy";
        }}
        onDrop={(e) => {
          e.preventDefault();
          const pos = screenToFlowPosition({ x: e.clientX, y: e.clientY });
          // an FMU file from the desktop becomes an FMU block (STD-01)
          const fmus = [...e.dataTransfer.files].filter((f) => f.name.toLowerCase().endsWith(".fmu"));
          if (fmus.length) {
            void (async () => {
              for (const [i, f] of fmus.entries())
                await dropFmuFile(f, { x: pos.x - 46 + 30 * i, y: pos.y - 27 + 30 * i });
            })();
            return;
          }
          const defId = e.dataTransfer.getData("application/lightsim");
          if (!defId) return;
          store.getState().addElement(defId, { x: pos.x - 46, y: pos.y - 27 });
        }}
        onMouseEnter={() => (hovered.current = true)}
        onMouseLeave={() => (hovered.current = false)}
        onMouseMove={(e) => {
          pointer.current = screenToFlowPosition({ x: e.clientX, y: e.clientY });
          pointerAt.current = { x: e.clientX, y: e.clientY };
        }}
        onDoubleClick={(e) => {
          // Coordinate hit-test: the second click of a double-click can land
          // on the pane while React re-renders the selection, so relying on
          // onNodeDoubleClick alone is not enough. Containers drill in;
          // everything else opens its parameter window.
          if (!system) return;
          const pos = screenToFlowPosition({ x: e.clientX, y: e.clientY });
          const hit = system.elements.find((el) => {
            const w = el.size?.width ?? 92;
            const h = el.size?.height ?? 78;
            return (
              pos.x >= el.position.x - 6 &&
              pos.x <= el.position.x + w + 6 &&
              pos.y >= el.position.y - 6 &&
              pos.y <= el.position.y + h + 6
            );
          });
          if (!hit) {
            // empty space: the add-a-part box opens there (not on the toolbar,
            // the minimap or a wire)
            const t = e.target as Element;
            if (!placingId && t.closest?.(".react-flow__pane") && !t.closest(".react-flow__minimap"))
              openAdd(e.clientX, e.clientY);
            return;
          }
          if (hit.isSubSystem && hit.subSystemId) {
            store.getState().setActiveSystem(hit.subSystemId);
          } else {
            useUIStore.getState().openParamDialog(hit.id);
          }
        }}
      >
        <ReactFlow
          nodes={nodes}
          edges={edges}
          nodeTypes={nodeTypes}
          edgeTypes={edgeTypes}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onConnect={onConnect}
          onConnectEnd={(event, state) => {
            // a wire let go over empty space: the box opens there, with the
            // parts it may connect to
            if (state.isValid || state.toNode || !state.fromNode || !state.fromHandle?.id) return;
            if (!(event.target as Element | null)?.closest?.(".react-flow__pane")) return;
            const port = portOf(state.fromNode.id, state.fromHandle.id);
            if (!port || port.kind === "signal") return;
            const pt = "changedTouches" in event ? event.changedTouches[0] : event;
            openAdd(pt.clientX, pt.clientY, { elementId: state.fromNode.id, port });
          }}
          onReconnect={onReconnect}
          isValidConnection={isValidConnection}
          onNodeDragStart={() => {
            store.getState().beginHistory();
            setDragging(true);
          }}
          onNodeDrag={onNodeDrag}
          onNodeDragStop={onNodeDragStop}
          onNodeContextMenu={(e, node) => {
            e.preventDefault();
            if (!selectedNodes.has(node.id)) {
              setSelectedNodes(new Set([node.id]));
              store.getState().select(node.id);
            }
            // on one of its ports: that port's menu
            const pin = (e.target as HTMLElement).closest?.(".react-flow__handle");
            openMenuAt(e.clientX, e.clientY, node.id, pin?.getAttribute("data-handleid") ?? undefined);
          }}
          onPaneContextMenu={(e) => {
            e.preventDefault();
            openMenuAt(
              (e as React.MouseEvent).clientX,
              (e as React.MouseEvent).clientY,
              null,
            );
          }}
          snapToGrid={snap}
          snapGrid={[GRID, GRID]}
          onNodeDoubleClick={(_, node) => {
            const el = node.data.element;
            if (el.isSubSystem && el.subSystemId) {
              store.getState().setActiveSystem(el.subSystemId);
            } else {
              useUIStore.getState().openParamDialog(el.id);
            }
          }}
          onPaneClick={(e) => {
            if (placingId) {
              // click-to-place: drop the armed library part where clicked, after
              // React Flow's own pane-click handling (which clears the selection)
              const pos = screenToFlowPosition({ x: e.clientX, y: e.clientY });
              const defId = placingId;
              setTimeout(() => store.getState().addElement(defId, { x: pos.x - 46, y: pos.y - 27 }));
              useUIStore.getState().setPlacingComponent(null);
              closeMenu();
              return;
            }
            setSelectedNodes(new Set());
            setSelectedEdges(new Set());
            store.getState().select(null);
            closeMenu();
          }}
          // Delete and Backspace are the diagram's own keys (above): one undo
          // step for the whole selection, and only while it has the keyboard
          deleteKeyCode={null}
          nodeDragThreshold={4}
          multiSelectionKeyCode={["Control", "Meta", "Shift"]}
          selectionKeyCode={["Shift"]}
          selectNodesOnDrag
          zoomOnDoubleClick={false}
          colorMode={theme}
          minZoom={0.15}
          maxZoom={2.5}
          // React Flow is MIT-licensed (THIRD-PARTY-NOTICES.txt keeps its
          // notice); the licence lets the link in the corner go
          proOptions={{ hideAttribution: true }}
        >
          {showGrid && (
            <Background
              variant={BackgroundVariant.Lines}
              gap={GRID}
              color="var(--ss-grid-line)"
            />
          )}
          {signalEdges.length > 0 && (
            <ViewportPortal>
              <svg
                className="pointer-events-none absolute left-0 top-0 overflow-visible"
                style={{ width: 1, height: 1 }}
              >
                {signalEdges.map((e) => (
                  <line
                    key={e.id}
                    x1={e.x1}
                    y1={e.y1}
                    x2={e.x2}
                    y2={e.y2}
                    style={{ stroke: KIND_COLOR.signal }}
                    strokeWidth={1.6}
                    strokeDasharray="5 4"
                    strokeLinecap="round"
                    vectorEffect="non-scaling-stroke"
                    opacity={0.85}
                  />
                ))}
              </svg>
            </ViewportPortal>
          )}
          {guides && (
            <ViewportPortal>
              {guides.x.map((x) => (
                <div
                  key={`gx-${x}`}
                  className="pointer-events-none absolute"
                  style={{
                    left: x,
                    top: -100000,
                    width: 1,
                    height: 200000,
                    background: "var(--ss-accent)",
                    opacity: 0.7,
                  }}
                />
              ))}
              {guides.y.map((y) => (
                <div
                  key={`gy-${y}`}
                  className="pointer-events-none absolute"
                  style={{
                    top: y,
                    left: -100000,
                    height: 1,
                    width: 200000,
                    background: "var(--ss-accent)",
                    opacity: 0.7,
                  }}
                />
              ))}
            </ViewportPortal>
          )}
          {showMiniMap && (
            <MiniMap
              pannable
              zoomable
              // the drawing is sized from style (a class would clip it)
              style={{ width: 150, height: 96 }}
              className="rounded border border-[color:var(--ss-border)] shadow-sm"
              bgColor="var(--ss-minimap-bg)"
              maskColor="var(--ss-minimap-mask)"
              nodeColor="var(--ss-minimap-node)"
              nodeStrokeColor="var(--ss-minimap-node-border)"
            />
          )}
        </ReactFlow>
        <EnergyBars />
        {adding && (
          <AddPartBox
            x={adding.x}
            y={adding.y}
            library={library}
            from={adding.from?.port}
            onPick={placeChosen}
            onClose={closeAdd}
          />
        )}
        {placingDef && (
          <div
            role="status"
            className="pointer-events-none absolute left-1/2 top-12 z-10 -translate-x-1/2 whitespace-nowrap rounded border border-[color:var(--ss-accent)] bg-[color:var(--ss-panel)] px-2.5 py-1 text-body text-[color:var(--ss-text)] shadow-sm"
          >
            Click the diagram to place <span className="font-semibold">{placingDef.name}</span> · Esc
            to cancel
          </div>
        )}
        {system && system.elements.length === 0 && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center p-6">
            <div className="max-w-[340px] rounded-lg border border-dashed border-[color:var(--ss-border)] bg-[color:var(--ss-panel)]/70 px-6 py-5 text-center">
              <PackagePlus size={30} className="mx-auto text-[color:var(--ss-accent)]" />
              <div className="mt-2 text-title font-semibold text-[color:var(--ss-text)]">
                Build your topology
              </div>
              <p className="mt-1 text-body leading-relaxed text-[color:var(--ss-text-dim)]">
                Add components from the <span className="font-medium">Components</span> panel: drag
                one here, double-click it or press Enter on it. Or double-click here and type a name.
                Then wire matching ports together.
              </p>
              <ul className="mt-3 space-y-1 text-left text-meta text-[color:var(--ss-text-dim)]">
                <li className="flex items-center gap-1.5">
                  <MousePointerClick size={12} className="shrink-0" />
                  Double-click a node to edit its parameters
                </li>
                <li className="flex items-center gap-1.5">
                  <Waypoints size={12} className="shrink-0" />
                  Drag port to port to connect (same domain only)
                </li>
              </ul>
              {/* the next step as a button (UX-16); the rest of the card lets
                  clicks through to the diagram, and while a part is armed the
                  buttons go, so a click in the middle places it */}
              {!placingDef && (
                <div className="pointer-events-auto mt-3 flex flex-wrap justify-center gap-2">
                  <button
                    className="rounded bg-[color:var(--ss-accent-fill)] px-3 py-1 text-body font-semibold text-white hover:brightness-110 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[color:var(--ss-accent)]"
                    onClick={() => {
                      useUIStore.getState().focusPanel("components");
                      requestAnimationFrame(() =>
                        document.querySelector<HTMLInputElement>("input[aria-label='Search components']")?.focus(),
                      );
                    }}
                  >
                    Add a part
                  </button>
                  <button
                    className="ss-toolbtn border border-[color:var(--ss-field-border)] px-3"
                    onClick={() => useUIStore.getState().setRibbonTab("start")}
                  >
                    Start from an example
                  </button>
                </div>
              )}
            </div>
          </div>
        )}
        {menu && (
          <div
            ref={menuRef}
            className="absolute z-50 min-w-[176px] rounded-md border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] py-1 shadow-lg"
            style={{ left: menu.x, top: menu.y }}
          >
            {menu.nodeId && menuEl && menuPort ? (
              <>
                {/* a port: hide it; a node's terminal: delete it for good */}
                <MenuBtn
                  icon={EyeOff}
                  label="Hide port"
                  disabled={menuPortUsed}
                  title={
                    menuPortUsed
                      ? `${menuPort.name} has a wire or a signal link: only an unused port can be hidden`
                      : `Hide ${menuPort.name} on the diagram (the part's menu shows it again)`
                  }
                  onClick={() => {
                    store.getState().hidePort(menuEl.id, menuPort.id);
                    closeMenu();
                  }}
                />
                {menuJunction && (
                  <MenuBtn
                    icon={Trash2}
                    label="Delete port"
                    danger
                    disabled={menuPortUsed || menuPorts.length <= 2}
                    title={
                      menuPortUsed
                        ? `${menuPort.name} has a wire: delete the wire first`
                        : menuPorts.length <= 2
                          ? "A node keeps at least two terminals"
                          : `Remove ${menuPort.name} from ${menuEl.label}`
                    }
                    onClick={() => {
                      store.getState().deleteTerminal(menuEl.id, menuPort.id);
                      closeMenu();
                    }}
                  />
                )}
              </>
            ) : menu.nodeId ? (
              <>
                <MenuBtn
                  icon={Settings2}
                  label="Parameters"
                  onClick={() => {
                    const el = system?.elements.find((e) => e.id === menu.nodeId);
                    if (el?.isSubSystem && el.subSystemId) store.getState().setActiveSystem(el.subSystemId);
                    else useUIStore.getState().openParamDialog(menu.nodeId!);
                    closeMenu();
                  }}
                />
                <MenuBtn
                  icon={Pencil}
                  label="Rename…"
                  kbd="F2"
                  onClick={() => {
                    const id = menu.nodeId!;
                    closeMenu();
                    void renamePart(id);
                  }}
                />
                {menuSignals && (
                  <MenuBtn
                    icon={Cable}
                    label="Signals…"
                    onClick={() => {
                      // its signal inputs and outputs in Data Bus Connections
                      store.getState().select(menu.nodeId!);
                      const ui = useUIStore.getState();
                      ui.setBusSelectedOnly(true);
                      ui.focusPanel("data-bus");
                      closeMenu();
                    }}
                  />
                )}
                {/* only for a part whose ports were moved */}
                {menuEl && Object.keys({ ...menuEl.portSides, ...menuEl.portOffsets }).length > 0 && (
                  <MenuBtn
                    icon={RotateCcw}
                    label="Reset port places"
                    onClick={() => {
                      store.getState().resetPortPlaces(menuEl.id);
                      closeMenu();
                    }}
                  />
                )}
                {/* only for a part with hidden ports: all back, or one */}
                {menuEl && menuHidden.length > 0 && (
                  <MenuBtn
                    icon={Eye}
                    label="Show hidden ports"
                    onClick={() => {
                      store.getState().showPorts(menuEl.id);
                      closeMenu();
                    }}
                  />
                )}
                {menuEl &&
                  menuHidden.length > 1 &&
                  menuHidden.map((p) => (
                    <MenuBtn
                      key={p.id}
                      icon={Eye}
                      indent
                      label={p.name}
                      title={`Show ${p.name} only`}
                      onClick={() => {
                        store.getState().showPorts(menuEl.id, p.id);
                        closeMenu();
                      }}
                    />
                  ))}
                {menuEl && menuJunction && (
                  <MenuBtn
                    icon={Plus}
                    label="Add a port"
                    title={`One more ${JUNCTIONS[menuLib!.id].toLowerCase()} on ${menuEl.label}`}
                    onClick={() => {
                      store.getState().addTerminal(menuEl.id);
                      closeMenu();
                    }}
                  />
                )}
                <div className="my-1 h-px bg-[color:var(--ss-border)]" />
                <MenuBtn
                  icon={CopyPlus}
                  label={`Duplicate${selectedNodes.size > 1 ? ` (${selectedNodes.size})` : ""}`}
                  kbd="Ctrl+D"
                  onClick={() => {
                    selectNew(store.getState().duplicateElements([...selectedNodes]));
                    closeMenu();
                  }}
                />
                <MenuBtn
                  icon={Copy}
                  label={`Copy${selectedNodes.size > 1 ? ` (${selectedNodes.size})` : ""}`}
                  kbd="Ctrl+C"
                  onClick={() => {
                    copySelection();
                    closeMenu();
                  }}
                />
                <MenuBtn
                  icon={Scissors}
                  label={`Cut${selectedNodes.size > 1 ? ` (${selectedNodes.size})` : ""}`}
                  kbd="Ctrl+X"
                  onClick={() => {
                    cutSelection();
                    closeMenu();
                  }}
                />
                <div className="my-1 h-px bg-[color:var(--ss-border)]" />
                <MenuBtn
                  icon={Trash2}
                  label={`Delete${selectedNodes.size > 1 ? ` (${selectedNodes.size})` : ""}`}
                  kbd="Del"
                  danger
                  onClick={() => {
                    deleteSelection();
                    closeMenu();
                  }}
                />
              </>
            ) : (
              <>
                <MenuBtn
                  icon={ClipboardPaste}
                  label="Paste"
                  kbd="Ctrl+V"
                  disabled={!clipboard}
                  onClick={() => {
                    selectNew(store.getState().pasteClipboard(pointer.current ?? undefined));
                    closeMenu();
                  }}
                />
                <MenuBtn
                  icon={BoxSelect}
                  label="Select all"
                  kbd="Ctrl+A"
                  onClick={() => {
                    selectAll();
                    closeMenu();
                  }}
                />
                <MenuBtn
                  icon={Maximize}
                  label="Fit view"
                  onClick={() => {
                    fitAll();
                    closeMenu();
                  }}
                />
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

export function TopologyCanvas() {
  return (
    <ReactFlowProvider>
      <TopologyCanvasInner />
    </ReactFlowProvider>
  );
}
