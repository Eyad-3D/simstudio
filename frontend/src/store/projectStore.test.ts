import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MockedObject } from "vitest";
import libraryJson from "../data/componentLibrary.json";
import type { ComponentDef, DataCheck, ElementInstance, Project } from "../types";

// The store talks to the engine only through api.ts; every call is mocked so
// these tests never touch the network.
vi.mock("../api", () => ({
  fetchLibrary: vi.fn(),
  fetchDemoProject: vi.fn(),
  fetchProject: vi.fn(),
  listProjects: vi.fn(),
  saveProject: vi.fn(),
  validateProject: vi.fn(),
  runSimulation: vi.fn(),
  runSimulationLive: vi.fn(),
}));

const library = libraryJson as unknown as {
  components: ComponentDef[];
  unitGroups: Record<string, string>;
};
const defName = (id: string) => library.components.find((c) => c.id === id)!.name;

let api: MockedObject<typeof import("../api")>;
let persist: typeof import("../persist");
let useProjectStore: typeof import("./projectStore").useProjectStore;
const store = () => useProjectStore.getState();

function el(id: string, componentDefId: string, label: string): ElementInstance {
  return { id, componentDefId, label, position: { x: 0, y: 0 }, parameterOverrides: {} };
}

/** A small hand-built model on the real component library: battery → bus → motor. */
function fixture(overrides: Partial<Project> = {}): Project {
  return {
    id: "fixture",
    name: "Fixture",
    systems: [
      {
        id: "sys-root",
        name: "Fixture",
        parentId: null,
        elements: [
          el("el-bat", "battery.generic", "Battery"),
          el("el-node", "electric.node", "Bus"),
          el("el-motor", "motor.emotor", "Motor"),
          el("el-shaft", "mech.shaft", "Shaft"),
          el("el-const", "signal.constant", "Demand"),
        ],
        connections: [
          {
            id: "c-1",
            sourceElementId: "el-bat",
            sourcePortId: "pos",
            targetElementId: "el-node",
            targetPortId: "t1",
          },
        ],
      },
    ],
    dataBusConnections: [],
    cases: [{ id: "case-1", name: "Case 1", duration: 10, timeStep: 1 }],
    ...overrides,
  };
}

const rootSystem = () => store().project!.systems.find((s) => s.parentId === null)!;
const allElementIds = () => store().project!.systems.flatMap((s) => s.elements.map((e) => e.id));
const findElement = (id: string) =>
  store().project!.systems.flatMap((s) => s.elements).find((e) => e.id === id);
const messages = () => store().messages.map((m) => `${m.level}: ${m.text}`);

beforeEach(async () => {
  // fresh module instances per test: the store keeps module-level state
  // (undo coalescing, the active run) that must not leak between tests
  vi.resetModules();
  localStorage.clear();
  api = vi.mocked(await import("../api"));
  persist = await import("../persist");
  useProjectStore = (await import("./projectStore")).useProjectStore;

  api.fetchLibrary.mockResolvedValue({
    components: library.components,
    unitGroups: library.unitGroups,
    offline: false,
  });
  api.fetchDemoProject.mockResolvedValue({ project: fixture(), offline: false });
  api.saveProject.mockResolvedValue({ saved: "fixture" });
});

