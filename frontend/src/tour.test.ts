import { describe, expect, it } from "vitest";
import { valueChanged } from "./tour";
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
