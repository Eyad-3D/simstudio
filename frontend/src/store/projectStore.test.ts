import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MockedObject } from "vitest";
import libraryJson from "../data/componentLibrary.json";
import type {
  ComponentDef,
  DataCheck,
  ElementInstance,
  Project,
  SimResult,
  SimRun,
  StoredRunInfo,
} from "../types";

// The store talks to the engine only through api.ts; every call is mocked so
// these tests never touch the network.
vi.mock("../api", () => ({
  fetchLibrary: vi.fn(),
  fetchVersion: vi.fn(),
  fetchDemoProject: vi.fn(),
  fetchProject: vi.fn(),
  listProjects: vi.fn(),
  saveProject: vi.fn(),
  validateProject: vi.fn(),
  runSimulation: vi.fn(),
  runSimulationLive: vi.fn(),
  listRuns: vi.fn(),
  fetchRun: vi.fn(),
  storeRun: vi.fn(),
  deleteRun: vi.fn(),
  deleteRuns: vi.fn(),
}));

const library = libraryJson as unknown as {
  components: ComponentDef[];
  unitGroups: Record<string, string>;
};
const defName = (id: string) => library.components.find((c) => c.id === id)!.name;

let api: MockedObject<typeof import("../api")>;
let persist: typeof import("../persist");
let useProjectStore: typeof import("./projectStore").useProjectStore;
let confirmReplaceProject: typeof import("./projectStore").confirmReplaceProject;
let useUIStore: typeof import("./uiStore").useUIStore;
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
/** Let the store's background work (reading stored runs) finish. */
const settled = () => new Promise((resolve) => setTimeout(resolve, 0));

/** The engine's run store as the api mocks see it: project id → run id → run. */
let disk: Map<string, Map<string, SimRun>>;
const folder = (projectId: string) => disk.get(projectId) ?? disk.set(projectId, new Map()).get(projectId)!;

/** What the engine lists for a stored run: the run without its channel data. */
function info({ result, status, ...run }: SimRun): StoredRunInfo {
  return { ...run, status: status as StoredRunInfo["status"], summary: result.summary, bytes: 1000 };
}

/** The engine passes the checks and finishes every run at once. */
function engineFinishesRuns() {
  api.validateProject.mockResolvedValue([]);
  api.runSimulationLive.mockImplementation((_project, caseId) => ({
    setParam: vi.fn(),
    cancel: vi.fn(),
    done: Promise.resolve({ caseId, status: "success", messages: [], channels: [], summary: [] }),
  }));
}