describe("start-up", () => {
  it("opens the example project, clean, when there is no recovery draft", async () => {
    await store().init();
    const s = store();
    expect(s.loaded).toBe(true);
    expect(s.offline).toBe(false);
    expect(s.library).toHaveLength(library.components.length);
    expect(s.project?.id).toBe("fixture");
    expect(s.activeSystemId).toBe("sys-root");
    expect(s.activeCaseId).toBe("case-1");
    expect(s.dirty).toBe(false);
    expect(messages()).toContain("info: Project 'Fixture' opened.");
  });

  it("restores an unsaved draft and keeps it flagged as unsaved", async () => {
    persist.saveDraft(fixture({ name: "Edited" }));
    await store().init();
    expect(store().project?.name).toBe("Edited");
    expect(store().dirty).toBe(true);
    expect(api.fetchProject).not.toHaveBeenCalled();
    expect(messages().some((m) => m.includes("Restored your unsaved draft"))).toBe(true);
  });

  it("reopens a clean draft's project from disk and does not report unsaved work", async () => {
    persist.saveDraft(fixture({ id: "other", name: "Kept copy" }), true);
    api.fetchProject.mockResolvedValue(fixture({ id: "other", name: "On disk" }));
    await store().init();
    expect(api.fetchProject).toHaveBeenCalledWith("other");
    expect(store().project?.name).toBe("On disk");
    expect(store().dirty).toBe(false);
    expect(messages().some((m) => m.includes("Restored"))).toBe(false);
  });

  it("falls back to the clean copy when that project is gone from disk", async () => {
    persist.saveDraft(fixture({ id: "gone", name: "Kept copy" }), true);
    api.fetchProject.mockRejectedValue(new Error("404 Project 'gone' not found"));
    await store().init();
    expect(store().project?.name).toBe("Kept copy");
    expect(store().dirty).toBe(false);
  });

  it("offline: runs from bundled data, warns, and does not reach for the disk", async () => {
    api.fetchLibrary.mockResolvedValue({
      components: library.components,
      unitGroups: library.unitGroups,
      offline: true,
    });
    persist.saveDraft(fixture({ name: "Kept copy" }), true);
    await store().init();
    expect(store().offline).toBe(true);
    expect(api.fetchProject).not.toHaveBeenCalled();
    expect(store().project?.name).toBe("Kept copy");
    expect(messages().some((m) => m.startsWith("warning: Backend not reachable"))).toBe(true);
  });
});

describe("dirty tracking and save", () => {
  it("an edit marks the project unsaved and Save clears the flag", async () => {
    await store().init();
    store().renameElement("el-bat", "Pack");
    expect(store().dirty).toBe(true);

    await store().saveRemote();
    expect(api.saveProject).toHaveBeenCalledTimes(1);
    const saved = api.saveProject.mock.calls[0][0];
    expect(saved.systems[0].elements.find((e) => e.id === "el-bat")?.label).toBe("Pack");
    expect(store().dirty).toBe(false);
    expect(messages()).toContain("info: Project 'Fixture' saved to the server.");
  });

  it("a failed save keeps the project flagged as unsaved", async () => {
    await store().init();
    store().renameElement("el-bat", "Pack");
    api.saveProject.mockRejectedValue(new Error("500 disk full"));
    await store().saveRemote();
    expect(store().dirty).toBe(true);
    expect(messages().some((m) => m.startsWith("error: Save failed: 500 disk full"))).toBe(true);
  });

  it("selection and case switching do not count as edits", async () => {
    await store().init();
    store().select("el-bat");
    store().setActiveCase("case-1");
    expect(store().dirty).toBe(false);
    expect(store().past).toHaveLength(0);
  });
});

