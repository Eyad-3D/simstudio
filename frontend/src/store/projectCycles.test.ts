import { beforeEach, describe, expect, it } from "vitest";
import type { CycleInfo } from "../api";
import type { Project, ProjectCycle } from "../types";
import {
  addProjectCycle,
  cycleUsers,
  followProjectCycles,
  newCycleId,
  ownCycleInfo,
  removeProjectCycle,
  renameProjectCycle,
} from "./projectCycles";
import { useProjectStore } from "./projectStore";

// the same cycles as backend/tests/test_own_cycles.py
const COMMUTE: ProjectCycle = {
  id: "own:commute",
  name: "My commute",
  axis: "time",
  x: [0, 10, 40, 60, 70],
  speed: [0, 30, 40, 20, 0],
  grade: [0, 0, 2, 2, 0],
  source: "commute.csv",
};
const LAP: ProjectCycle = {
  id: "own:lap",
  name: "Test track lap",
  axis: "distance",
  x: [0, 200, 400, 600, 800],
  speed: [20, 60, 40, 70, 30],
};
const HILL: ProjectCycle = {
  id: "own:hill",
  name: "Hill road",
  axis: "distance",
  x: [0, 500, 1000],
  grade: [0, 4, 0],
};

const WLTC: CycleInfo = {
  id: "wltc-3b",
  name: "WLTC class 3b",
  region: "Europe / UN (WLTP)",
  register: "DR-25",
  phases: [],
  duration_s: 1800,
  distance_km: 23.266,
  vmax_kmh: 131.3,
};

function project(): Project {
  return {
    id: "p",
    name: "P",
    systems: [
      {
        id: "sys",
        name: "P",
        parentId: null,
        elements: [
          {
            id: "t1",
            componentDefId: "signal.driving_task",
            label: "Task",
            position: { x: 0, y: 0 },
            parameterOverrides: {},
          },
        ],
        connections: [],
      },
    ],
    dataBusConnections: [],
    cases: [{ id: "c1", name: "City", duration: 600, timeStep: 1 }],
  };
}

const store = () => useProjectStore.getState();

followProjectCycles();

beforeEach(() => {
  useProjectStore.setState({
    project: project(),
    cycles: [WLTC],
    past: [],
    future: [],
    dirty: false,
    readOnly: null,
    messages: [],
  });
});

describe("a cycle's figures (CON-11)", () => {
  it("match the engine's for a cycle against time", () => {
    const info = ownCycleInfo(COMMUTE);
    expect(info).toMatchObject({
      duration_s: 70,
      vmax_kmh: 40,
      axis: "time",
      own: true,
      grade: true,
      speed: true,
    });
    expect(info.distance_km).toBeCloseTo(1900 / 3600, 3);
  });

  it("give a cycle against distance its length and the time its speeds take", () => {
    const info = ownCycleInfo(LAP);
    expect(info.distance_km).toBe(0.8);
    const t = [40, 50, 55, 50].reduce((s, v) => s + 200 / (v / 3.6), 0);
    expect(info.duration_s).toBeCloseTo(t, 3);
    expect(ownCycleInfo(HILL)).toMatchObject({
      speed: false,
      grade: true,
      duration_s: 0,
      vmax_kmh: 0,
    });
  });
});

describe("a project's own cycles (CON-11)", () => {
  it("get an id from their name, numbered when it is taken", () => {
    expect(newCycleId("My Commute (2026)", null)).toBe("own:my-commute-2026");
    expect(newCycleId("Ünïcode Straße", null)).toBe("own:unicode-stra-e");
    expect(newCycleId("***", null)).toBe("own:cycle");
    const p = { ...project(), cycles: [COMMUTE] };
    expect(newCycleId("commute", p)).toBe("own:commute-2");
  });

  it("are kept in the project in one undo step, and listed beside the bundled ones", () => {
    const id = addProjectCycle({ ...COMMUTE, id: undefined });
    expect(id).toBe("own:my-commute");
    expect(store().project!.cycles!.map((c) => c.id)).toEqual(["own:my-commute"]);
    expect(store().dirty).toBe(true);
    expect(store().cycles.map((c) => c.id)).toEqual(["wltc-3b", "own:my-commute"]);
    store().undo();
    expect(store().project!.cycles).toBeUndefined();
    expect(store().cycles.map((c) => c.id)).toEqual(["wltc-3b"]);
  });

  it("set a case's length like a bundled cycle when a Driving Task picks one", () => {
    addProjectCycle(COMMUTE);
    store().setDrivingCycle("t1", "own:commute");
    expect(store().project!.cases[0].duration).toBe(70);
    expect(store().messages.at(-1)!.text).toBe("'City' now runs 70 s, the length of My commute.");
  });

  it("leave a cycle with a grade only out of the Driving Task's list", () => {
    addProjectCycle(HILL);
    addProjectCycle(LAP);
    expect(store().cycles.map((c) => c.id)).toEqual(["wltc-3b", "own:lap"]);
  });

  it("are removed only when nothing uses them", () => {
    addProjectCycle(COMMUTE);
    store().setDrivingCycle("t1", "own:commute");
    expect(cycleUsers(store().project!, "own:commute")).toEqual(["'Task'"]);
    expect(removeProjectCycle("own:commute")).toBe(false);
    expect(store().messages.at(-1)!.text).toBe(
      "Drive cycle 'My commute' is used by 'Task': pick another cycle there first.",
    );
    store().setDrivingCycle("t1", "own:commute", "c1");
    store().setDrivingCycle("t1", "");
    expect(cycleUsers(store().project!, "own:commute")).toEqual(["'Task' in case 'City'"]);
    useProjectStore.setState({
      project: {
        ...store().project!,
        cases: [{ id: "c1", name: "City", duration: 70, timeStep: 1 }],
      },
    });
    expect(removeProjectCycle("own:commute")).toBe(true);
    expect(store().project!.cycles).toBeUndefined();
  });

  it("are renamed in one undo step", () => {
    addProjectCycle(COMMUTE);
    renameProjectCycle("own:commute", "  Way to work ");
    expect(store().project!.cycles![0].name).toBe("Way to work");
    expect(store().cycles.at(-1)!.name).toBe("Way to work");
    store().undo();
    expect(store().project!.cycles![0].name).toBe("My commute");
  });

  it("are not added to a project open read-only", () => {
    useProjectStore.setState({ readOnly: "a newer LightSim saved it" });
    expect(addProjectCycle(COMMUTE)).toBeNull();
    expect(store().project!.cycles).toBeUndefined();
  });
});
