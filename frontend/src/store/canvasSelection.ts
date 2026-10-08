// The parts and wires selected on the model diagram, and what the diagram's
// keys, its menus, the ribbon and the Elements list do with them (UX-19).
// Kept apart from the canvas so the ribbon's Delete acts on the same
// selection as the Delete key, with one undo step, whether or not the
// diagram is on screen. Built only on the project store's own actions.
import { create } from "zustand";
import { promptDialog } from "../dialog";
import { useProjectStore } from "./projectStore";
import { useUIStore } from "./uiStore";

type Ids = ReadonlySet<string>;
type Update = Ids | ((prev: Ids) => Ids);

export interface CanvasSelection {
  /** selected parts, in the order they were picked (the last is the one
   *  Properties shows) */
  nodes: Ids;
  /** selected wires */
  edges: Ids;
  setNodes: (update: Update) => void;
  setEdges: (update: Update) => void;
}

const EMPTY: Ids = new Set();

export const useCanvasSelection = create<CanvasSelection>((set, get) => ({
  nodes: EMPTY,
  edges: EMPTY,
  setNodes: (update) => set({ nodes: typeof update === "function" ? update(get().nodes) : update }),
  setEdges: (update) => set({ edges: typeof update === "function" ? update(get().edges) : update }),
}));

/** Keep only the ids still in `valid` (the same set when nothing went). */
function prune(ids: Ids, valid: Set<string>): Ids {
  for (const id of ids) if (!valid.has(id)) return new Set([...ids].filter((x) => valid.has(x)));
  return ids;
}

// The selection follows the project: a part selected elsewhere (Properties,
// Elements, Problems) becomes the selection unless it is already in it, so
// picking a part of a multi-selection keeps the others; entering another
// sub-system clears it; and parts and wires that are gone (deleted, undone,
// another project) drop out of it.
useProjectStore.subscribe((s, prev) => {
  const sel = useCanvasSelection.getState();
  if (s.activeSystemId !== prev.activeSystemId) {
    if (sel.nodes.size || sel.edges.size) useCanvasSelection.setState({ nodes: EMPTY, edges: EMPTY });
  }
  if (s.selectedElementId !== prev.selectedElementId) {
    const id = s.selectedElementId;
    sel.setNodes((nodes) => (!id ? (nodes.size ? EMPTY : nodes) : nodes.has(id) ? nodes : new Set([id])));
  }
  if (s.project !== prev.project) {
    const parts = new Set<string>();
    const wires = new Set<string>();
    for (const sy of s.project?.systems ?? []) {
      for (const el of sy.elements) parts.add(el.id);
      for (const c of sy.connections) wires.add(c.id);
    }
    const now = useCanvasSelection.getState();
    const nodes = prune(now.nodes, parts);
    const edges = prune(now.edges, wires);
    if (nodes !== now.nodes || edges !== now.edges) useCanvasSelection.setState({ nodes, edges });
  }
});
useCanvasSelection.getState().setNodes(() => {
  const id = useProjectStore.getState().selectedElementId;
  return id ? new Set([id]) : EMPTY;
});

/** The selected parts; with none on the diagram, the selected element (a
 *  part picked in a list while the diagram was closed). */
export function selectedParts(): string[] {
  const { nodes } = useCanvasSelection.getState();
  if (nodes.size) return [...nodes];
  const id = useProjectStore.getState().selectedElementId;
  return id ? [id] : [];
}

/** The one part F2 and Enter act on: the one Properties shows. */
function currentPart(): string | null {
  const parts = selectedParts();
  const id = useProjectStore.getState().selectedElementId;
  return id && parts.includes(id) ? id : (parts[parts.length - 1] ?? null);
}

function clear() {
  useCanvasSelection.setState({ nodes: EMPTY, edges: EMPTY });
}

/** Delete the selected parts and wires, in one undo step. False when
 *  nothing is selected. */
export function deleteSelection(): boolean {
  const parts = selectedParts();
  const wires = [...useCanvasSelection.getState().edges];
  if (parts.length === 0 && wires.length === 0) return false;
  useProjectStore.getState().removeElements(parts, wires);
  clear();
  return true;
}

/** Copy the selected parts (and the wires between them). */
export function copySelection(): boolean {
  const parts = selectedParts();
  if (parts.length === 0) return false;
  useProjectStore.getState().copyElements(parts);
  return true;
}

/** Copy the selected parts, then delete them and the selected wires: one
 *  undo step brings them back (the clipboard is not in the history). */
export function cutSelection(): boolean {
  if (!copySelection()) return false;
  return deleteSelection();
}

/** Select every part of the sub-system shown. */
export function selectAll(): boolean {
  const st = useProjectStore.getState();
  const sys = st.project?.systems.find((sy) => sy.id === st.activeSystemId);
  if (!sys || sys.elements.length === 0) return false;
  const ids = sys.elements.map((e) => e.id);
  // the one Properties shows stays last, so it stays shown
  const keep = st.selectedElementId && ids.includes(st.selectedElementId) ? st.selectedElementId : null;
  const ordered = keep ? [...ids.filter((id) => id !== keep), keep] : ids;
  useCanvasSelection.getState().setNodes(new Set(ordered));
  if (!keep) st.select(ordered[ordered.length - 1]);
  return true;
}

/** Ask for a part's new name (the part menu's Rename…, F2). */
export async function renamePart(id: string): Promise<void> {
  const st = useProjectStore.getState();
  const el = st.project?.systems.flatMap((s) => s.elements).find((e) => e.id === id);
  if (!el) return;
  const name = await promptDialog({ title: "Rename element", defaultValue: el.label, confirmLabel: "Rename" });
  if (name != null && name.trim()) useProjectStore.getState().renameElement(id, name.trim());
}

/** F2: rename the part Properties shows. */
export function renameSelection(): boolean {
  const id = currentPart();
  if (!id) return false;
  void renamePart(id);
  return true;
}

/** Enter: bring Properties forward with the keyboard focus in the selected
 *  part's first field (its name), waiting a few frames for the panel to
 *  show. */
export function focusProperties(): boolean {
  const id = currentPart();
  if (!id) return false;
  const st = useProjectStore.getState();
  if (st.selectedElementId !== id) st.select(id);
  useUIStore.getState().focusPanel("properties");
  let frames = 10;
  const focusName = () => {
    const field = document.querySelector<HTMLInputElement>("[data-properties-panel] input[aria-label='Name']");
    if (field && field.offsetParent !== null) field.focus();
    else if (--frames > 0) requestAnimationFrame(focusName);
  };
  requestAnimationFrame(focusName);
  return true;
}
