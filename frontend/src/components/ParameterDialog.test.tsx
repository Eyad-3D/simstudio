import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test } from "vitest";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import type { Project } from "../types";
import { ParameterDialog } from "./ParameterDialog";

// tells React the updates below run inside act() on purpose
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

test("the dialog forgets a part that is no longer in the project", () => {
  useProjectStore.setState({ project: { systems: [{ elements: [] }] } as unknown as Project });
  useUIStore.getState().openParamDialog("gone");
  const root = createRoot(document.body.appendChild(document.createElement("div")));
  act(() => root.render(<ParameterDialog />));
  expect(useUIStore.getState().paramDialogId).toBeNull();
  act(() => root.unmount());
});

test("the battery shows only the fields of the way it is defined (MOD-08)", async () => {
  const library = (await import("../data/componentLibrary.json")).default as unknown as {
    components: import("../types").ComponentDef[];
  };
  const battery = library.components.find((c) => c.id === "battery.generic")!;
  const element = {
    id: "b", componentDefId: "battery.generic", label: "Pack", position: { x: 0, y: 0 },
    parameterOverrides: {} as Record<string, unknown>,
  };
  const setProject = () =>
    useProjectStore.setState({
      project: { systems: [{ elements: [element] }] } as unknown as Project,
      libraryById: { "battery.generic": battery },
    } as never);
  setProject();
  useUIStore.getState().openParamDialog("b");
  const root = createRoot(document.body.appendChild(document.createElement("div")));
  act(() => root.render(<ParameterDialog />));
  const text = () => document.body.textContent ?? "";
  expect(text()).toContain("Series Resistance R0");
  expect(text()).not.toContain("Cells in Series");
  element.parameterOverrides = { pack_model: "Cells" };
  act(() => setProject());
  expect(text()).toContain("Cells in Series");
  expect(text()).not.toContain("Series Resistance R0");
  act(() => root.unmount());
  useUIStore.getState().closeParamDialog();
});

test("typing a tyre code fills in the wheel (MOD-48)", async () => {
  const library = (await import("../data/componentLibrary.json")).default as unknown as {
    components: import("../types").ComponentDef[];
  };
  const wheelDef = library.components.find((c) => c.id === "propulsion.wheel")!;
  useProjectStore.setState({
    project: {
      id: "p", name: "P", cases: [], dataBusConnections: [],
      systems: [{ id: "root", name: "Root", parentId: null, connections: [], elements: [{
        id: "w", componentDefId: "propulsion.wheel", label: "Wheel FL", position: { x: 0, y: 0 },
        parameterOverrides: {},
      }] }],
    } as unknown as Project,
    libraryById: { "propulsion.wheel": wheelDef },
  } as never);
  useUIStore.getState().openParamDialog("w");
  const root = createRoot(document.body.appendChild(document.createElement("div")));
  act(() => root.render(<ParameterDialog />));
  const input = document.querySelector<HTMLInputElement>('input[aria-label="Tyre Code"]')!;
  const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  act(() => {
    setValue.call(input, "205/55 R16 91V");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  const overrides = () =>
    useProjectStore.getState().project!.systems[0].elements[0].parameterOverrides;
  expect(overrides().tyre_code).toBe("205/55 R16 91V");
  expect(overrides().radius_m).toBeCloseTo(0.3065, 4);
  expect(overrides().mu_nominal_load_N).toBeCloseTo(3016.6, 1);
  expect(document.querySelector('[data-testid="tyre-summary"]')?.textContent).toContain(
    "load index 91",
  );
  const label = document.querySelector<HTMLSelectElement>(
    'select[aria-label="Rolling Resistance Label Class"]',
  )!;
  const setSelect = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!;
  act(() => {
    setSelect.call(label, "B");
    label.dispatchEvent(new Event("change", { bubbles: true }));
  });
  expect(overrides().rolling_resistance).toBeCloseTo(0.0072, 6);
  act(() => root.unmount());
  useUIStore.getState().closeParamDialog();
});
