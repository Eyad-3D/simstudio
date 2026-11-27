import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, expect, it, vi } from "vitest";
import type { Project } from "../types";
import { optionLabel } from "../notYet";

// the ribbon's charts library (uPlot) reads matchMedia as it loads
vi.stubGlobal("matchMedia", () => ({ matches: false, addEventListener() {}, removeEventListener() {} }));
let useProjectStore: typeof import("../store/projectStore").useProjectStore;
let useUIStore: typeof import("../store/uiStore").useUIStore;
let PANEL_LIST: typeof import("./DockLayout").PANEL_LIST;
let Ribbon: typeof import("./Ribbon").Ribbon;
beforeAll(async () => {
  ({ useProjectStore } = await import("../store/projectStore"));
  ({ useUIStore } = await import("../store/uiStore"));
  ({ PANEL_LIST } = await import("./DockLayout"));
  ({ Ribbon } = await import("./Ribbon"));
});

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const project: Project = {
  id: "p",
  name: "P",
  systems: [{ id: "root", name: "P", parentId: null, elements: [], connections: [] }],
  dataBusConnections: [],
  cases: [{ id: "c", name: "City", duration: 10, timeStep: 0.1 }],
};

let root: Root | null = null;
afterEach(() => {
  act(() => root?.unmount());
  root = null;
  document.body.innerHTML = "";
});
async function ribbon(tab: string) {
  useProjectStore.setState({ project, running: false });
  useUIStore.setState({ ribbonTab: tab as never });
  const host = document.body.appendChild(document.createElement("div"));
  root = createRoot(host);
  await act(async () => root!.render(<Ribbon />));
  return host;
}
const buttonNamed = (host: HTMLElement, text: string) =>
  [...host.querySelectorAll("button")].find((b) => b.textContent?.trim() === text);
const click = (el: Element) => act(() => el.dispatchEvent(new MouseEvent("click", { bubbles: true })));

it("runs the FS events and Calibrate lap, which this version runs on its lap cases", async () => {
  const runFsEvents = vi.fn(async () => {});
  useProjectStore.setState({ runFsEvents, project: useProjectStore.getState().project ?? ({} as never) });
  const host = await ribbon("simulations");
  const fs = buttonNamed(host, "FS events")!;
  expect(fs.getAttribute("aria-disabled")).toBeNull();
  expect(fs.getAttribute("data-not-yet")).toBeNull();
  await click(fs);
  expect(runFsEvents).toHaveBeenCalledOnce();
  // Calibrate lap runs on the new engine's lap profile: it can be pressed
  const calibrate = buttonNamed(host, "Calibrate lap")!;
  expect(calibrate.getAttribute("aria-disabled")).toBeNull();
  expect(calibrate.getAttribute("data-not-yet")).toBeNull();
  // what does run stays where it was
  for (const name of ["Acceleration test", "Import lap", "Vehicle tests", "US label"])
    expect(buttonNamed(host, name), name).toBeTruthy();
  // the checks have one name: the Data Checks
  expect(buttonNamed(host, "Data Checks")).toBeTruthy();
  expect(buttonNamed(host, "Checks")).toBeUndefined();
  // every list choice runs in this version: a Battery's Defined By: Cells
  // is offered as it is (it was marked "not in this version yet")
  expect(optionLabel("pack_model", "Cells")).toEqual({ label: "Cells", notYet: false });
  expect(optionLabel("pack_model", "Pack values")).toEqual({ label: "Pack values", notYet: false });
});

it("lists every panel by name, and shows the one picked, also one that was closed", async () => {
  const focusPanel = vi.fn();
  useUIStore.setState({ focusPanel });
  const host = await ribbon("home");
  await click(buttonNamed(host, "Panels")!);
  const items = [...host.querySelectorAll("[role=menu][aria-label=Panels] [role=menuitem]")].map((b) => b.textContent);
  expect(items).toEqual(PANEL_LIST.map((p) => p.title));
  expect(items).toEqual(expect.arrayContaining(["Messages", "Problems", "Signal Plot", "Monitors"]));
  await click([...host.querySelectorAll("[role=menuitem]")].find((b) => b.textContent === "Messages")!);
  expect(focusPanel).toHaveBeenCalledWith("messages");
  expect(host.querySelector("[role=menu][aria-label=Panels]")).toBeNull();
});

it("the Results page's ribbon holds the setting to open Results after each run, kept as before (choice B)", async () => {
  useUIStore.getState().setResultsAfterRun(false);
  const host = await ribbon("results");
  const box = [...host.querySelectorAll("label")].find((l) => l.textContent?.includes("Open after each run"))?.querySelector("input");
  expect(box?.checked).toBe(false);
  await act(async () => box!.click());
  expect(useUIStore.getState().resultsAfterRun).toBe(true);
  expect(localStorage.getItem("lightsim-results-after-run")).toBe("1");
  // a value stored before the move reads the same
  await act(async () => useUIStore.getState().setResultsAfterRun(false));
  expect(box!.checked).toBe(false);
});

it("counts an example's stored results as what came with it (app pass #22)", async () => {
  const run = { id: "stored-c", caseId: "c", caseName: "City", startedAt: 0, status: "success", result: {} } as never;
  useProjectStore.setState({ runs: [run], storedRunCount: 0 });
  const host = await ribbon("results");
  expect(host.textContent).toContain("1 result that came with the example");
  expect(host.textContent).not.toContain("not stored on disk");
  useProjectStore.setState({ runs: [] });
});

it("Open lists the saved projects apart from the examples, with when each was saved, and no internal id (#23)", async () => {
  const saved = new Date(2026, 10, 26, 14, 5).getTime();
  vi.stubGlobal("fetch", async (url: string) => {
    const body = url.endsWith("/projects")
      ? [{ id: "bev-car-8yr3i3y", name: "Battery Electric Car", description: "A copy", modified: saved }]
      : url.endsWith("/examples")
        ? [{ id: "bev-car", name: "Battery Electric Car", description: "The example" }]
        : [];
    return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
  });
  const host = await ribbon("home");
  await act(async () => buttonNamed(host, "Open")!.click());
  const menu = host.querySelector('[role="menu"][aria-label="Open project"]')!;
  const yours = menu.querySelector('[role="group"][aria-label="Your projects"]')!;
  const examples = menu.querySelector('[role="group"][aria-label="Examples"]')!;
  expect(yours.textContent).toContain("Battery Electric Car");
  expect(yours.textContent).toContain(`Saved ${new Date(saved).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })}`);
  expect(examples.textContent).toContain("Battery Electric Car");
  expect(menu.textContent).not.toContain("bev-car");
  vi.unstubAllGlobals();
  vi.stubGlobal("matchMedia", () => ({ matches: false, addEventListener() {}, removeEventListener() {} }));
});
