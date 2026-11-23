// Hiding a port and showing it again, and a node's terminals: added and
// deleted. Each is one undo step and saved with the project; a port in use
// can be neither hidden nor deleted, a node keeps at least two terminals, and
// a hidden port is not there for wiring until it is shown again.
import { beforeEach, describe, expect, it } from "vitest";
import libraryJson from "../../data/componentLibrary.json";
import { portsOf, useProjectStore } from "../../store/projectStore";
import type { ComponentDef, ElementInstance, Project } from "../../types";
import { drawnPorts, nodeSize, partDef, portPlaces, routingInput } from "./portPlaces";

const library = (libraryJson as unknown as { components: ComponentDef[] }).components;
const libraryById = Object.fromEntries(library.map((d) => [d.id, d]));
const at = { x: 0, y: 0 };
const part = (id: string, componentDefId: string): ElementInstance => ({
  id, label: id, componentDefId, position: at, parameterOverrides: {},
});

beforeEach(() => {
  // a battery on an electric node's t1, a motor on its t2; the battery's SOC linked to a Monitor
  useProjectStore.setState({
    project: {
      id: "x", name: "X", cases: [],
      systems: [{
        id: "root", name: "Root", parentId: null,
        elements: [part("bus", "electric.node"), part("bat", "battery.generic"), part("mot", "motor.emotor"),
          part("fl", "mech.node")],
        connections: [
          { id: "c1", sourceElementId: "bat", sourcePortId: "pos", targetElementId: "bus", targetPortId: "t1" },
          { id: "c2", sourceElementId: "bus", sourcePortId: "t2", targetElementId: "mot", targetPortId: "pos" },
        ],
      }],
      dataBusConnections: [{ id: "d1", element1Id: "bat", port1Id: "sig_soc", element2Id: "mot", port2Id: "sig_torque_cmd" }],
    } as unknown as Project,
    activeSystemId: "root",
    libraryById,
    past: [],
    future: [],
  } as never);
});
const st = () => useProjectStore.getState();
const el = (id: string) => st().project!.systems[0].elements.find((e) => e.id === id)!;
const ids = (id: string) => drawnPorts(el(id), partDef(el(id), libraryById[el(id).componentDefId])).map((p) => p.id);

describe("hiding a port", () => {
  it("hides an unused port, as one undo step, and the part's menu shows it again", () => {
    expect(st().hidePort("bus", "t5")).toBe(true);
    expect(ids("bus")).toEqual(["t1", "t2", "t3", "t4"]);
    expect(portPlaces(el("bus"), libraryById["electric.node"]).t5).toBeUndefined();
    expect(st().past).toHaveLength(1);
    st().undo();
    expect(el("bus").hiddenPorts).toBeUndefined();
    st().redo();
    st().hidePort("bus", "t4");
    st().showPorts("bus", "t5"); // one back
    expect(el("bus").hiddenPorts).toEqual(["t4"]);
    st().showPorts("bus"); // all back
    expect(el("bus").hiddenPorts).toBeUndefined();
    expect(ids("bus")).toEqual(["t1", "t2", "t3", "t4", "t5"]);
    expect(st().past).toHaveLength(4);
  });

  it("refuses a port with a wire or a signal link", () => {
    expect(st().hidePort("bus", "t1")).toBe(false); // a wire
    expect(st().hidePort("bat", "sig_soc")).toBe(false); // a signal link
    expect(el("bus").hiddenPorts).toBeUndefined();
    expect(st().past).toHaveLength(0);
  });

  it("is saved with the project and drawn hidden when it is opened again", () => {
    st().hidePort("bus", "t5");
    const file = JSON.parse(JSON.stringify(st().project)) as Project;
    const opened = file.systems[0].elements.find((e) => e.id === "bus")!;
    expect(opened.hiddenPorts).toEqual(["t5"]);
    expect(drawnPorts(opened, libraryById["electric.node"]).map((p) => p.id)).not.toContain("t5");
  });

  it("is not there for wiring until it is shown: no pin, no wire to it, and shown it takes one", () => {
    st().hidePort("bus", "t5");
    // no pin on the diagram, so no wire starts or ends there; the router has no place for it
    expect(Object.keys(portPlaces(el("bus"), libraryById["electric.node"]))).not.toContain("t5");
    st().showPorts("bus");
    // shown again, a part added from a wire dragged off it is wired to it
    st().addElement("electric.constant_drive", { x: 300, y: 0 }, { elementId: "bus", portId: "t5", toPortId: "pos" });
    const wired = st().project!.systems[0].connections.find((c) => c.sourcePortId === "t5");
    expect(wired).toBeTruthy();
    // and in use it cannot be hidden
    expect(st().hidePort("bus", "t5")).toBe(false);
  });

  it("comes back at a free place on a part whose ports were moved", () => {
    st().placePort("bus", "t1", { side: "top", frac: 0.5 });
    st().hidePort("bus", "t3");
    st().placePort("bus", "t2", { side: "right", frac: 0.5 }); // where t3 was
    st().showPorts("bus");
    const places = portPlaces(el("bus"), libraryById["electric.node"]);
    const right = Object.entries(places).filter(([, p]) => p.side === "right").map(([, p]) => p.frac);
    expect(new Set(right).size).toBe(right.length); // no two ports in one place
  });
});

