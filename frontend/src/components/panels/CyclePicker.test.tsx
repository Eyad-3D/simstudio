import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { useProjectStore } from "../../store/projectStore";
import { CyclePreview } from "./CyclePicker";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("sketches the typed profile only while no cycle is named", async () => {
  const host = document.body.appendChild(document.createElement("div"));
  const root = createRoot(host);
  const points: [number, number][] = [[0, 0], [600, 80]];
  act(() => useProjectStore.setState({ cycles: [] }));
  await act(async () => root.render(<CyclePreview cycleId="" points={points} />));
  expect(host.textContent).toMatch(/^Custom profile: 600 s/);
  // a newer file's cycle this version lacks: the typed profile is not what runs
  await act(async () => root.render(<CyclePreview cycleId="nedc" points={points} />));
  expect(host.textContent).toBe("");
});
