// Moving a port round its part: where it may go, that the place is saved
// with the project and comes back, one undo step per move, the part's
// right-click menu putting the library's places back, and files from before
// (with no places saved) drawn as they always were.
import { beforeEach, describe, expect, it } from "vitest";
import libraryJson from "../../data/componentLibrary.json";
import { useProjectStore } from "../../store/projectStore";
import type { ComponentDef, ElementInstance, Project } from "../../types";
import { PORT_GAP, movedPlace, nodeSize, portPlaces, portSpot, routingInput } from "./portPlaces";

const library = (libraryJson as unknown as { components: ComponentDef[] }).components;
const libraryById = Object.fromEntries(library.map((d) => [d.id, d]));
const node: ElementInstance = { id: "n", label: "Node", componentDefId: "mech.node", position: { x: 0, y: 0 }, parameterOverrides: {} };
const brake: ElementInstance = { id: "b", label: "Brake", componentDefId: "mech.brake", position: { x: -200, y: 0 }, parameterOverrides: {} };

describe("where a moved port may go", () => {
  it("snaps to the nearest sixth of a side", () => {
    expect(portSpot(0.3, [], 92)).toBeCloseTo(2 / 6);
    expect(portSpot(0.95, [], 92)).toBeCloseTo(5 / 6);
  });
  it("never onto another port: the nearest free place instead", () => {
    // a port at the middle of a 92-wide side: the sixth next to it is 15 away, free
    expect(portSpot(0.5, [0.5], 92)).toBeCloseTo(2 / 6);
    // on a 54-high side every sixth is 9 apart: the ones beside a port are taken
    const f = portSpot(0.5, [0.5], 54)!;
    expect(Math.abs(f - 0.5) * 54).toBeGreaterThanOrEqual(PORT_GAP);
  });
  it("is refused on a side with no room left", () => {
    expect(portSpot(0.5, [0.2, 0.4, 0.6, 0.8], 54)).toBeNull();
  });
  it("keeps the library's places until a port is moved", () => {
    const places = portPlaces(node, libraryById["mech.node"]);
    expect(places.f1).toEqual({ side: "left", frac: 0.5 });
    expect(places.f2.side).toBe("right");
    expect(nodeSize(node, places)).toEqual({ width: 92, height: 72 });
  });
});

describe("a moved port in the project", () => {
  beforeEach(() => {
    useProjectStore.setState({
      project: {
        id: "x", name: "X", cases: [], dataBusConnections: [],
        systems: [{ id: "root", name: "Root", parentId: null, elements: [structuredClone(node), structuredClone(brake)],
          connections: [{ id: "c", sourceElementId: "n", sourcePortId: "f2", targetElementId: "b", targetPortId: "flange" }] }],
      } as unknown as Project,
      activeSystemId: "root",
      libraryById,
      past: [],
      future: [],
    } as never);
  });
  const el = (id: string) => useProjectStore.getState().project!.systems[0].elements.find((e) => e.id === id)!;
  const def = libraryById["mech.node"];

  it("is saved per part and port, every other port of the part where it was, as one undo step", () => {
    const before = portPlaces(el("n"), def);
    const place = movedPlace(el("n"), def, "f2", "left", 0.2)!;
    useProjectStore.getState().placePort("n", "f2", place);
    const after = portPlaces(el("n"), def);
    expect(after.f2.side).toBe("left");
    expect(after.f2.frac).toBeCloseTo(1 / 6, 4);
    for (const p of ["f1", "f3", "f4"]) expect(after[p]).toEqual(before[p]);
    expect(el("n").portSides).toEqual({ f1: "left", f2: "left", f3: "right", f4: "right" });
    expect(useProjectStore.getState().past).toHaveLength(1);
    useProjectStore.getState().undo();
    expect(el("n").portSides).toBeUndefined();
    // and the wire now leaves the node on its left
    useProjectStore.getState().redo();
    const { wires } = routingInput(useProjectStore.getState().project!.systems[0], libraryById);
    expect(wires[0].from).toMatchObject({ side: "left", x: 0 });
  });

  it("comes back when the project is saved and opened again", () => {
    useProjectStore.getState().placePort("n", "f3", { side: "top", frac: 0.5 });
    const file = JSON.parse(JSON.stringify(useProjectStore.getState().project)) as Project;
    const opened = file.systems[0].elements.find((e) => e.id === "n")!;
    expect(portPlaces(opened, def)).toEqual(portPlaces(el("n"), def));
    expect(portPlaces(opened, def).f3).toEqual({ side: "top", frac: 0.5 });
  });

  it("goes back to the library's places from the part's menu, as one undo step", () => {
    useProjectStore.getState().placePort("n", "f3", { side: "top", frac: 0.5 });
    useProjectStore.getState().resetPortPlaces("n");
    expect(el("n").portSides).toBeUndefined();
    expect(el("n").portOffsets).toBeUndefined();
    expect(portPlaces(el("n"), def)).toEqual(portPlaces(node, def));
    expect(useProjectStore.getState().past).toHaveLength(2);
  });

  it("leaves a file from before (no places saved) as it was: nothing added, drawn as always", () => {
    useProjectStore.getState().placePort("n", "f3", { side: "top", frac: 0.5 });
    expect(el("b")).toEqual(brake);
    expect(portPlaces(el("b"), libraryById["mech.brake"])).toEqual({ flange: { side: "left", frac: 0.5 } });
  });
});
