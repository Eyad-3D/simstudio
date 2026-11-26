import { describe, expect, it } from "vitest";
import { useUIStore } from "./store/uiStore";
import { useProjectStore } from "./store/projectStore";
import { HINTS, tick, useTourStore, valueChanged, watchSteps } from "./tour";
import type { ElementInstance, Project } from "./types";

const el = (id: string, parameterOverrides: ElementInstance["parameterOverrides"] = {}): ElementInstance => ({
  id,
  componentDefId: "signal.constant",
  label: id,
  position: { x: 0, y: 0 },
  parameterOverrides,
});
const model = (...elements: ElementInstance[]): Project => ({
  id: "p",
  name: "P",
  systems: [{ id: "root", name: "P", parentId: null, elements, connections: [] }],
  dataBusConnections: [],
  cases: [],
});

describe("the step bar's Set values", () => {
  it("ticks when a value of a part already in the model changes", () => {
    expect(valueChanged(model(el("a")), model(el("a", { value: 2 })))).toBe(true);
    expect(valueChanged(model(el("a", { value: 2 })), model(el("a", {})))).toBe(true);
  });

  it("does not tick when a part is added, deleted or left as it was", () => {
    const a = el("a", { value: 2 });
    expect(valueChanged(model(a), model(a, el("b")))).toBe(false); // added from the library
    expect(valueChanged(model(a, el("b", { value: 5 })), model(a))).toBe(false); // deleted
    expect(valueChanged(model(a), model(el("a", { value: 2 })))).toBe(false); // same values
  });
});

describe("the one-time hints", () => {
  it("the Results page's hint goes when the page does", () => {
    useTourStore.setState({ hints: {}, hidden: false, hint: null, hintKey: null });
    useUIStore.setState({ ribbonTab: "home" });
    window.dispatchEvent(new KeyboardEvent("keydown")); // hints wait for the user's first action
    const stop = watchSteps();
    useUIStore.setState({ ribbonTab: "results" });
    expect(useTourStore.getState().hint).toBe(HINTS.results);
    useUIStore.setState({ ribbonTab: "home" });
    expect(useTourStore.getState().hint).toBeNull();
    // a panel's hint is not the page's: it stays when the page changes
    useTourStore.setState({ hint: HINTS.cases, hintKey: "cases" });
    useUIStore.setState({ ribbonTab: "simulations" });
    expect(useTourStore.getState().hint).toBe(HINTS.cases);
    stop();
  });
});

describe("the step bar follows the open project", () => {
  it("a new empty project shows no step done; the example's stay with the example", () => {
    useTourStore.setState({ doneIn: {}, hidden: false });
    useProjectStore.setState({ project: { ...model(el("a")), id: "example" }, dataChecks: [], runs: [], activeCaseId: null } as never);
    const stop = watchSteps();
    // the example has parts and no errors: Build is done there
    expect(useTourStore.getState().doneIn?.example).toEqual({ build: true });
    tick("run");
    // a new empty project: nothing done, Build with 0 parts not either
    useProjectStore.setState({ project: { ...model(), id: "new" }, dataChecks: [] } as never);
    expect(useTourStore.getState().doneIn?.new).toBeUndefined();
    // back to the example: its steps are as they were
    expect(useTourStore.getState().doneIn?.example).toEqual({ build: true, run: true });
    stop();
  });

  it("a Save As (the same project under a new id) keeps its steps", () => {
    useTourStore.setState({ doneIn: {}, hidden: false });
    const p = { ...model(el("a")), id: "before" };
    useProjectStore.setState({ project: p, dataChecks: [], runs: [], activeCaseId: null } as never);
    const stop = watchSteps();
    tick("values");
    useProjectStore.setState({ project: { ...p, id: "saved-as" } } as never);
    expect(useTourStore.getState().doneIn?.["saved-as"]).toEqual({ build: true, values: true });
    stop();
  });
});