describe("project lifecycle", () => {
  it("Open replaces the project and resets history, selection, runs and checks", async () => {
    await store().init();
    store().addElement("signal.constant", { x: 10, y: 10 });
    useProjectStore.setState({ dataChecks: [] });
    api.fetchProject.mockResolvedValue(
      fixture({
        id: "other",
        name: "Other",
        systems: [{ id: "sys-other", name: "Other", parentId: null, elements: [], connections: [] }],
      }),
    );

    await store().openProject("other");
    const s = store();
    expect(s.project?.id).toBe("other");
    expect(s.activeSystemId).toBe("sys-other");
    expect(s.selectedElementId).toBeNull();
    expect(s.past).toHaveLength(0);
    expect(s.future).toHaveLength(0);
    expect(s.runs).toHaveLength(0);
    expect(s.dataChecks).toBeNull();
    expect(s.dirty).toBe(false);
  });

  it("a failed Open keeps the current project and its unsaved edits", async () => {
    await store().init();
    store().renameElement("el-bat", "Pack");
    api.fetchProject.mockRejectedValue(new Error("404 Project 'nope' not found"));
    await store().openProject("nope");
    expect(store().project?.id).toBe("fixture");
    expect(findElement("el-bat")?.label).toBe("Pack");
    expect(store().dirty).toBe(true);
    expect(messages().some((m) => m.startsWith("error: Failed to open project"))).toBe(true);
  });

  it("New starts an empty, clean project with one case", async () => {
    await store().init();
    store().renameElement("el-bat", "Pack");
    store().newProject();
    const s = store();
    expect(s.project?.name).toBe("New Project");
    expect(s.project?.systems).toHaveLength(1);
    expect(rootSystem().elements).toHaveLength(0);
    expect(s.project?.cases).toHaveLength(1);
    expect(s.activeSystemId).toBe(rootSystem().id);
    expect(s.activeCaseId).toBe(s.project?.cases[0].id);
    expect(s.past).toHaveLength(0);
    expect(s.dirty).toBe(false);
  });

  it("Import loads a project file as unsaved work and fills in missing lists", async () => {
    await store().init();
    const file = fixture({ id: "imported", name: "Imported" }) as Partial<Project>;
    delete file.dataBusConnections;
    delete file.cases;
    store().importProject(JSON.stringify(file));
    expect(store().project?.id).toBe("imported");
    expect(store().project?.dataBusConnections).toEqual([]);
    expect(store().project?.cases).toEqual([]);
    expect(store().dirty).toBe(true);
  });

  it("Import rejects files that are not projects and keeps the current one", async () => {
    await store().init();
    store().importProject("{broken");
    store().importProject(JSON.stringify({ name: "no id or systems" }));
    expect(store().project?.id).toBe("fixture");
    expect(store().dirty).toBe(false);
    expect(messages().filter((m) => m.startsWith("error: Import failed"))).toHaveLength(2);
  });
});

describe("undo / redo", () => {
  it("steps back and forward through edits", async () => {
    await store().init();
    const before = allElementIds().length;
    store().addElement("signal.constant", { x: 10, y: 10 });
    expect(allElementIds()).toHaveLength(before + 1);

    store().undo();
    expect(allElementIds()).toHaveLength(before);
    expect(store().future).toHaveLength(1);
    expect(store().dirty).toBe(true);

    store().redo();
    expect(allElementIds()).toHaveLength(before + 1);
    expect(store().future).toHaveLength(0);
  });

  it("a new edit after undo discards the redo branch", async () => {
    await store().init();
    store().renameElement("el-bat", "A");
    store().undo();
    store().renameElement("el-node", "B");
    expect(store().future).toHaveLength(0);
    store().redo();
    expect(findElement("el-bat")?.label).toBe("Battery");
  });

  it("undo and redo with no history do nothing", async () => {
    await store().init();
    const project = store().project;
    store().undo();
    store().redo();
    expect(store().project).toBe(project);
    expect(store().dirty).toBe(false);
  });

  it("rapid edits to one field share an undo step; a pause starts a new one", async () => {
    await store().init();
    let t = 1_000_000;
    vi.spyOn(Date, "now").mockImplementation(() => t);
    store().setParameter("el-shaft", "efficiency_pct", 9);
    t += 100;
    store().setParameter("el-shaft", "efficiency_pct", 95);
    expect(store().past).toHaveLength(1);
    t += 5_000;
    store().setParameter("el-shaft", "efficiency_pct", 96);
    expect(store().past).toHaveLength(2);

    store().undo();
    expect(findElement("el-shaft")?.parameterOverrides.efficiency_pct).toBe(95);
    store().undo();
    expect(findElement("el-shaft")?.parameterOverrides).toEqual({});
  });

  it("a drag is one undo step (beginHistory + moves)", async () => {
    await store().init();
    store().beginHistory();
    for (const x of [5, 10, 15]) store().moveElement("el-bat", { x, y: 0 });
    expect(store().past).toHaveLength(1);
    expect(findElement("el-bat")?.position).toEqual({ x: 15, y: 0 });
    store().undo();
    expect(findElement("el-bat")?.position).toEqual({ x: 0, y: 0 });
  });

  it("keeps at most 50 undo steps", async () => {
    await store().init();
    for (let i = 0; i < 60; i++) store().addCase();
    expect(store().past).toHaveLength(50);
    for (let i = 0; i < 60; i++) store().undo();
    // the oldest 10 steps were dropped, so undo stops 10 cases short
    expect(store().project?.cases).toHaveLength(11);
  });

  it("keeps at most 50 drag steps too", async () => {
    await store().init();
    for (let x = 1; x <= 60; x++) {
      store().beginHistory();
      store().moveElement("el-bat", { x, y: 0 });
    }
    expect(store().past).toHaveLength(50);
    for (let i = 0; i < 60; i++) store().undo();
    // the oldest 10 drags were dropped
    expect(findElement("el-bat")?.position).toEqual({ x: 10, y: 0 });
  });
});

