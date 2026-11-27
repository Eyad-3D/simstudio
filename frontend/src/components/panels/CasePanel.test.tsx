import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { useProjectStore } from "../../store/projectStore";
import { CasePanel } from "./CasePanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function project(kind?: "cycle" | "acceleration") {
  return {
    id: "p",
    name: "P",
    systems: [],
    dataBusConnections: [],
    cases: [{ id: "c", name: "City", duration: 600, timeStep: 1, ...(kind ? { kind } : {}) }],
  };
}

it("sets a cycle case's run mode to quick and back (step 2's quick mode)", async () => {
  act(() => useProjectStore.setState({ project: project(), activeCaseId: "c", readOnly: null }));
  const host = document.body.appendChild(document.createElement("div"));
  await act(async () => createRoot(host).render(<CasePanel />));
  const select = host.querySelector<HTMLSelectElement>('select[aria-label="Run mode"]');
  expect(select).not.toBeNull();
  expect(select!.value).toBe("full");
  expect([...select!.options].map((o) => o.textContent)).toEqual(["Full", "Quick"]);
  await act(async () => {
    select!.value = "quick";
    select!.dispatchEvent(new Event("change", { bubbles: true }));
  });
  expect(useProjectStore.getState().project!.cases[0].mode).toBe("quick");
  await act(async () => {
    select!.value = "full";
    select!.dispatchEvent(new Event("change", { bubbles: true }));
  });
  expect(useProjectStore.getState().project!.cases[0].mode).toBeUndefined();
});

it("quotes a refused quick run's words as the run gives them: the reason, then the code", async () => {
  act(() => useProjectStore.setState({ project: project(), activeCaseId: "c", readOnly: null }));
  const host = document.body.appendChild(document.createElement("div"));
  await act(async () => createRoot(host).render(<CasePanel />));
  const balance = [...host.querySelectorAll("label")].find((l) => l.textContent?.includes("Charge balance"));
  const tip = balance?.getAttribute("title") ?? "";
  expect(tip).toContain("“Not run in quick mode: … [QUICK-BALANCE]”");
  expect(tip).not.toContain("Not run [");
});

it("offers no run mode for a case that drives no cycle", async () => {
  act(() => useProjectStore.setState({ project: project("acceleration"), activeCaseId: "c", readOnly: null }));
  const host = document.body.appendChild(document.createElement("div"));
  await act(async () => createRoot(host).render(<CasePanel />));
  expect(host.querySelector('select[aria-label="Run mode"]')).toBeNull();
});
