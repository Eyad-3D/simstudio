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