describe("elements and wiring", () => {
  it("adds an element with a numbered label and selects it", async () => {
    await store().init();
    store().addElement("battery.generic", { x: 40, y: 60 });
    const added = rootSystem().elements.at(-1)!;
    expect(added.componentDefId).toBe("battery.generic");
    expect(added.label).toBe(`${defName("battery.generic")} 2`);
    expect(added.position).toEqual({ x: 40, y: 60 });
    expect(store().selectedElementId).toBe(added.id);
    expect(store().dirty).toBe(true);
  });

  it("ignores unknown component types", async () => {
    await store().init();
    store().addElement("no.such.component", { x: 0, y: 0 });
    expect(allElementIds()).toHaveLength(5);
    expect(store().dirty).toBe(false);
  });

  it("a System container gets its own sub-system, and deleting it removes the whole subtree", async () => {
    await store().init();
    store().addElement("container.system", { x: 0, y: 0 });
    const container = rootSystem().elements.at(-1)!;
    expect(container.isSubSystem).toBe(true);
    const sub = store().project!.systems.find((s) => s.id === container.subSystemId);
    expect(sub?.parentId).toBe("sys-root");

    store().setActiveSystem(sub!.id);
    store().addElement("signal.constant", { x: 0, y: 0 });
    const inner = store().selectedElementId!;
    store().setActiveSystem("sys-root");

    store().removeElements([container.id]);
    expect(store().project!.systems).toHaveLength(1);
    expect(allElementIds()).not.toContain(container.id);
    expect(allElementIds()).not.toContain(inner);
  });

  it("deleting an element drops the wires and data-bus links on it and clears the selection", async () => {
    await store().init();
    store().addConnection("el-const", "sig_out", "el-motor", "sig_demand_in");
    expect(store().project!.dataBusConnections).toHaveLength(1);

    store().select("el-bat");
    store().removeElements(["el-bat", "el-const"]);
    expect(allElementIds()).not.toContain("el-bat");
    expect(rootSystem().connections).toHaveLength(0);
    expect(store().project!.dataBusConnections).toHaveLength(0);
    expect(store().selectedElementId).toBeNull();

    store().undo();
    expect(allElementIds()).toContain("el-bat");
    expect(rootSystem().connections.map((c) => c.id)).toEqual(["c-1"]);
  });

  it("wires two compatible ports", async () => {
    await store().init();
    store().addConnection("el-node", "t2", "el-motor", "pos");
    const wires = rootSystem().connections;
    expect(wires).toHaveLength(2);
    expect(wires[1]).toMatchObject({
      sourceElementId: "el-node",
      sourcePortId: "t2",
      targetElementId: "el-motor",
      targetPortId: "pos",
    });
    expect(store().dirty).toBe(true);
  });

  it("refuses to wire ports of different kinds and says why", async () => {
    await store().init();
    store().addConnection("el-motor", "shaft", "el-node", "t2");
    store().addConnection("el-const", "sig_out", "el-node", "t2");
    expect(rootSystem().connections).toHaveLength(1);
    expect(store().project!.dataBusConnections).toHaveLength(0);
    expect(store().dirty).toBe(false);
    expect(messages().filter((m) => m.startsWith("error:"))).toHaveLength(2);
  });

  it("ignores a duplicate wire drawn in either direction", async () => {
    await store().init();
    store().addConnection("el-bat", "pos", "el-node", "t1");
    store().addConnection("el-node", "t1", "el-bat", "pos");
    expect(rootSystem().connections).toHaveLength(1);
    expect(store().past).toHaveLength(0);
  });

  it("stores signal wiring as a data-bus link", async () => {
    await store().init();
    store().addConnection("el-const", "sig_out", "el-motor", "sig_demand_in");
    expect(rootSystem().connections).toHaveLength(1);
    expect(store().project!.dataBusConnections).toEqual([
      expect.objectContaining({
        element1Id: "el-const",
        port1Id: "sig_out",
        element2Id: "el-motor",
        port2Id: "sig_demand_in",
      }),
    ]);
  });

  it("removes wires and data-bus links by id, undoably", async () => {
    await store().init();
    store().addConnection("el-const", "sig_out", "el-motor", "sig_demand_in");
    const dbc = store().project!.dataBusConnections[0].id;
    store().removeConnections(["c-1", dbc]);
    expect(rootSystem().connections).toHaveLength(0);
    expect(store().project!.dataBusConnections).toHaveLength(0);
    store().undo();
    expect(rootSystem().connections).toHaveLength(1);
    expect(store().project!.dataBusConnections).toHaveLength(1);
  });
});

