// UX-19: the diagram's selection, shared with the ribbon, and what its keys
// do with it: delete and cut in one undo step, select all.
import { afterAll, beforeEach, describe, expect, it } from "vitest";
import type { ElementInstance, Project } from "../types";
import {
  cutSelection,
  deleteSelection,
  selectAll,
  selectedParts,
  useCanvasSelection,
} from "./canvasSelection";
import { stopRechecks, useProjectStore } from "./projectStore";

// no engine here: the store's quiet re-checks would call it
stopRechecks();

const el = (id: string, label: string): ElementInstance => ({
  id,
  componentDefId: "mech.shaft",
  label,
  position: { x: 0, y: 0 },
  parameterOverrides: {},
});
const wire = (id: string, a: string, b: string) => ({
  id,
  sourceElementId: a,
  sourcePortId: "out",
  targetElementId: b,
  targetPortId: "in",
});

function fixture(): Project {
  return {
    id: "p",
    name: "P",
    systems: [
      {
        id: "sys-root",
        name: "P",
        parentId: null,
        elements: [el("a", "A"), el("b", "B"), el("c", "C"), el("d", "D")],
        connections: [wire("ab", "a", "b"), wire("bc", "b", "c"), wire("cd", "c", "d")],
      },
    ],
    dataBusConnections: [],
    cases: [{ id: "case-1", name: "Case 1", duration: 10, timeStep: 1 }],
  } as Project;
}

const st = () => useProjectStore.getState();
const sel = () => useCanvasSelection.getState();
const parts = () => st().project!.systems[0].elements.map((e) => e.id);
const wires = () => st().project!.systems[0].connections.map((c) => c.id);

beforeEach(() => {
  useProjectStore.setState({
    project: fixture(),
    activeSystemId: "sys-root",
    selectedElementId: null,
    clipboard: null,
    past: [],
    future: [],
  });
  useCanvasSelection.setState({ nodes: new Set(), edges: new Set() });
});
afterAll(() => useProjectStore.setState({ project: null }));

describe("the selection follows the selected element", () => {
  it("a part picked elsewhere becomes the selection", () => {
    st().select("b");
    expect([...sel().nodes]).toEqual(["b"]);
    st().select(null);
    expect(sel().nodes.size).toBe(0);
  });

  it("picking a part of a multi-selection keeps the others", () => {
    sel().setNodes(new Set(["a", "b", "c"]));
    st().select("b");
    expect([...sel().nodes]).toEqual(["a", "b", "c"]);
  });

  it("entering another sub-system clears it", () => {
    sel().setNodes(new Set(["a", "b"]));
    sel().setEdges(new Set(["ab"]));
    st().setActiveSystem("sys-other");
    expect(sel().nodes.size + sel().edges.size).toBe(0);
  });

  it("parts and wires that are gone drop out of it", () => {
    sel().setNodes(new Set(["a", "b"]));
    sel().setEdges(new Set(["cd"]));
    st().removeElements(["b"], []); // and its wires ab and bc
    expect([...sel().nodes]).toEqual(["a"]);
    expect([...sel().edges]).toEqual(["cd"]);
  });
});

describe("delete", () => {
  it("deletes every selected part and wire, and one undo brings them all back", () => {
    sel().setNodes(new Set(["a", "c"]));
    sel().setEdges(new Set(["bc"]));
    expect(deleteSelection()).toBe(true);
    expect(parts()).toEqual(["b", "d"]);
    expect(wires()).toEqual([]);
    expect(sel().nodes.size + sel().edges.size).toBe(0);
    expect(st().past).toHaveLength(1);
    st().undo();
    expect(parts()).toEqual(["a", "b", "c", "d"]);
    expect(wires()).toEqual(["ab", "bc", "cd"]);
  });

  it("deletes only wires", () => {
    sel().setEdges(new Set(["ab", "cd"]));
    deleteSelection();
    expect(parts()).toHaveLength(4);
    expect(wires()).toEqual(["bc"]);
  });

  it("with nothing on the diagram, deletes the selected element", () => {
    st().select("d");
    useCanvasSelection.setState({ nodes: new Set() }); // the diagram is closed
    expect(selectedParts()).toEqual(["d"]);
    deleteSelection();
    expect(parts()).toEqual(["a", "b", "c"]);
  });

  it("does nothing, and adds no undo step, with nothing selected", () => {
    expect(deleteSelection()).toBe(false);
    expect(st().past).toHaveLength(0);
  });
});

describe("cut", () => {
  it("copies the parts, deletes them in one undo step, and pastes them back", () => {
    sel().setNodes(new Set(["b", "c"]));
    expect(cutSelection()).toBe(true);
    expect(parts()).toEqual(["a", "d"]);
    expect(st().clipboard?.elements.map((e) => e.label)).toEqual(["B", "C"]);
    expect(st().past).toHaveLength(1);
    const pasted = st().pasteClipboard();
    expect(pasted).toHaveLength(2);
    // the wire between the cut parts comes with them
    expect(st().project!.systems[0].connections).toHaveLength(1);
  });

  it("does nothing with no part selected", () => {
    sel().setEdges(new Set(["ab"]));
    expect(cutSelection()).toBe(false);
    expect(wires()).toHaveLength(3);
  });
});

describe("select all", () => {
  it("selects every part of the sub-system shown, keeping the part Properties shows", () => {
    st().select("b");
    expect(selectAll()).toBe(true);
    expect([...sel().nodes].sort()).toEqual(["a", "b", "c", "d"]);
    expect(st().selectedElementId).toBe("b");
    expect([...sel().nodes].at(-1)).toBe("b");
  });

  it("selects the last part for Properties when none was selected", () => {
    selectAll();
    expect(sel().nodes.size).toBe(4);
    expect(st().selectedElementId).toBe("d");
  });
});
