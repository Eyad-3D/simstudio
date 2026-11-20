import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import libraryJson from "../data/componentLibrary.json";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import type { ComponentDef, ElementInstance, Project } from "../types";
import { ParameterDialog } from "./ParameterDialog";
import { markText, tableSlice, type ShownAxis } from "./TablePlot";

// tells React the updates below run inside act() on purpose
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// ---- what the plot draws ----

const speed = (keys: string[], unit = "1/min"): ShownAxis => ({ name: "Speed", lib: "1/min", unit, keys });
const torque = { name: "Torque", lib: "N·m", unit: "N·m" };

test("one axis: one curve with a point per breakpoint, in the units shown", () => {
  const table = { "0": 0, "3000": 0.7, "6000": 0.8 };
  const s = tableSlice(table, [speed(["0", "3000", "6000"], "rad/s")], torque, 0, { mark: [1] });
  expect(s.x.title).toBe("Speed (rad/s)");
  expect(s.yTitle).toBe("Torque (N·m)");
  expect(s.x.values[1]).toBeCloseTo(314.159265, 5);
  expect(s.curves).toEqual([{ label: "Torque", y: [0, 0.7, 0.8] }]);
  expect(s.mark).toEqual({ curve: 0, index: 1 });
  // as the cells show them: digits enough to read back exactly
  expect(markText(table, [speed(["0", "3000", "6000"], "rad/s")], torque, [1])).toBe(
    "Speed 314.159265359 rad/s: Torque 0.7 N·m",
  );
  // the values in another unit too
  const kw = tableSlice({ "1": 1500 }, [speed(["1"])], { name: "Loss", lib: "W", unit: "kW" }, 0);
  expect(kw.curves[0].y).toEqual([1.5]);
  expect(kw.yTitle).toBe("Loss (kW)");
});

test("two axes: a curve per column or per row, the swap moves the mark with it; a missing value is a gap", () => {
  // {column (axis 0): {row (axis 1): value}}, as a 2D map is stored
  const table = { "100": { "0": 1, "10": 2 }, "200": { "0": 3 } };
  const axes = [
    { name: "Voltage", lib: "V", unit: "V", keys: ["100", "200"] },
    { name: "Current", lib: "A", unit: "A", keys: ["0", "10"] },
  ];
  const value = { name: "Loss", lib: "W", unit: "W" };
  // the selected cell: column 200 V, row 0 A
  const mark = [1, 0];

  // rows along x (the default): a curve per column
  const byColumn = tableSlice(table, axes, value, 1, { mark });
  expect(byColumn.x).toEqual({ title: "Current (A)", values: [0, 10] });
  expect(byColumn.curves).toEqual([
    { label: "Voltage 100 V", y: [1, 2] },
    { label: "Voltage 200 V", y: [3, null] },
  ]);
  expect(byColumn.mark).toEqual({ curve: 1, index: 0 });

  // columns along x: a curve per row
  const byRow = tableSlice(table, axes, value, 0, { mark });
  expect(byRow.x).toEqual({ title: "Voltage (V)", values: [100, 200] });
  expect(byRow.curves).toEqual([
    { label: "Current 0 A", y: [1, 3] },
    { label: "Current 10 A", y: [2, null] },
  ]);
  expect(byRow.mark).toEqual({ curve: 0, index: 1 });

  expect(markText(table, axes, value, mark)).toBe("Voltage 200 V, Current 0 A: Loss 3 W");
  expect(markText(table, axes, value, [1, 1])).toBe("Voltage 200 V, Current 10 A: Loss no value");
  expect(markText(table, axes, value, null)).toBe("");
});

test("more axes: a slice holds the others at a breakpoint, and marks only a cell in it", () => {
  const t = { a1: { b1: { c1: 1, c2: 2 }, b2: { c1: 3, c2: 4 } }, a2: { b1: { c1: 5, c2: 6 }, b2: { c1: 7, c2: 8 } } };
  const ax = (name: string, keys: string[]): ShownAxis => ({ name, lib: "", unit: "", keys });
  const axes = [ax("A", ["a1", "a2"]), ax("B", ["b1", "b2"]), ax("C", ["c1", "c2"])];
  const value = { name: "V", lib: "", unit: "" };
  const s = tableSlice(t, axes, value, 2, { by: 1, fixed: [1, 0, 0], mark: [1, 1, 0] });
  expect(s.curves.map((c) => c.y)).toEqual([
    [5, 6],
    [7, 8],
  ]);
  expect(s.mark).toEqual({ curve: 1, index: 0 });
  // a cell at the other breakpoint of A is not in this slice
  expect(tableSlice(t, axes, value, 2, { by: 1, fixed: [1, 0, 0], mark: [0, 1, 0] }).mark).toBeNull();
});

// ---- Copy and Paste through the buttons ----

const library = (libraryJson as unknown as { components: ComponentDef[] }).components;
let root: Root;
const part = () => useProjectStore.getState().project!.systems[0].elements[0];
const table = (key: string) => document.querySelector<HTMLElement>(`[data-param="${key}"]`)!;
const button = (key: string, text: string) =>
  [...table(key).querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.trim() === text)!;
