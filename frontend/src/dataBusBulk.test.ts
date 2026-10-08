// UX-15: bulk wiring in Data Bus Connections, on the real component library
// and the Battery Electric Car (src/data/demoProject.json).
import { afterAll, beforeEach, describe, expect, it, vi } from "vitest";
import libraryJson from "./data/componentLibrary.json";
import demo from "./data/demoProject.json";
import type { ComponentDef, ElementInstance, PortDef, Project } from "./types";

// the store reaches the engine only through api.ts; nothing here may
vi.mock("./api", () => ({ validateProject: vi.fn(() => Promise.resolve([])) }));

const { applyBulkLinks, busIndex, inputGroups, keyOf, normName, planConnectToAll, planMatchingNames } = await import(
  "./dataBusBulk"
);
const { stopRechecks, useProjectStore } = await import("./store/projectStore");

const library = libraryJson as unknown as { components: ComponentDef[] };
const libraryById = Object.fromEntries(library.components.map((c) => [c.id, c]));
const bev = () => structuredClone(demo) as unknown as Project;
const unwired = (): Project => ({ ...bev(), dataBusConnections: [] });
const elements = (p: Project) => p.systems.flatMap((s) => s.elements);
/** a link as "from → to", by part and port name */
const text = (l: { from: { name: string }; to: { name: string } }) => `${l.from.name} → ${l.to.name}`;

function script(id: string, label: string, ports: [string, "input" | "output", string?][]): ElementInstance {
  const dynamicPorts: PortDef[] = ports.map(([name, direction, unitGroup], i) => ({
    id: `p${i}`,
    name,
    direction,
    kind: "signal",
    unitGroup,
  }));
  return { id, componentDefId: "signal.script", label, position: { x: 0, y: 0 }, parameterOverrides: {}, dynamicPorts };
}

afterAll(() => stopRechecks());

describe("names", () => {
  it("compares names without case, spaces or underscores", () => {
    expect(normName("Brake Command")).toBe(normName("brake_command"));
    expect(normName("BrakeCommand")).toBe("brakecommand");
    expect(normName("H₂ Mass")).toBe(normName("h₂-mass"));
  });
});

describe("busIndex", () => {
  it("lists every signal port and what feeds each input", () => {
    const index = busIndex(bev(), libraryById);
    const brakeFl = index.inputs.find((e) => e.name === "Brake FL · Brake Command")!;
    expect(index.feeds.get(keyOf(brakeFl))!.map((f) => f.from.name)).toEqual(["Driver · Brake Command"]);
    expect(index.outputs.every((e) => e.port.direction === "output")).toBe(true);
    // the monitors' own ports are inputs too
    expect(index.inputs.map((e) => e.name)).toContain("BMS Monitor · soc");
  });

  it("counts a signal wire drawn on the diagram as a source, and a link between two inputs as none", () => {
    const p = unwired();
    p.systems[0].connections.push({
      id: "w1",
      sourceElementId: "el-driver",
      sourcePortId: "sig_brake_cmd",
      targetElementId: "el-brake-fl",
      targetPortId: "sig_demand_in",
    });
    p.dataBusConnections.push({ id: "bad", element1Id: "el-brake-fr", port1Id: "sig_demand_in", element2Id: "el-driver", port2Id: "sig_speed_in" });
    const index = busIndex(p, libraryById);
    expect(index.feeds.get("el-brake-fl:sig_demand_in")).toEqual([expect.objectContaining({ linkId: null })]);
    expect(index.feeds.get("el-brake-fr:sig_demand_in")).toBeUndefined();
  });
});

