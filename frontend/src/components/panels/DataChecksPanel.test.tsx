import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it } from "vitest";
import { useProjectStore } from "../../store/projectStore";
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
  useProjectStore.setState({ project: null, dataChecks: null, checksError: null, runs: [], messages: [] });
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
