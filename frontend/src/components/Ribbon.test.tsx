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

it("offers no button that does nothing: what this version does not run is not shown", async () => {
  const host = await ribbon("simulations");
  // FS events needs lap mode, which this version does not run (UI 4.43)
  expect(buttonNamed(host, "FS events")).toBeUndefined();
  // Calibrate lap stays, dimmed: a lesson and its doc test name it (a question for the owner)
  const calibrate = buttonNamed(host, "Calibrate lap")!;
  expect(calibrate.getAttribute("data-not-yet")).toBe("Calibrate lap: not in this version of LightSim yet");
  await click(calibrate);
  expect(document.querySelector("[role=dialog]")).toBeNull();
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
