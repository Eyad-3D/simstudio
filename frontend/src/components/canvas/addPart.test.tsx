// Adding a part from the diagram: which parts a wire from a port may reach
// and to which of their ports, where the part goes, the box's keys (Enter
// places, Esc leaves nothing), and a part with its wire as one undo step.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import libraryJson from "../../data/componentLibrary.json";
import { useProjectStore } from "../../store/projectStore";
import type { ComponentDef, PortDef, Project } from "../../types";
import { AddPartBox } from "./AddPartBox";
import { partSpot, partsFor, portsJoin, wirePort } from "./addPart";
import { canvasAction } from "./canvasKeys";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const library = (libraryJson as unknown as { components: ComponentDef[] }).components;
const def = (id: string) => library.find((d) => d.id === id)!;
const port = (id: string, p: string) => def(id).ports.find((x) => x.id === p)! as PortDef;
const ids = (defs: ComponentDef[]) => defs.map((d) => d.id);

describe("the parts a dragged wire may reach", () => {
  it("joins only ports of the same kind, never a signal (the canvas's rule)", () => {
    expect(portsJoin({ kind: "electrical" }, { kind: "electrical" })).toBe(true);
    expect(portsJoin({ kind: "electrical" }, { kind: "mechanical" })).toBe(false);
    expect(portsJoin({ kind: "signal" }, { kind: "signal" })).toBe(false);
  });
  it("from a battery terminal: the parts with an electrical port, none without", () => {
    const parts = ids(partsFor(library, "", port("battery.generic", "neg")));
    expect(parts).toEqual(expect.arrayContaining(["motor.emotor", "controller.dcdc", "electric.node", "boundary.ground"]));
    expect(parts).not.toContain("propulsion.wheel");
    expect(parts).not.toContain("signal.monitor");
    expect(parts.every((id) => def(id).ports.some((p) => p.kind === "electrical"))).toBe(true);
  });
  it("from a shaft: the mechanical parts; and the search narrows them as the Components panel's does", () => {
    const shaft = port("motor.emotor", "shaft");
    expect(ids(partsFor(library, "", shaft))).toEqual(expect.arrayContaining(["mech.gearbox", "propulsion.wheel", "mech.brake"]));
    expect(ids(partsFor(library, "transmission", shaft))).toEqual(["mech.gearbox"]);
    // from a battery terminal, "e-motor" finds the E-Motor only; "motor"
    // also finds the battery, whose description names the motor, after it
    expect(ids(partsFor(library, "e-motor", port("battery.generic", "pos")))).toEqual(["motor.emotor"]);
    expect(ids(partsFor(library, "motor", port("battery.generic", "pos")))).toEqual(["motor.emotor", "battery.generic"]);
  });
  it("puts the parts whose name matches first, as Enter takes the first", () => {
    const found = ids(partsFor(library, "brake"));
    expect(found[0]).toBe("mech.brake");
    // the same parts as the Components panel finds, only in another order
    expect([...found].sort()).toEqual(ids(library.filter((d) => partsFor([d], "brake").length)).sort());
  });
  it("without a wire, the box finds what the Components panel finds", () => {
    expect(ids(partsFor(library, "ice"))).toEqual(["engine.combustion", "fuel.tank"]);
    expect(partsFor(library, "")).toHaveLength(library.length);
  });
});

describe("the port the wire goes to", () => {
  it("the one with the same id: a negative terminal to the motor's negative, not its first electrical port", () => {
    expect(wirePort(def("motor.emotor"), port("battery.generic", "neg"))).toBe("neg");
    expect(wirePort(def("motor.emotor"), port("battery.generic", "pos"))).toBe("pos");
  });
  it("else the first fitting port in the library's order", () => {
    expect(wirePort(def("controller.dcdc"), port("battery.generic", "neg"))).toBe("a_pos");
    expect(wirePort(def("mech.gearbox"), port("motor.emotor", "shaft"))).toBe("flange_in");
    expect(wirePort(def("propulsion.wheel"), port("battery.generic", "pos"))).toBeNull();
  });
});