describe("connect to all", () => {
  const brakes = (p: Project) => {
    const index = busIndex(p, libraryById);
    const group = inputGroups(index, libraryById).find((g) => g.defId === "mech.brake")!;
    const source = index.outputs.find((e) => e.name === "Driver · Brake Command")!;
    return { index, group, source };
  };

  it("groups the inputs that several parts of one type have", () => {
    const groups = inputGroups(busIndex(bev(), libraryById), libraryById);
    expect(groups.map((g) => `${g.typeName} · ${g.portName} (${g.inputs.length})`)).toEqual(["Brake · Brake Command (4)"]);
  });

  it("links one output to the input of every Brake", () => {
    const { index, group, source } = brakes(unwired());
    const plan = planConnectToAll(index, source, group);
    expect(plan.links.map(text)).toEqual([
      "Driver · Brake Command → Brake FL · Brake Command",
      "Driver · Brake Command → Brake FR · Brake Command",
      "Driver · Brake Command → Brake RL · Brake Command",
      "Driver · Brake Command → Brake RR · Brake Command",
    ]);
    expect(plan.skipped).toEqual([]);
  });

  it("leaves inputs that have this source, and keeps another source unless asked to replace it", () => {
    const p = bev();
    // Brake RR takes another signal
    const rr = p.dataBusConnections.find((d) => d.element2Id === "el-brake-rr")!;
    rr.element1Id = "el-driver";
    rr.port1Id = "sig_traction_cmd";
    const { index, group, source } = brakes(p);
    const keep = planConnectToAll(index, source, group);
    expect(keep.links).toEqual([]);
    expect(keep.skipped.map((s) => `${s.to.label}: ${s.why}`)).toEqual([
      "Brake FL: already connected to it",
      "Brake FR: already connected to it",
      "Brake RL: already connected to it",
      "Brake RR: keeps its source, Driver · Traction Command",
    ]);
    const replace = planConnectToAll(index, source, group, { replace: true });
    expect(replace.links.map((l) => [l.to.label, l.replaces])).toEqual([["Brake RR", [rr.id]]]);
  });

  it("never replaces a wire drawn on the diagram, nor links a part to itself", () => {
    const p = unwired();
    p.systems[0].connections.push({
      id: "w1",
      sourceElementId: "el-driver",
      sourcePortId: "sig_traction_cmd",
      targetElementId: "el-brake-fl",
      targetPortId: "sig_demand_in",
    });
    const { index, group } = brakes(p);
    const own = index.outputs.find((e) => e.name === "Brake FR · Brake Torque")!;
    const plan = planConnectToAll(index, own, group, { replace: true });
    expect(plan.skipped.map((s) => `${s.to.label}: ${s.why}`)).toEqual([
      "Brake FL: wired on the diagram to Driver · Traction Command (a wire on the diagram)",
      "Brake FR: it is on the source's own part",
    ]);
    expect(plan.links.map((l) => l.to.label)).toEqual(["Brake RL", "Brake RR"]);
  });
});

describe("connect by matching names", () => {
  it("rebuilds the Battery Electric Car's links whose names match", () => {
    const plan = planMatchingNames(busIndex(unwired(), libraryById));
    expect(plan.links.map(text).sort()).toEqual([
      "Driver · Brake Command → Brake FL · Brake Command",
      "Driver · Brake Command → Brake FR · Brake Command",
      "Driver · Brake Command → Brake RL · Brake Command",
      "Driver · Brake Command → Brake RR · Brake Command",
      "Driver · Traction Command → E-Motor · Traction Command",
      "HV Battery Pack · Current → BMS Monitor · current",
      "HV Battery Pack · SOC → BMS Monitor · soc",
      "Vehicle Task · Target Speed → Driver · Target Speed",
    ]);
    expect(plan.skipped).toEqual([]);
  });

  it("leaves inputs that have a source alone", () => {
    expect(planMatchingNames(busIndex(bev(), libraryById)).links).toEqual([]);
  });

  it("pairs a Script's ports by part and port name, and lists an input that two outputs match", () => {
    const p = unwired();
    p.systems[0].elements.push(
      script("el-s1", "Strategy", [
        ["vehicle_speed", "input", "Velocity"],
        ["Battery SOC", "input"],
        ["Brake Command", "output"],
        ["mechanical power", "input", "Power"],
      ]),
    );
    elements(p).find((e) => e.id === "el-battery")!.label = "Battery";
    const plan = planMatchingNames(busIndex(p, libraryById));
    const links = plan.links.map(text);
    expect(links).toContain("Vehicle · Vehicle Speed → Strategy · vehicle_speed");
    expect(links).toContain("Battery · SOC → Strategy · Battery SOC");
    // the brakes' input matches the Driver's output and the Script's
    expect(links.filter((l) => l.includes("Brake Command"))).toEqual([]);
    expect(plan.skipped.map((s) => `${s.to.name}: ${s.why}`)).toContain(
      "Brake FL · Brake Command: 2 outputs match: Driver · Brake Command, Strategy · Brake Command",
    );
    // E-Motor's Mechanical Power is the only one in the BEV: no ambiguity
    expect(links).toContain("E-Motor · Mechanical Power → Strategy · mechanical power");
  });

  it("never pairs ports of different units, nor a part with itself", () => {
    const p = unwired();
    p.systems[0].elements.push(
      script("el-s1", "Strategy", [
        ["SOC", "input", "Velocity"],
        ["loop", "input"],
        ["loop", "output"],
      ]),
    );
    const plan = planMatchingNames(busIndex(p, libraryById));
    expect(plan.links.map(text).filter((l) => l.includes("Strategy"))).toEqual([]);
    expect(plan.skipped.map((s) => `${s.to.name}: ${s.why}`)).toContain(
      "Strategy · SOC: HV Battery Pack · SOC has another unit (Percent, not Velocity)",
    );
  });

  it("with a part picked, makes only the links to or from it", () => {
    const plan = planMatchingNames(busIndex(unwired(), libraryById), { elementId: "el-driver" });
    expect(plan.links.map(text).sort()).toEqual([
      "Driver · Brake Command → Brake FL · Brake Command",
      "Driver · Brake Command → Brake FR · Brake Command",
      "Driver · Brake Command → Brake RL · Brake Command",
      "Driver · Brake Command → Brake RR · Brake Command",
      "Driver · Traction Command → E-Motor · Traction Command",
      "Vehicle Task · Target Speed → Driver · Target Speed",
    ]);
  });
});