beforeEach(async () => {
  // fresh module instances per test: the store keeps module-level state
  // (undo coalescing, the active run) that must not leak between tests
  vi.resetModules();
  localStorage.clear();
  api = vi.mocked(await import("../api"));
  persist = await import("../persist");
  ({ useProjectStore, confirmReplaceProject } = await import("./projectStore"));
  useUIStore = (await import("./uiStore")).useUIStore;

  api.fetchLibrary.mockResolvedValue({
    components: library.components,
    unitGroups: library.unitGroups,
    offline: false,
  });
  api.fetchVersion.mockResolvedValue("0.1.0");
  api.fetchDemoProject.mockResolvedValue({ project: fixture(), offline: false });
  api.saveProject.mockResolvedValue({ saved: "fixture" });

  disk = new Map();
  api.listRuns.mockImplementation(async (projectId) =>
    [...folder(projectId).values()].sort((a, b) => b.startedAt - a.startedAt).map(info),
  );
  api.fetchRun.mockImplementation(async (projectId, runId) => {
    const run = folder(projectId).get(runId);
    if (!run) throw new Error(`404 Run '${runId}' not found`);
    return structuredClone(run);
  });
  api.storeRun.mockImplementation(async (projectId, run) => {
    folder(projectId).set(run.id, structuredClone(run));
    return { saved: run.id, stored: folder(projectId).size, bytes: 0, budget: 500 * 2 ** 20, pruned: [] };
  });
  api.deleteRun.mockImplementation(async (projectId, runId) => {
    if (!folder(projectId).delete(runId)) throw new Error(`404 Run '${runId}' not found`);
    return { deleted: runId, stored: folder(projectId).size };
  });
  api.deleteRuns.mockImplementation(async (projectId) => {
    const deleted = folder(projectId).size;
    folder(projectId).clear();
    return { deleted, stored: 0 };
  });
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

  it("an edit made while a save is in flight stays unsaved", async () => {
    await store().init();
    store().renameElement("el-bat", "Pack");
    let finish!: (value: { saved: string }) => void;
    api.saveProject.mockReturnValue(new Promise((resolve) => (finish = resolve)));
    const saving = store().saveRemote();
    await Promise.resolve();
    store().renameElement("el-bat", "Pack 2"); // e.g. while the 409 dialog is open
    finish({ saved: "fixture" });
    await saving;
    expect(api.saveProject.mock.calls[0][0].systems[0].elements[0].label).toBe("Pack");
    expect(store().dirty).toBe(true);
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

  it("a copy opens as a new, unsaved project with an id of its own", async () => {
    await store().init();
    const source = fixture({ name: "Old version", cases: [{ id: "case-9", name: "Old case", duration: 5, timeStep: 1 }] });
    store().openAsCopy(source, "Old version (copy)", "Opened a copy.");
    const s = store();
    expect(s.project?.id).not.toBe("fixture");
    expect(s.project?.name).toBe("Old version (copy)");
    expect(s.project?.systems).toEqual(source.systems);
    expect(s.revision).toBeNull(); // Save creates a file and never replaces one
    expect(s.dirty).toBe(true);
    expect(s.activeCaseId).toBe("case-9");
    expect(s.runs).toEqual([]);
    expect(s.past).toEqual([]);
    expect(messages()).toContain("info: Opened a copy.");

    await store().saveRemote();
    expect(api.saveProject).toHaveBeenCalledWith(expect.objectContaining({ id: s.project!.id }), null);
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

describe("asking before a project is replaced", () => {
  /** Answer the open Save / Don't save / Cancel dialog. */
  function answer(choice: "save" | "discard" | "cancel") {
    const dialog = useUIStore.getState().dialog;
    expect(dialog?.title).toBe("Save changes to 'Fixture'?");
    dialog!.resolve(choice === "save" ? true : choice === "discard" ? "alt" : null);
  }

  it("goes ahead without asking when nothing is unsaved", async () => {
    await store().init();
    expect(await confirmReplaceProject("Opening 'Other'")).toBe(true);
    expect(useUIStore.getState().dialog).toBeNull();
  });

  it("Don't save goes ahead, Cancel does not, and neither saves", async () => {
    await store().init();
    store().renameElement("el-bat", "Pack");
    const discard = confirmReplaceProject("Opening 'Other'");
    answer("discard");
    expect(await discard).toBe(true);
    const cancel = confirmReplaceProject("Opening 'Other'");
    answer("cancel");
    expect(await cancel).toBe(false);
    expect(api.saveProject).not.toHaveBeenCalled();
  });

  it("Save goes ahead once the save worked, and not when it failed", async () => {
    await store().init();
    store().renameElement("el-bat", "Pack");
    api.saveProject.mockRejectedValueOnce(new Error("500 disk full"));
    const failed = confirmReplaceProject("Opening 'Other'");
    answer("save");
    expect(await failed).toBe(false);
    const saved = confirmReplaceProject("Opening 'Other'");
    answer("save");
    expect(await saved).toBe(true);
    expect(api.saveProject).toHaveBeenCalledTimes(2);
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

describe("stored runs", () => {
  /** A finished run of the fixture's case that started at `startedAt`. */
  function run(id: string, startedAt: number, extra: Partial<SimRun> = {}): SimRun {
    const result = { caseId: "case-1", status: "success" as const, messages: [], channels: [], summary: [] };
    return { id, caseId: "case-1", caseName: "Case 1", startedAt, status: "success", result, ...extra };
  }
  /** Put runs on the engine's disk for a project. */
  function stored(projectId: string, runs: SimRun[]) {
    for (const r of runs) folder(projectId).set(r.id, r);
  }
  /** `n` stored runs of the fixture, run-0 the oldest. */
  const history = (n: number) => Array.from({ length: n }, (_, i) => run(`run-${i}`, 1_000 + i));
  const shownIds = () => store().runs.map((r) => r.id);

  it("opening a project lists its stored runs and reads the newest 20", async () => {
    stored("fixture", history(22));
    await store().init();
    await settled();
    expect(api.listRuns).toHaveBeenCalledWith("fixture");
    expect(store().storedRunCount).toBe(22);
    expect(shownIds()).toEqual(history(22).slice(2).reverse().map((r) => r.id));
    expect(api.fetchRun).toHaveBeenCalledTimes(20);
    expect(api.fetchRun).not.toHaveBeenCalledWith("fixture", "run-0");
    expect(store().activeRunId).toBe("run-21");
    expect(store().runsLoading).toBe(false);
  });

  it("a stored run that cannot be read is left out and reported", async () => {
    stored("fixture", history(3));
    api.fetchRun.mockImplementation(async (_projectId, runId) => {
      if (runId === "run-1") throw new Error("500 corrupt file");
      return run(runId, 1_000 + Number(runId.slice(4)));
    });
    await store().init();
    await settled();
    expect(shownIds()).toEqual(["run-2", "run-0"]);
    expect(messages()).toContain("warning: 1 stored run(s) could not be read and are not listed.");
  });

  it("a project whose runs cannot be listed opens without them, with a warning", async () => {
    api.listRuns.mockRejectedValue(new Error("500 disk unreadable"));
    await store().init();
    await settled();
    expect(store().runs).toEqual([]);
    expect(store().runsLoading).toBe(false);
    expect(messages()).toContain("warning: Stored runs could not be listed: 500 disk unreadable");
  });

  it("a finished run is stored with its project", async () => {
    await store().init();
    engineFinishesRuns();
    await store().run();
    const id = store().activeRunId!;
    expect(api.storeRun).toHaveBeenCalledTimes(1);
    expect(api.storeRun.mock.calls[0][0]).toBe("fixture");
    expect(api.storeRun.mock.calls[0][1]).toMatchObject({ id, caseId: "case-1", status: "success" });
    expect(api.storeRun.mock.calls[0][1].incomplete).toBeUndefined();
    expect(store().storedRunCount).toBe(1);
    expect(folder("fixture").has(id)).toBe(true);
  });

  it("runs the disk budget deleted drop out of the history, with a warning", async () => {
    stored("fixture", history(2));
    await store().init();
    await settled();
    engineFinishesRuns();
    api.storeRun.mockResolvedValueOnce({ saved: "x", stored: 2, bytes: 0, budget: 500 * 2 ** 20, pruned: ["run-0"] });
    await store().run();
    expect(shownIds()).toEqual([store().activeRunId, "run-1"]);
    expect(store().storedRunCount).toBe(2);
    expect(messages()).toContain(
      "warning: Stored runs of this project reached the 500 MB disk budget: deleted the 1 oldest run(s).",
    );
  });

  it("a run that cannot be stored stays listed for this session, with a warning", async () => {
    await store().init();
    engineFinishesRuns();
    api.storeRun.mockRejectedValueOnce(new Error("507 disk full"));
    await store().run();
    expect(store().runs).toHaveLength(1);
    expect(store().storedRunCount).toBe(0);
    expect(messages()).toContain(
      "warning: Run 'Case 1' could not be stored on disk (507 disk full); it is kept for this session only.",
    );
  });

  it("deleting a run deletes it on disk and lists the next older stored run in its place", async () => {
    stored("fixture", history(21));
    await store().init();
    await settled();
    expect(shownIds()).not.toContain("run-0");
    await store().removeRun("run-20");
    expect(api.deleteRun).toHaveBeenCalledWith("fixture", "run-20");
    expect(folder("fixture").has("run-20")).toBe(false);
    expect(shownIds()).toHaveLength(20);
    expect(shownIds()).not.toContain("run-20");
    expect(shownIds().at(-1)).toBe("run-0");
    expect(store().activeRunId).toBe("run-19");
    expect(store().storedRunCount).toBe(20);
  });

  it("deleting a run that was never stored is not an error; other failures are", async () => {
    await store().init();
    engineFinishesRuns();
    api.storeRun.mockRejectedValueOnce(new Error("507 disk full"));
    await store().run();
    await store().removeRun(store().activeRunId!);
    expect(store().runs).toEqual([]);
    expect(messages().filter((m) => m.startsWith("error:"))).toEqual([]);

    stored("fixture", history(1));
    api.deleteRun.mockRejectedValueOnce(new Error("500 file locked"));
    await store().removeRun("run-0");
    expect(messages()).toContain("error: Could not delete the stored run: 500 file locked");
    expect(shownIds()).toEqual(["run-0"]); // still on disk, so listed again
  });

  it("Clear deletes every stored run; when that fails they are listed again", async () => {
    stored("fixture", history(3));
    await store().init();
    await settled();
    await store().clearRuns();
    expect(api.deleteRuns).toHaveBeenCalledWith("fixture");
    expect(folder("fixture").size).toBe(0);
    expect(store().runs).toEqual([]);
    expect(store().storedRunCount).toBe(0);
    expect(messages()).toContain("info: Deleted 3 stored run(s) of 'Fixture'.");

    stored("fixture", history(2));
    api.deleteRuns.mockRejectedValueOnce(new Error("500 file locked"));
    await store().clearRuns();
    expect(messages()).toContain("error: Could not delete the stored runs: 500 file locked");
    expect(shownIds()).toEqual(["run-1", "run-0"]);
  });

  it("after a reload the run shown is the newest complete run that is not a sweep point", async () => {
    const sweep = { sweepId: "sweep-1", sweepParam: "Mass", sweepUnit: "kg" };
    stored("fixture", [
      run("single-old", 1_000),
      run("single", 2_000),
      run("single-failed", 3_000, { status: "failed", incomplete: "failed" }),
      run("single-stopped", 4_000, { incomplete: "stopped at t = 12 s" }),
      run("point-1", 5_000, { ...sweep, sweepValue: 1 }),
      run("point-2", 6_000, { ...sweep, sweepValue: 2, incomplete: "stopped at t = 3 s" }),
    ]);
    await store().init();
    await settled();
    expect(shownIds()[0]).toBe("point-2");
    expect(store().activeRunId).toBe("single");
  });

  it("with no such run, the newest run is shown", async () => {
    const sweep = { sweepId: "sweep-1", sweepParam: "Mass", sweepUnit: "kg" };
    stored("fixture", [
      run("point-1", 5_000, { ...sweep, sweepValue: 1 }),
      run("point-2", 6_000, { ...sweep, sweepValue: 2, incomplete: "stopped at t = 3 s" }),
    ]);
    await store().init();
    await settled();
    expect(store().activeRunId).toBe("point-2");
  });

  it("stored runs that arrive after another project was opened are dropped", async () => {
    stored("fixture", history(2));
    let answer!: (index: StoredRunInfo[]) => void;
    api.listRuns.mockReturnValueOnce(new Promise((resolve) => (answer = resolve)));
    await store().init();
    api.fetchProject.mockResolvedValue(fixture({ id: "other", name: "Other" }));
    await store().openProject("other");
    answer(history(2).map(info));
    await settled();
    expect(store().project?.id).toBe("other");
    expect(store().runs).toEqual([]);
    expect(api.fetchRun).not.toHaveBeenCalledWith("fixture", expect.anything());
  });
});

describe("run snapshots", () => {
  /** A live run the test finishes by hand, like the engine at the end of a run. */
  function liveRun() {
    let finish!: () => void;
    const handle = { setParam: vi.fn(), cancel: vi.fn(), done: undefined as unknown as Promise<SimResult> };
    api.validateProject.mockResolvedValue([]);
    api.runSimulationLive.mockImplementation((_project, caseId) => {
      handle.done = new Promise((resolve) => {
        finish = () => resolve({ caseId, status: "success", messages: [], channels: [], summary: [] });
      });
      return handle;
    });
    return { handle, finish: () => finish() };
  }

  it("a run carries the model, case settings, app version and fingerprint it was made with", async () => {
    await store().init();
    engineFinishesRuns();
    const model = store().project!;
    await store().run();
    const snap = store().runs[0].snapshot!;
    expect(snap.project).toEqual(model);
    expect(snap.case).toEqual(model.cases[0]);
    expect(snap.appVersion).toBe("0.1.0");
    expect(snap.modelHash).toMatch(/^[0-9a-f]{64}$/);
    expect(snap.liveEdits).toEqual([]);
    expect(api.storeRun.mock.calls[0][1].snapshot).toEqual(snap); // stored on disk with it
  });

  it("live edits made while it runs are logged; the model stays as the run started", async () => {
    await store().init();
    const { handle, finish } = liveRun();
    const running = store().run();
    await vi.waitFor(() => expect(api.runSimulationLive).toHaveBeenCalled());
    useProjectStore.setState({ liveT: 12 });
    store().setParameter("el-shaft", "efficiency_pct", 9);
    store().setParameter("el-shaft", "efficiency_pct", 90); // typing: same moment, one edit
    store().setParameter("el-bat", "soc_init", { 0: 1 } as never); // a table is not sent live
    useProjectStore.setState({ liveT: 30 });
    store().setParameter("el-shaft", "efficiency_pct", 91);
    expect(store().runs[0].snapshot!.liveEdits).toHaveLength(2); // shown while it runs
    finish();
    await running;

    expect(handle.setParam).toHaveBeenCalledTimes(3);
    const snap = store().runs[0].snapshot!;
    expect(snap.liveEdits).toEqual([
      { t: 12, elementId: "el-shaft", key: "efficiency_pct", value: 90 },
      { t: 30, elementId: "el-shaft", key: "efficiency_pct", value: 91 },
    ]);
    expect(findElement("el-shaft")?.parameterOverrides.efficiency_pct).toBe(91);
    expect(snap.project.systems[0].elements.find((e) => e.id === "el-shaft")?.parameterOverrides).toEqual({});
  });

  it("each sweep point's snapshot holds its swept value", async () => {
    await store().init();
    engineFinishesRuns();
    await store().runSweep({ caseId: "case-1", elementId: "el-shaft", paramKey: "efficiency_pct", values: [80, 90] });
    const swept = store()
      .runs.map((r) => r.snapshot!.case.parameterOverrides?.["el-shaft"]?.efficiency_pct)
      .sort();
    expect(swept).toEqual([80, 90]);
    expect(store().project!.cases[0].parameterOverrides).toBeUndefined(); // the project is untouched
  });

  it("the model left out of a snapshot is only the project's saved studies", async () => {
    await store().init();
    engineFinishesRuns();
    await store().runSweep({ caseId: "case-1", elementId: "el-shaft", paramKey: "efficiency_pct", values: [80] });
    expect(store().project!.studies).toHaveLength(1);
    await store().run();
    const { studies: _studies, ...model } = store().project!;
    expect(store().runs[0].snapshot!.project).toEqual(model);
    expect(store().runs[0].snapshot!.project.studies).toBeUndefined();
  });

  it("a run's model opens as an unsaved copy, on the run's case", async () => {
    await store().init();
    engineFinishesRuns();
    store().addCase();
    await store().run();
    const run = store().runs[0];
    store().renameElement("el-bat", "Edited after the run");
    store().openRunModel(run.id);
    const s = store();
    expect(s.project?.id).not.toBe("fixture");
    expect(s.project?.name).toMatch(/^Fixture \(run of .+\)$/);
    expect(s.project?.systems).toEqual(run.snapshot!.project.systems);
    expect(findElement("el-bat")?.label).toBe("Battery");
    expect(s.activeCaseId).toBe(run.caseId);
    expect(s.dirty).toBe(true);
    expect(s.revision).toBeNull();
    expect(messages().some((m) => m.startsWith("info: Opened the model of run 'Case 2'"))).toBe(true);
  });

  it("a run stored before runs kept a snapshot cannot be opened as a model", async () => {
    folder("fixture").set("old", {
      id: "old",
      caseId: "case-1",
      caseName: "Case 1",
      startedAt: 1_000,
      status: "success",
      result: { caseId: "case-1", status: "success", messages: [], channels: [], summary: [] },
    });
    await store().init();
    await settled();
    store().openRunModel("old");
    expect(store().project?.id).toBe("fixture");
  });
});

describe("studies", () => {
  const sweep = (values: number[]) =>
    store().runSweep({ caseId: "case-1", elementId: "el-shaft", paramKey: "efficiency_pct", values });
  const range = (n: number, from: number) => Array.from({ length: n }, (_, i) => from + i);

  /** The engine answers each run with an energy figure of twice the swept efficiency. */
  function engineAnswers() {
    api.validateProject.mockResolvedValue([]);
    api.runSimulationLive.mockImplementation((project, caseId) => {
      const eff = Number(project.cases[0].parameterOverrides?.["el-shaft"]?.efficiency_pct ?? 0);
      const summary = [
        { label: "Energy", value: eff * 2, unit: "kWh" },
        { label: "Final SOC", value: 50, unit: "%", notValid: eff > 90 ? "cycle not followed" : null },
      ];
      return {
        setParam: vi.fn(),
        cancel: vi.fn(),
        done: Promise.resolve({ caseId, status: "success" as const, messages: [], channels: [], summary }),
      };
    });
  }

  it("a finished sweep is saved with the project as a study with its results table", async () => {
    await store().init();
    engineAnswers();
    await sweep([80, 95]);
    const [study] = store().project!.studies!;
    const runIds = store()
      .runs.slice()
      .reverse()
      .map((r) => r.id);
    expect(study).toMatchObject({
      caseId: "case-1",
      caseName: "Case 1",
      factors: [
        {
          elementId: "el-shaft",
          paramKey: "efficiency_pct",
          elementLabel: "Shaft",
          paramLabel: "Mechanical Efficiency",
          unit: "%",
          values: [80, 95],
        },
      ],
      kpis: [
        { label: "Energy", unit: "kWh" },
        { label: "Final SOC", unit: "%" },
      ],
      points: [
        { values: [80], runId: runIds[0], status: "success", kpis: { Energy: 160, "Final SOC": 50 } },
        {
          values: [95],
          runId: runIds[1],
          status: "success",
          kpis: { Energy: 190, "Final SOC": 50 },
          notValid: { "Final SOC": "cycle not followed" },
        },
      ],
    });
    expect(study.id).toBe(store().runs[0].sweepId);
    // part of the project, so it is saved (and kept in the recovery draft) with it
    expect(store().dirty).toBe(true);
    expect(store().past).toHaveLength(0); // not an edit to undo
    await store().saveRemote();
    expect(api.saveProject.mock.calls[0][0].studies).toEqual([study]);
  });

  it("a second 16-point sweep leaves the first study's table intact", async () => {
    await store().init();
    engineAnswers();
    await sweep(range(16, 60));
    const first = structuredClone(store().project!.studies![0]);
    await sweep(range(16, 80));
    const studies = store().project!.studies!;
    expect(studies).toHaveLength(2);
    expect(studies[0]).toEqual(first);
    expect(studies[1].points.map((p) => p.kpis.Energy)).toEqual(range(16, 80).map((v) => v * 2));
    // most of the first sweep's runs have left the 20-run history; its table has not
    expect(store().runs.filter((r) => r.sweepId === first.id)).toHaveLength(4);
    expect(first.points.map((p) => p.kpis.Energy)).toEqual(range(16, 60).map((v) => v * 2));
  });

  it("a stopped sweep lists the stopped point and the points not run", async () => {
    await store().init();
    api.validateProject.mockResolvedValue([]);
    api.runSimulationLive.mockImplementation((_project, caseId) => {
      let stop!: () => void;
      const done = new Promise<SimResult>((resolve) => {
        stop = () =>
          resolve({
            caseId,
            status: "success",
            messages: [{ level: "warning", text: "Run cancelled at t = 3 s." }],
            channels: [],
            summary: [{ label: "Energy", value: 1, unit: "kWh" }],
          });
      });
      return { setParam: vi.fn(), cancel: vi.fn(() => stop()), done };
    });
    const running = sweep([80, 90, 95]);
    await vi.waitFor(() => expect(api.runSimulationLive).toHaveBeenCalled());
    store().stopRun();
    await running;
    expect(store().project!.studies![0].points).toEqual([
      expect.objectContaining({ values: [80], status: "success", incomplete: "stopped at t = 3 s" }),
      { values: [90], status: "not run", kpis: {} },
      { values: [95], status: "not run", kpis: {} },
    ]);
  });

  it("undo and redo leave studies alone; a study can be deleted", async () => {
    await store().init();
    engineAnswers();
    store().renameElement("el-bat", "Pack");
    await sweep([80, 90]);
    const studies = store().project!.studies;
    store().undo();
    expect(findElement("el-bat")?.label).toBe("Battery");
    expect(store().project!.studies).toBe(studies);
    store().redo();
    expect(store().project!.studies).toBe(studies);

    store().removeStudy(studies![0].id);
    expect(store().project!.studies).toEqual([]);
    expect(store().runs).toHaveLength(2); // its runs stay in the history
  });

  it("a study saved with a project comes back when it is opened again", async () => {
    await store().init();
    engineAnswers();
    await sweep([80, 90]);
    const saved = store().project!;
    api.fetchProject.mockResolvedValue(structuredClone(saved));
    await store().openProject(saved.id);
    expect(store().project!.studies).toEqual(saved.studies);
  });
});
