import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import libraryJson from "../../data/componentLibrary.json";
import type { ComponentDef, Project, SimRun } from "../../types";
import { DataChecksPanel } from "./DataChecksPanel";
import { MessagesPanel } from "./MessagesPanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
// jsdom lays nothing out (Messages scrolls to its last line)
Element.prototype.scrollIntoView = () => {};
const libraryById = Object.fromEntries(
  (libraryJson as { components: ComponentDef[] }).components.map((d) => [d.id, d]),
);

let root: Root | null = null;
afterEach(() => {
  act(() => root?.unmount());
  root = null;
  document.body.innerHTML = "";
  useProjectStore.setState({ project: null, dataChecks: null, checksError: null, refused: null, runs: [], messages: [] });
  vi.restoreAllMocks();
});
async function show(node: React.ReactNode) {
  const host = document.body.appendChild(document.createElement("div"));
  root = createRoot(host);
  await act(async () => root!.render(node));
  return host;
}

const project = (mass: number): Project => ({
  id: "p",
  name: "P",
  systems: [
    {
      id: "root",
      name: "P",
      parentId: null,
      elements: [
        { id: "el-1", componentDefId: "vehicle.body", label: "Vehicle", position: { x: 0, y: 0 }, parameterOverrides: { mass_kg: mass } },
      ],
      connections: [],
    },
  ],
  dataBusConnections: [],
  cases: [{ id: "c", name: "City", duration: 10, timeStep: 0.1 }],
});

it("says the Data Checks could not run, and why, instead of a clean '0 errors'", async () => {
  useProjectStore.setState({ project: project(1500), dataChecks: null, checksError: "the engine could not be reached" });
  const host = await show(<DataChecksPanel />);
  expect(host.textContent).toContain("The Data Checks could not run: the engine could not be reached");
  expect(host.textContent).toContain("Nothing has been checked yet");
  expect(host.textContent).toContain("Not checked");
  expect(host.textContent).not.toContain("0 errors");
  expect(host.textContent).not.toContain("Not checked yet. The model is checked by itself");

  // with earlier checks: they stay listed, said to be from the last check that ran
  act(() => useProjectStore.setState({ dataChecks: [{ level: "warning", text: "Brake has no Brake Command signal." }] }));
  expect(host.textContent).toContain("The rows below are from the last check that ran.");
  expect(host.textContent).toContain("Brake has no Brake Command signal.");
  // and the notice goes once they run again
  act(() => useProjectStore.setState({ checksError: null }));
  expect(host.querySelector("[role=alert]")).toBeNull();
});

it("marks the last run's problems once the model has changed since that run", async () => {
  const before = project(1500);
  const run: SimRun = {
    id: "r",
    caseId: "c",
    caseName: "City",
    startedAt: 0,
    status: "failed",
    snapshot: { project: before, case: before.cases[0], appVersion: null, liveEdits: [] },
    result: { caseId: "c", status: "failed", channels: [], summary: [], messages: [{ level: "error", text: "'Vehicle' stalled." }] },
  };
  useProjectStore.setState({ project: before, dataChecks: [], runs: [run], libraryById });
  const host = await show(<DataChecksPanel />);
  expect(host.textContent).toContain("'Vehicle' stalled.");
  expect(host.textContent).toContain("Last run: City");
  expect(host.textContent).not.toContain("model changed since");
  // the model as the run had it, edited: the row may be fixed, or not
  act(() => useProjectStore.setState({ project: project(1200) }));
  expect(host.textContent).toContain("'Vehicle' stalled.");
  expect(host.textContent).toContain("model changed since: run again to check");
});

it("lists a refused model's values, says it was refused, and a row opens the part's window at its value", async () => {
  const open = vi.spyOn(useUIStore.getState(), "openParamDialog").mockImplementation(() => {});
  useProjectStore.setState({
    project: project(1500),
    dataChecks: [{ level: "warning", text: "An older check." }],
    refused: [
      { elementId: "el-1", part: "Vehicle", paramKey: "mass_kg", text: "Vehicle Mass: must be a number (got nothing)", code: "float_type at …" },
      { text: "Highway · duration: Input should be greater than 0 (got -1)", code: "greater_than at …" },
    ],
  });
  const host = await show(<DataChecksPanel />);
  expect(host.textContent).toContain("The engine refused the model: 2 problems");
  expect(host.textContent).not.toMatch(/0 errors|Not checked/);
  expect(host.textContent).toContain("Vehicle Mass: must be a number (got nothing)");
  // the model as refused is all there is: the older checks are not current
  expect(host.textContent).not.toContain("An older check.");
  const rows = [...host.querySelectorAll("li button")] as HTMLButtonElement[];
  expect(rows).toHaveLength(2);
  act(() => rows[0].click());
  expect(open).toHaveBeenCalledWith("el-1", "mass_kg");
  // about no part: nothing to open
  expect(rows[1].disabled).toBe(true);
});

it("a Data Check about one parameter opens the part's window at it; one about no parameter only shows the part", async () => {
  const open = vi.spyOn(useUIStore.getState(), "openParamDialog").mockImplementation(() => {});
  useProjectStore.setState({
    project: project(-1),
    dataChecks: [
      { level: "error", text: "Vehicle Mass of 'Vehicle' must be above 0 kg.", elementIds: ["el-1"], paramKey: "mass_kg" },
      { level: "warning", text: "'Vehicle' is not connected.", elementIds: ["el-1"] },
    ],
  });
  const host = await show(<DataChecksPanel />);
  const rows = [...host.querySelectorAll("li button")] as HTMLButtonElement[];
  act(() => rows[0].click());
  expect(open).toHaveBeenLastCalledWith("el-1", "mass_kg");
  act(() => rows[1].click());
  expect(open).toHaveBeenCalledTimes(1);
});

it("a Data Check about a case's own value makes that case active and opens the part's window at the value", async () => {
  const open = vi.spyOn(useUIStore.getState(), "openParamDialog").mockImplementation(() => {});
  useProjectStore.setState({
    project: project(1500),
    activeCaseId: null,
    dataChecks: [
      { level: "error", text: "Vehicle Mass of 'Vehicle' in case 'City' must be above 0 kg.", elementIds: ["el-1"], paramKey: "mass_kg", caseId: "c" },
    ],
  });
  const host = await show(<DataChecksPanel />);
  act(() => (host.querySelector("li button") as HTMLButtonElement).click());
  expect(useProjectStore.getState().activeCaseId).toBe("c");
  expect(open).toHaveBeenLastCalledWith("el-1", "mass_kg");
});

it("counts in words: one message, two messages", async () => {
  useProjectStore.setState({ messages: [{ time: "09:30:00", level: "info", text: "Opened." }] });
  const host = await show(<MessagesPanel />);
  expect(host.textContent).toContain("1 message");
  expect(host.textContent).not.toMatch(/message\(s\)|1 messages/);
  act(() =>
    useProjectStore.setState({
      messages: [
        { time: "09:30:00", level: "info", text: "Opened." },
        { time: "09:30:01", level: "info", text: "Saved." },
      ],
    }),
  );
  expect(host.textContent).toContain("2 messages");
});