describe("a node's terminals", () => {
  const bus = () => partDef(el("bus"), libraryById["electric.node"]);

  it("gets one more from Add a port, as one undo step, and wires to it", () => {
    st().addTerminal("bus");
    expect(el("bus").terminals).toEqual(["t1", "t2", "t3", "t4", "t5", "t6"]);
    const t6 = bus().ports.find((p) => p.id === "t6")!;
    expect(t6).toMatchObject({ name: "Terminal 6", kind: "electrical" });
    expect(bus().ports.at(-1)!.id).toBe("sig_power"); // its signal stays
    // the node grows to fit it
    expect(nodeSize(el("bus"), portPlaces(el("bus"), bus())).height).toBeGreaterThan(90);
    expect(st().past).toHaveLength(1);
    st().addElement("electric.constant_drive", { x: 300, y: 0 }, { elementId: "bus", portId: "t6", toPortId: "pos" });
    const { wires } = routingInput(st().project!.systems[0], libraryById);
    expect(wires.find((w) => w.from.side === "right" && w.id !== "c2")).toBeTruthy();
    st().undo();
    st().undo();
    expect(el("bus").terminals).toBeUndefined();
  });

  it("names a mechanical node's new terminal a flange", () => {
    st().addTerminal("fl");
    st().addTerminal("fl");
    expect(el("fl").terminals).toEqual(["f1", "f2", "f3", "f4", "f5", "f6"]);
    expect(portsOf(el("fl"), libraryById).find((p) => p.id === "f6")).toMatchObject({ name: "Flange 6", kind: "mechanical" });
  });

  it("loses an unused one from Delete port, for good, as one undo step", () => {
    expect(st().deleteTerminal("bus", "t3")).toBe(true);
    expect(el("bus").terminals).toEqual(["t1", "t2", "t4", "t5"]);
    expect(st().past).toHaveLength(1);
    // the next added takes the next number, not the gap
    st().addTerminal("bus");
    expect(el("bus").terminals).toEqual(["t1", "t2", "t4", "t5", "t6"]);
  });

  it("cannot delete one with a wire, nor go below two", () => {
    expect(st().deleteTerminal("bus", "t1")).toBe(false);
    expect(st().deleteTerminal("bus", "t5")).toBe(true);
    expect(st().deleteTerminal("bus", "t4")).toBe(true);
    expect(st().deleteTerminal("bus", "t3")).toBe(true);
    expect(el("bus").terminals).toEqual(["t1", "t2"]);
    expect(st().deleteTerminal("bus", "t2")).toBe(false); // wired, and the last two
    st().undo();
    expect(el("bus").terminals).toEqual(["t1", "t2", "t3"]);
  });

  it("back to the library's terminals, the part is as it was in the file", () => {
    st().addTerminal("bus");
    st().deleteTerminal("bus", "t6");
    expect(el("bus")).toEqual(part("bus", "electric.node"));
  });

  it("is offered on the two nodes only", () => {
    st().addTerminal("bat");
    expect(el("bat").terminals).toBeUndefined();
    expect(st().deleteTerminal("bat", "neg")).toBe(false);
  });
});
