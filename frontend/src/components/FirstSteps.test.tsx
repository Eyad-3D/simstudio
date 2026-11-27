// The step bar goes once its steps are done in the open project, and Help
// shows it again; a hint goes when its page is left or used, and Help shows
// the hints again (calm pages, A).
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it } from "vitest";
import { useProjectStore } from "../store/projectStore";
import { HINTS, hintFor, leaveHint, showHintsAgain, showStepBar, useTourStore } from "../tour";
import type { Project } from "../types";
import { FirstSteps } from "./FirstSteps";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const project = (id: string): Project => ({
  id,
  name: id,
  systems: [{ id: "root", name: id, parentId: null, elements: [], connections: [] }],
  dataBusConnections: [],
  cases: [],
});
const ALL = { build: true, values: true, tests: true, run: true, results: true } as const;

let root: Root | null = null;
beforeEach(() => {
  useTourStore.setState({ toured: true, hidden: false, doneIn: { done: ALL }, hints: {}, hint: null, hintKey: null, shown: false });
  window.dispatchEvent(new KeyboardEvent("keydown")); // hints wait for the user's first action
});
afterEach(() => {
  act(() => root?.unmount());
  root = null;
  document.body.innerHTML = "";
});
async function bar(id: string) {
  useProjectStore.setState({ project: project(id), dataChecks: null, runs: [] } as never);
  const host = document.body.appendChild(document.createElement("div"));
  root = createRoot(host);
  await act(async () => root!.render(<FirstSteps />));
  return () => host.querySelector('nav[aria-label="First steps"]');
}

it("the bar goes once every step is done in the open project, and stays for one where they are not", async () => {
  const nav = await bar("done");
  expect(nav()).toBeNull();
  await act(async () => useProjectStore.setState({ project: project("other") }));
  expect(nav()).not.toBeNull();
});

it("Help → Show First steps brings the done bar back, without the tutorial link (it is in Help)", async () => {
  const nav = await bar("done");
  await act(async () => showStepBar());
  expect(nav()?.textContent).toContain("All done.");
  expect(nav()?.textContent).not.toContain("your first electric car");
});

it("a hint shows once, even with the bar hidden; it goes when its page is left or used; Help shows it again", async () => {
  useTourStore.setState({ hidden: true });
  const area = document.body.appendChild(document.createElement("div"));
  const inside = area.appendChild(document.createElement("button"));
  hintFor("cases", () => area);
  expect(useTourStore.getState().hint).toBe(HINTS.cases);
  // left: gone, and not again on a later visit
  leaveHint("cases");
  expect(useTourStore.getState().hint).toBeNull();
  hintFor("cases", () => area);
  expect(useTourStore.getState().hint).toBeNull();
  // Help → Show hints again: shown on the next visit; a press in its page puts it away
  showHintsAgain();
  hintFor("cases", () => area);
  expect(useTourStore.getState().hint).toBe(HINTS.cases);
  document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));
  expect(useTourStore.getState().hint).toBe(HINTS.cases); // a press elsewhere is not using it
  inside.dispatchEvent(new Event("pointerdown", { bubbles: true }));
  expect(useTourStore.getState().hint).toBeNull();
});

it("leaving another page leaves the hint on show", () => {
  hintFor("cases");
  leaveHint("data-bus");
  expect(useTourStore.getState().hint).toBe(HINTS.cases);
});