const press = (el: HTMLElement) => act(async () => el.click());
const cell = (key: string, r: number, c: number) =>
  table(key).querySelectorAll(".ss-grid tr")[r].querySelectorAll("td")[c];
const down = (el: Element, shiftKey = false) =>
  act(() => void el.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, shiftKey })));
const notice = (key: string) => table(key).querySelector("[data-grid-notice]")?.textContent ?? "";

let clip = "";
const clipboard = { writeText: vi.fn(), readText: vi.fn() };

function open(parameterOverrides: ElementInstance["parameterOverrides"] = {}) {
  const def = library.find((c) => c.id === "motor.emotor")!;
  const element = { id: "p", componentDefId: def.id, label: "Motor", position: { x: 0, y: 0 }, parameterOverrides };
  useProjectStore.setState({
    project: {
      id: "x", name: "X", cases: [], dataBusConnections: [],
      systems: [{ id: "root", name: "Root", parentId: null, connections: [], elements: [element] }],
    } as unknown as Project,
    libraryById: { [def.id]: def },
    past: [],
    future: [],
  } as never);
  useUIStore.getState().openParamDialog("p");
  act(() => root.render(<ParameterDialog />));
}

beforeEach(() => {
  root = createRoot(document.body.appendChild(document.createElement("div")));
  clip = "";
  clipboard.writeText.mockImplementation(async (t: string) => void (clip = t));
  clipboard.readText.mockImplementation(async () => clip);
  Object.defineProperty(window.navigator, "clipboard", { value: clipboard, configurable: true });
});
afterEach(() => {
  act(() => root.unmount());
  useUIStore.getState().closeParamDialog();
  document.body.innerHTML = "";
});

test("Copy takes the whole table, or the selection; Paste puts a whole table back in place", async () => {
  open({ drag_torque: { "0": 0, "3000": 0.7, "6000": 0.8 } });
  const header = [...table("drag_torque").querySelectorAll(".ss-cell-colhead")].map((h) => h.textContent).join("\t");
  await press(button("drag_torque", "Copy"));
  expect(clip).toBe(`${header}\n0\t0\n3000\t0.7\n6000\t0.8`);
  expect(notice("drag_torque")).toBe("Copied the whole table.");

  // the selection, when it is more than one cell
  down(cell("drag_torque", 2, 1));
  down(cell("drag_torque", 3, 1), true);
  await press(button("drag_torque", "Copy"));
  expect(clip).toBe("0.7\n0.8");

  // the whole table, changed (as in Excel), pastes back over the same cells
  clip = `${header}\n0\t0\n3000\t0.9\n6000\t0.8`;
  await press(button("drag_torque", "Paste"));
  expect(part().parameterOverrides.drag_torque).toEqual({ "0": 0, "3000": 0.9, "6000": 0.8 });

  // a block pastes from the selected cell, as Ctrl+V does
  down(cell("drag_torque", 3, 1));
  clip = "1.5";
  await press(button("drag_torque", "Paste"));
  expect(part().parameterOverrides.drag_torque).toEqual({ "0": 0, "3000": 0.9, "6000": 1.5 });
});

test("a 2D map copied whole pastes back with its breakpoints; the corner names the values", async () => {
  open({ power_loss: { "0": { "0": 0.1, "100": 1.6 }, "3000": { "0": 0.35, "100": 2 } } });
  expect(cell("power_loss", 0, 0).textContent).toMatch(/^\S.* \(kW\)$/);
  // and Enter on it opens their unit's chooser
  down(cell("power_loss", 0, 0));
  const frame = table("power_loss").querySelector(".ss-grid-frame")!;
  act(() => void frame.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
  expect(table("power_loss").querySelector('[role="group"][aria-label^="Unit of the"][aria-label$="values"]')).not.toBeNull();
  await press(button("power_loss", "Copy"));
  const lines = clip.split("\n");
  expect(lines).toHaveLength(3);
  expect(lines[0].split("\t").slice(1)).toEqual(["0", "3000"]);
  clip = clip.replace("\t2", "\t2.5");
  await press(button("power_loss", "Paste"));
  expect(part().parameterOverrides.power_loss).toEqual({ "0": { "0": 0.1, "100": 1.6 }, "3000": { "0": 0.35, "100": 2.5 } });
});

test("a clipboard that cannot be read is said under the table", async () => {
  open();
  clipboard.readText.mockRejectedValue(new DOMException("denied", "NotAllowedError"));
  await press(button("drag_torque", "Paste"));
  const alert = table("drag_torque").querySelector('[role="alert"]');
  expect(alert?.textContent).toMatch(/Could not read the clipboard: permission refused/);
});

test("the plot's line names the selected cell, and follows an edit", async () => {
  open({ drag_torque: { "0": 0, "3000": 0.7 } });
  const mark = () => table("drag_torque").querySelector("[data-plot-mark]")?.textContent;
  down(cell("drag_torque", 2, 1));
  expect(mark()).toMatch(/^Marked: Speed 3000 1\/min: .+ 0\.7 N·m$/);
  clip = "0.75";
  await press(button("drag_torque", "Paste"));
  expect(mark()).toMatch(/ 0\.75 N·m$/);
  // undo: the plot follows the table back
  act(() => useProjectStore.getState().undo());
  expect(mark()).toMatch(/ 0\.7 N·m$/);
});
