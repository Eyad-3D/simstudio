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