describe("data checks gate", () => {
  const error: DataCheck = { level: "error", text: "Port 'pos' is not connected." };

  it("Data Checks stores the engine's findings", async () => {
    await store().init();
    api.validateProject.mockResolvedValue([error]);
    const checks = await store().runDataChecks();
    expect(checks).toEqual([error]);
    expect(store().dataChecks).toEqual([error]);
    expect(store().checking).toBe(false);
    expect(messages()).toContain("error: Data checks: 1 error(s), 0 warning(s).");
  });

  it("an error-level check blocks the run before it reaches the engine", async () => {
    await store().init();
    api.validateProject.mockResolvedValue([error]);
    await store().run();
    expect(api.runSimulationLive).not.toHaveBeenCalled();
    expect(store().running).toBe(false);
    expect(messages()).toContain("error: Run blocked — fix 1 data-check error(s) first.");
  });
});

describe("run history", () => {
  /** The engine passes the checks and finishes every run at once. */
  function engineFinishesRuns() {
    api.validateProject.mockResolvedValue([]);
    api.runSimulationLive.mockImplementation((_project, caseId) => ({
      setParam: vi.fn(),
      cancel: vi.fn(),
      done: Promise.resolve({ caseId, status: "success", messages: [], channels: [], summary: [] }),
    }));
  }

  /** Run the active case `n` times; the ids of the runs, oldest first. */
  async function runTimes(n: number): Promise<string[]> {
    const ids: string[] = [];
    for (let i = 0; i < n; i++) {
      await store().run();
      ids.push(store().activeRunId!);
    }
    return ids;
  }

  it("a finished run is stored newest first and becomes the active run", async () => {
    await store().init();
    engineFinishesRuns();
    const [first, second] = await runTimes(2);
    const s = store();
    expect(api.runSimulationLive).toHaveBeenCalledTimes(2);
    expect(s.runs.map((r) => r.id)).toEqual([second, first]);
    expect(s.activeRunId).toBe(second);
    expect(s.runs[0]).toMatchObject({ caseId: "case-1", caseName: "Case 1", status: "success" });
    expect(s.running).toBe(false);
  });

  it("keeps only the 20 newest runs", async () => {
    await store().init();
    engineFinishesRuns();
    const ids = await runTimes(22);
    expect(store().runs.map((r) => r.id)).toEqual(ids.slice(2).reverse());
  });

  it("removing the active run falls back to the newest one left; Clear empties the history", async () => {
    await store().init();
    engineFinishesRuns();
    const [oldest, middle, newest] = await runTimes(3);
    store().setActiveRun(middle);
    store().removeRun(oldest);
    expect(store().activeRunId).toBe(middle); // another run's removal leaves it be
    store().removeRun(middle);
    expect(store().runs.map((r) => r.id)).toEqual([newest]);
    expect(store().activeRunId).toBe(newest);
    store().clearRuns();
    expect(store().runs).toEqual([]);
    expect(store().activeRunId).toBeNull();
  });
});
