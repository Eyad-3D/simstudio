// The table tools in the parameter window's form: cells shaded by value, a
// right-click on selected cells (set, multiply, add, interpolate), typing a
// value and Ctrl+Enter, a right-click on a header (insert, delete) in a
// table of two and of three axes, a paste past the end, and a cell picked
// from the plot. Every action is one undo step.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { gridKeys, type Grid } from "../../tableN";
import type { ComponentDef, Project } from "../../types";
import { ParameterDialog } from "../ParameterDialog";
import { SpreadsheetGrid } from "../SpreadsheetGrid";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** A part with a curve, a map (Speed × Torque, a plane: rpm/1000 + N·m/10)
 *  and a table of three axes. */
const def: ComponentDef = {
  id: "test.tables",
  category: "Test",
  name: "Tables",
  icon: "table",
  domain: "signal",
  ports: [],
  parameters: [
    {
      key: "curve",
      label: "Curve",
      unit: "-",
      type: "table",
      axes: [{ name: "Speed", unit: "-", outside: "clamp" }],
      default: { "0": 0, "10": 5, "20": 10, "30": 15 },
    },
    {
      key: "map",
      label: "Loss",
      unit: "-",
      type: "table",
      axes: [
        { name: "Speed", unit: "-", outside: "clamp" },
        { name: "Torque", unit: "-", outside: "clamp" },
      ],
      default: {
        "1000": { "0": 1, "10": 2, "20": 3 },
        "2000": { "0": 2, "10": 3, "20": 4 },
        "4000": { "0": 4, "10": 5, "20": 6 },
      },
    },
    {
      key: "r",
      label: "Cell Resistance",
      unit: "-",
      type: "table",
      axes: [
        { name: "Temperature", unit: "-", outside: "clamp" },
        { name: "SOC", unit: "-", outside: "clamp" },
        { name: "Current", unit: "-", outside: "clamp" },
      ],
      default: {
        "0": { "20": { "-100": 3.1, "100": 2.9 }, "80": { "-100": 2.6, "100": 2.4 } },
        "25": { "20": { "-100": 1.9, "100": 1.8 }, "80": { "-100": 1.5, "100": 1.4 } },
      },
    },
  ],
} as unknown as ComponentDef;

let root: Root;
beforeEach(() => {
  window.localStorage.clear();
  useUIStore.setState({ tableShade: { curve: false, map: true } });
  root = createRoot(document.body.appendChild(document.createElement("div")));
  useProjectStore.setState({
    project: {
      id: "x", name: "X", cases: [], dataBusConnections: [],
      systems: [{ id: "root", name: "Root", parentId: null, connections: [], elements: [
        { id: "p", label: "Part", componentDefId: def.id, position: { x: 0, y: 0 }, parameterOverrides: {} },
      ] }],
    } as unknown as Project,
    libraryById: { [def.id]: def },
    unitGroups: {},
    past: [],
    future: [],
  } as never);
  useUIStore.getState().openParamDialog("p");
  act(() => root.render(<ParameterDialog />));
});
afterEach(() => {
  act(() => root.unmount());
  useUIStore.getState().closeParamDialog();
  document.body.innerHTML = "";
});

const value = (key: string) => {
  const el = useProjectStore.getState().project!.systems[0].elements[0];
  return (el.parameterOverrides[key] ?? def.parameters.find((p) => p.key === key)!.default) as Grid;
};
const undo = () => act(() => useProjectStore.getState().undo());
const undoSteps = () => (useProjectStore.getState() as unknown as { past: unknown[] }).past.length;
/** The grid of the parameter whose table shows `label` in its first header. */
const grid = (label: string) =>
  [...document.querySelectorAll<HTMLElement>(".ss-table-box")].find((b) =>
    b.querySelector(".ss-grid td")?.textContent?.startsWith(label),
  )!;
const cell = (g: HTMLElement, r: number, c: number) => g.querySelectorAll(".ss-grid tr")[r].querySelectorAll("td")[c];
const down = (el: Element, shiftKey = false) =>
  act(() => void el.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, shiftKey })));
const rightClick = (el: Element) =>
  act(() => void el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 5, clientY: 5 })));
const item = (text: string) =>
  [...document.querySelectorAll<HTMLButtonElement>("[role=menuitem]")].find((b) => b.textContent === text)!;
