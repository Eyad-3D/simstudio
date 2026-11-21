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

it("shows what this version does not run, but a press does nothing and it says why", async () => {
  const runFsEvents = vi.fn(async () => {});
  useProjectStore.setState({ runFsEvents });
  const host = await ribbon("simulations");
  for (const name of ["FS events"]) {
    const b = buttonNamed(host, name)!;
    expect(b, name).toBeTruthy();
    expect(b.disabled).toBe(false); // it can take focus and be read
    expect(b.getAttribute("aria-disabled")).toBe("true");
    expect(b.getAttribute("data-not-yet")).toBe(`${name}: not in this version of LightSim yet`);
    expect(b.getAttribute("aria-description")).toContain("not in this version");
    await click(b);
  }
  expect(runFsEvents).not.toHaveBeenCalled();
  // Calibrate lap runs on the new engine's lap profile: it can be pressed
  const calibrate = buttonNamed(host, "Calibrate lap")!;
  expect(calibrate.getAttribute("aria-disabled")).toBeNull();
  expect(calibrate.getAttribute("data-not-yet")).toBeNull();
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