describe("where the part goes", () => {
  it("centred on the spot, on the grid only with Snap on", () => {
    expect(partSpot({ x: 300, y: 200 }, null)).toEqual({ x: 254, y: 173 });
    expect(partSpot({ x: 300, y: 200 }, 20)).toEqual({ x: 260, y: 180 });
  });
  it("Shift+A opens the box; A alone and Ctrl+Shift+A do nothing", () => {
    const k = (key: string, shiftKey = false, ctrlKey = false) => canvasAction({ key, shiftKey, ctrlKey, metaKey: false, altKey: false });
    expect(k("A", true)).toBe("addPart");
    expect(k("a")).toBeNull();
    expect(k("A", true, true)).toBeNull();
  });
});

describe("the box and the store", () => {
  let root: Root;
  const battery = { id: "b", label: "HV Battery Pack 1", componentDefId: "battery.generic", position: { x: 0, y: 0 }, parameterOverrides: {} };
  beforeEach(() => {
    root = createRoot(document.body.appendChild(document.createElement("div")));
    useProjectStore.setState({
      project: {
        id: "x", name: "X", cases: [], dataBusConnections: [],
        systems: [{ id: "root", name: "Root", parentId: null, connections: [], elements: [battery] }],
      } as unknown as Project,
      activeSystemId: "root",
      libraryById: Object.fromEntries(library.map((d) => [d.id, d])),
      past: [],
      future: [],
    } as never);
  });
  afterEach(() => {
    act(() => root.unmount());
    document.body.innerHTML = "";
  });
  const sys = () => useProjectStore.getState().project!.systems[0];
  const key = (el: Element, k: string) => act(() => void el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true })));
  const type = (input: HTMLInputElement, text: string) =>
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, text);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
  /** The box as the diagram shows it for a wire from the battery's negative terminal. */
  const open = (onClose = vi.fn()) => {
    const from = port("battery.generic", "neg");
    const onPick = (d: ComponentDef) =>
      useProjectStore.getState().addElement(d.id, partSpot({ x: 400, y: 120 }, null), {
        elementId: "b",
        portId: from.id,
        toPortId: wirePort(d, from)!,
      });
    act(() => root.render(<AddPartBox x={10} y={10} library={library} from={from} onPick={onPick} onClose={onClose} />));
    return { input: document.querySelector<HTMLInputElement>("[data-add-part] input")!, onClose };
  };
  const options = () => [...document.querySelectorAll("[role=option]")].map((o) => o.textContent);

  it("typing filters, the arrows move, Enter places the part at the spot wired to its fitting port, in one undo step", () => {
    const { input } = open();
    expect(document.activeElement).toBe(input);
    expect(options()).not.toContain("Wheel");
    type(input, "e-mot");
    expect(options()).toEqual(["E-MotorNegative Terminal (−)"]);
    type(input, "");
    key(input, "ArrowDown");
    expect(document.querySelectorAll("[role=option]")[1].getAttribute("aria-selected")).toBe("true");
    type(input, "e-motor");
    key(input, "Enter");
    const motor = sys().elements.find((e) => e.componentDefId === "motor.emotor")!;
    expect(motor.position).toEqual({ x: 354, y: 93 });
    expect(sys().connections).toEqual([
      expect.objectContaining({ sourceElementId: "b", sourcePortId: "neg", targetElementId: motor.id, targetPortId: "neg" }),
    ]);
    expect(useProjectStore.getState().past).toHaveLength(1);
    act(() => useProjectStore.getState().undo());
    expect(sys().elements).toEqual([battery]);
    expect(sys().connections).toEqual([]);
  });

  it("Esc, or a press elsewhere, closes it and leaves no part and no wire", () => {
    const { input, onClose } = open();
    type(input, "motor");
    key(input, "Escape");
    expect(onClose).toHaveBeenCalledTimes(1);
    act(() => void document.body.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true })));
    expect(onClose).toHaveBeenCalledTimes(2);
    expect(sys().elements).toEqual([battery]);
    expect(sys().connections).toEqual([]);
    expect(useProjectStore.getState().past).toHaveLength(0);
  });

  it("a wire between ports of different kinds is not made: the part alone goes in", () => {
    act(() => useProjectStore.getState().addElement("propulsion.wheel", { x: 0, y: 0 }, { elementId: "b", portId: "neg", toPortId: "shaft" }));
    expect(sys().elements).toHaveLength(2);
    expect(sys().connections).toEqual([]);
  });
});