const click = (el: HTMLElement) => act(async () => el.click());
const type = (input: HTMLInputElement, text: string) =>
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, text);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
/** Answer the menu's number field and press OK. */
const answer = async (text: string) => {
  const box = document.querySelector<HTMLElement>(".ss-grid-menu")!;
  await type(box.querySelector("input")!, text);
  await click([...box.querySelectorAll("button")].find((b) => b.textContent === "OK")!);
};
const notice = () => document.querySelector("[data-grid-notice]")?.textContent ?? "";

test("a map is shaded low to high, a curve is not; the switch is kept per kind", async () => {
  const map = grid("Loss");
  expect(cell(map, 1, 1).getAttribute("style")).toMatch(/linear-gradient\(rgba?\(68,? 1,? 84/);
  expect(cell(map, 3, 3).getAttribute("style")).toMatch(/rgba?\(253,? 231,? 37/);
  // breakpoints and headers are not shaded, nor a curve
  expect(cell(map, 1, 0).getAttribute("style")).toBeNull();
  expect(cell(grid("Speed"), 1, 1).getAttribute("style")).toBeNull();
  const shade = [...map.parentElement!.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.includes("Shade"))!;
  expect(shade.getAttribute("aria-pressed")).toBe("true");
  await click(shade);
  expect((cell(grid("Loss"), 1, 1) as HTMLElement).style.backgroundImage).toBe("");
  expect(JSON.parse(window.localStorage.getItem("lightsim-table-shade")!)).toEqual({ curve: false, map: false });
  // the numbers are still there
  expect(cell(grid("Loss"), 1, 1).textContent).toBe("1");
});

test("multiply a selection from its right-click menu: one undo step, the rest untouched", async () => {
  const map = grid("Loss");
  down(cell(map, 1, 1));
  down(cell(map, 2, 2), true);
  rightClick(cell(map, 2, 2));
  expect(document.querySelector("[role=menu]")?.getAttribute("aria-label")).toBe("Cell actions");
  const before = undoSteps();
  await click(item("Multiply by…"));
  await answer("x");
  expect(document.querySelector(".ss-grid-menu")?.textContent).toContain("Enter a factor as a number");
  await answer("1.5");
  expect(document.querySelector(".ss-grid-menu")).toBeNull();
  expect(value("map")).toEqual({
    "1000": { "0": 1.5, "10": 3, "20": 3 },
    "2000": { "0": 3, "10": 4.5, "20": 4 },
    "4000": { "0": 4, "10": 5, "20": 6 },
  });
  expect(notice()).toBe("Multiplied 4 cells by 1.5.");
  expect(undoSteps()).toBe(before + 1);
  undo();
  expect(value("map")).toEqual(def.parameters[1].default);
  // the line no longer names a change that is gone
  expect(notice()).toBe("");
});

test("add to a selection, and set it to a value by typing it and Ctrl+Enter", async () => {
  const map = grid("Loss");
  down(cell(map, 3, 1));
  down(cell(map, 3, 3), true);
  rightClick(cell(map, 3, 1));
  await click(item("Add…"));
  await answer("-0.5");
  expect(value("map")).toMatchObject({ "1000": { "20": 2.5 }, "2000": { "20": 3.5 }, "4000": { "20": 5.5 } });
  // a typed value goes into every selected cell at once
  const frame = grid("Loss").querySelector(".ss-grid-frame")!;
  act(() => void frame.dispatchEvent(new KeyboardEvent("keydown", { key: "7", bubbles: true })));
  const input = grid("Loss").querySelector<HTMLInputElement>(".ss-cell-input")!;
  const before = undoSteps();
  act(() => void input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", ctrlKey: true, bubbles: true })));
  expect(value("map")).toMatchObject({ "1000": { "20": 7 }, "2000": { "20": 7 }, "4000": { "20": 7 } });
  expect(undoSteps()).toBe(before + 1);
  expect(notice()).toBe("Set 3 cells to 7.");
});

test("interpolate between a selection's edges, at the breakpoints' values", async () => {
  const map = grid("Loss");
  // a wrong middle value, then the whole map selected and interpolated
  down(cell(map, 2, 2));
  rightClick(cell(map, 2, 2));
  expect(item("Interpolate between the edges").disabled).toBe(true);
  await click(item("Set to…"));
  await answer("99");
  expect(value("map")).toMatchObject({ "2000": { "10": 99 } });
  down(cell(grid("Loss"), 1, 1));
  down(cell(grid("Loss"), 3, 3), true);
  rightClick(cell(grid("Loss"), 2, 2));
  await click(item("Interpolate between the edges"));
  // Speed 2000 sits a third of the way from 1000 to 4000: the plane's 3
  expect(value("map")).toEqual(def.parameters[1].default);
});

test("insert a column in a map: midway, its values interpolated; delete takes it out", async () => {
  rightClick(cell(grid("Loss"), 0, 2));
  expect(document.querySelector("[role=menu]")?.getAttribute("aria-label")).toBe("Column actions");
  await click(item("Insert column to the right"));
  const m = value("map") as Record<string, Record<string, number>>;
  expect(gridKeys(m, 2)[0]).toEqual(["1000", "2000", "3000", "4000"]);
  expect(m["3000"]).toEqual({ "0": 3, "10": 4, "20": 5 });
  // at the end: the last step on, the edge copied
  rightClick(cell(grid("Loss"), 1, 0));
  await click(item("Insert row below"));
  rightClick(cell(grid("Loss"), 4, 0));
  await click(item("Insert row below"));
  expect((value("map") as Record<string, Record<string, number>>)["4000"]).toEqual({ "0": 4, "5": 4.5, "10": 5, "20": 6, "30": 6 });
  // delete the column inserted, and every row but one cannot go
  rightClick(cell(grid("Loss"), 0, 3));
  await click(item("Delete column"));
  expect(gridKeys(value("map"), 2)[0]).toEqual(["1000", "2000", "4000"]);
  down(cell(grid("Loss"), 1, 0));
  down(cell(grid("Loss"), 5, 0), true);
  rightClick(cell(grid("Loss"), 2, 0));
  expect(item("Delete 5 rows").disabled).toBe(true);
});

test("insert a breakpoint in a table of three axes: the whole slice, in what is saved; one undo step", async () => {
  // rows: Current, columns: SOC, Temperature held
  const g = grid("Cell Resistance");
  expect(cell(g, 0, 1).textContent).toBe("20");
  rightClick(cell(g, 0, 1));
  const before = undoSteps();
  await click(item("Insert column to the right"));
  const t = value("r") as Record<string, Record<string, Record<string, number>>>;
  expect(gridKeys(t, 3)).toEqual([["0", "25"], ["20", "50", "80"], ["-100", "100"]]);
  expect(t["0"]["50"]).toEqual({ "-100": 2.85, "100": 2.65 });
  expect(t["25"]["50"]).toEqual({ "-100": 1.7, "100": 1.6 });
  expect(document.body.textContent).toContain("Inserted SOC 50: its points are interpolated between its neighbours, in every slice.");
  expect(undoSteps()).toBe(before + 1);
  undo();
  expect(gridKeys(value("r"), 3)[1]).toEqual(["20", "80"]);
});

test("a paste past the end grows the table: breakpoints not given continue the last step", async () => {
  const map = grid("Loss");
  down(cell(map, 3, 1));
  const before = undoSteps();
  act(() => {
    const e = new Event("paste", { bubbles: true, cancelable: true }) as Event & { clipboardData: unknown };
    e.clipboardData = { getData: () => "9\t9\n8\t8\n7\t7" };
    map.querySelector(".ss-grid-frame")!.dispatchEvent(e);
  });
  const m = value("map") as Record<string, Record<string, number>>;
  expect(gridKeys(m, 2)[1]).toEqual(["0", "10", "20", "30", "40"]);
  expect(m["1000"]).toMatchObject({ "20": 9, "30": 8, "40": 7 });
  expect(undoSteps()).toBe(before + 1);
  expect(notice()).toBe(
    "The paste went past the end of the table: added 2 rows. A breakpoint the paste did not give continues the last step.",
  );
});

test("a cell picked on the plot is selected (the grid's focusCell)", () => {
  act(() => root.unmount());
  root = createRoot(document.body.appendChild(document.createElement("div")));
  const matrix = [0, 1, 2].map((r) => [0, 1, 2].map((c) => ({ text: `${r}${c}`, kind: r && c ? "body" : "colHeader" }) as const));
  act(() => root.render(<SpreadsheetGrid matrix={matrix} onCommit={() => {}} onPasteBlock={() => {}} focusCell={{ r: 2, c: 1 }} />));
  expect(document.querySelector("td.ss-active")?.textContent).toBe("21");
});