describe("applyBulkLinks", () => {
  const store = () => useProjectStore.getState();
  beforeEach(() => {
    useProjectStore.setState({ project: unwired(), libraryById, past: [], future: [], messages: [] });
  });

  it("makes every link as one undo step", () => {
    const plan = planMatchingNames(busIndex(store().project!, libraryById));
    expect(applyBulkLinks(plan.links, "matching names")).toBe(8);
    expect(store().project!.dataBusConnections).toHaveLength(8);
    expect(store().past).toHaveLength(1);
    expect(store().dirty).toBe(true);
    expect(store().messages.at(-1)?.text).toBe(
      "Data bus: 8 links connected (matching names). One Undo takes them all back.",
    );
    store().undo();
    expect(store().project!.dataBusConnections).toEqual([]);
    store().redo();
    expect(store().project!.dataBusConnections).toHaveLength(8);
  });

  it("replaces the links it was planned to, in the same step", () => {
    useProjectStore.setState({ project: bev() });
    const p = store().project!;
    const rr = p.dataBusConnections.find((d) => d.element2Id === "el-brake-rr")!;
    const index = busIndex(p, libraryById);
    const group = inputGroups(index, libraryById)[0];
    const source = index.outputs.find((e) => e.name === "Driver · Traction Command")!;
    const plan = planConnectToAll(index, source, group, { replace: true });
    expect(applyBulkLinks(plan.links, "every Brake")).toBe(4);
    const links = store().project!.dataBusConnections;
    expect(links).toHaveLength(13);
    expect(links.some((d) => d.id === rr.id)).toBe(false);
    expect(links.filter((d) => d.port1Id === "sig_traction_cmd" && d.element2Id.startsWith("el-brake"))).toHaveLength(4);
    store().undo();
    expect(store().project!.dataBusConnections).toEqual(bev().dataBusConnections);
  });

  it("leaves out a link the project no longer allows", () => {
    const plan = planMatchingNames(busIndex(store().project!, libraryById));
    // meanwhile Brake FL got a source of its own, and the E-Motor is gone
    const p = structuredClone(store().project!);
    p.dataBusConnections.push({ id: "x", element1Id: "el-vehicle", port1Id: "sig_speed", element2Id: "el-brake-fl", port2Id: "sig_demand_in" });
    p.systems[0].elements = p.systems[0].elements.filter((e) => e.id !== "el-motor");
    useProjectStore.setState({ project: p });
    expect(applyBulkLinks(plan.links, "matching names")).toBe(6);
    expect(applyBulkLinks([], "nothing")).toBe(0);
    expect(store().past).toHaveLength(1);
  });
});
