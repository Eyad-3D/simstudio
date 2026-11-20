import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import libraryJson from "../data/componentLibrary.json";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import type { ComponentDef, ElementInstance, Project } from "../types";
import { ParameterDialog } from "./ParameterDialog";

// tells React the updates below run inside act() on purpose
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const library = (libraryJson as unknown as { components: ComponentDef[] }).components;
const defOf = (id: string) => library.find((c) => c.id === id)!;

let root: Root;
const part = () => useProjectStore.getState().project!.systems[0].elements[0];
const q = <T extends Element>(sel: string) => document.querySelector<T>(sel)!;
const byText = (text: string, sel = "button") =>
  [...document.querySelectorAll<HTMLElement>(sel)].find((b) => b.textContent === text)!;
const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
function type(input: HTMLInputElement, text: string) {
  act(() => {
    setValue.call(input, text);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const click = (el: HTMLElement) => act(() => el.click());
const key = (el: Element, k: string) =>
  act(() => void el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true })));

function open(componentDefId: string, parameterOverrides: ElementInstance["parameterOverrides"] = {}) {
  const element = { id: "p", componentDefId, label: "Part", position: { x: 0, y: 0 }, parameterOverrides };
  useProjectStore.setState({
    project: {
      id: "x", name: "X", cases: [], dataBusConnections: [],
      systems: [{ id: "root", name: "Root", parentId: null, connections: [], elements: [element] }],
    } as unknown as Project,
    libraryById: { [componentDefId]: defOf(componentDefId) },
    past: [],
    future: [],
  } as never);
  useUIStore.getState().openParamDialog("p");
  act(() => root.render(<ParameterDialog />));
}

beforeEach(() => {
  root = createRoot(document.body.appendChild(document.createElement("div")));
});
afterEach(() => {
  act(() => root.unmount());
  useUIStore.getState().closeParamDialog();
  document.body.innerHTML = "";
});

test("a number shown in another unit: same value, typed in that unit, limits in it; nothing written by showing", () => {
  open("vehicle.body", { initial_speed_kmh: 36 });
  const field = () => q<HTMLInputElement>('input[aria-label="Initial Speed"]');
  const unit = () => q<HTMLButtonElement>('button[aria-label$="unit of Initial Speed"]');
  expect(field().value).toBe("36");
  // a unit with no other stays plain text: no control for % or -
  expect(document.querySelector('button[aria-label$="unit of Drag Coefficient"]')).toBeNull();

  click(unit());
  expect(unit().getAttribute("aria-expanded")).toBe("true");
  const chooser = q('[role="group"][aria-label="Unit of Initial Speed"]');
  // the current unit has the focus; the library's is named as the saved one
  expect(document.activeElement?.textContent).toBe("km/h (as saved)");
  const before = useProjectStore.getState().past.length;
  click(byText("m/s"));
  expect(part().parameterOverrides.initial_speed_kmh).toBe(36);
  expect(part().displayUnits).toEqual({ initial_speed_kmh: { value: "m/s" } });
  expect(field().value).toBe("10");
  expect(useProjectStore.getState().past.length).toBe(before + 1);
  expect(chooser.isConnected).toBe(false);
  expect(document.activeElement).toBe(unit());

  // what is typed is read in the shown unit, and stored once in the library's
  type(field(), "20");
  expect(part().parameterOverrides.initial_speed_kmh).toBe(72);
  // the limit's message speaks the shown unit; the check stays in km/h
  type(field(), "-1");
  expect(part().parameterOverrides.initial_speed_kmh).toBe(-3.6);
  expect(document.body.textContent).toContain("must be at least 0 m/s");
  type(field(), "20");

  // back to km/h writes nothing either
  click(unit());
  click(byText("km/h (as saved)"));
  expect(part().parameterOverrides.initial_speed_kmh).toBe(72);
  expect(part().displayUnits).toBeUndefined();
  expect(field().value).toBe("72");
});

test("'the number is in another unit' keeps the number, changes the value, in one undo step", () => {
  open("vehicle.body", { initial_speed_kmh: 50 });
  const field = () => q<HTMLInputElement>('input[aria-label="Initial Speed"]');
  click(q('button[aria-label$="unit of Initial Speed"]'));
  const groups = document.querySelectorAll('[role="group"][aria-label="Unit of Initial Speed"] [role="group"]');
  const before = useProjectStore.getState().past.length;
  click([...groups[1].querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "mph")!);
  expect(field().value).toBe("50");
  expect(part().parameterOverrides.initial_speed_kmh).toBe(80.4672);
  expect(part().displayUnits).toEqual({ initial_speed_kmh: { value: "mph" } });
  expect(useProjectStore.getState().past.length).toBe(before + 1);
  act(() => useProjectStore.getState().undo());
  expect(part().parameterOverrides.initial_speed_kmh).toBe(50);
  expect(part().displayUnits).toBeUndefined();
  expect(field().value).toBe("50");
});

test("Esc closes the chooser, and only the chooser", () => {
  open("vehicle.body");
  click(q('button[aria-label$="unit of Initial Speed"]'));
  key(document.activeElement!, "Escape");
  expect(document.querySelector(".ss-unit-chooser")).toBeNull();
  expect(useUIStore.getState().paramDialogId).toBe("p");
});

test("a table's axis and values: header double-click or Enter, both actions, cells typed in the shown unit", () => {
  open("motor.emotor", { drag_torque: { "0": 0, "3000": 0.7, "6000": 0.8 } });
  const section = q('[data-param="drag_torque"]');
  const cells = () => [...section.querySelectorAll(".ss-cell")].map((c) => c.textContent);
  expect(cells()).toEqual(["Speed (1/min)", "Drag Torque (unpowered) (N·m)", "0", "0", "3000", "0.7", "6000", "0.8"]);

  // a double-click on the axis header opens the axis's chooser
  act(() => void section.querySelector(".ss-cell-colhead")!.closest("td")!.dispatchEvent(
    new MouseEvent("dblclick", { bubbles: true }),
  ));
  expect(q('[role="group"][aria-label="Unit of the Speed axis"]')).toBeTruthy();
  click(byText("rad/s"));
  expect(cells().slice(0, 3)).toEqual(["Speed (rad/s)", "Drag Torque (unpowered) (N·m)", "0"]);
  expect(cells()[4]).toBe("314.159265359");
  expect(part().parameterOverrides.drag_torque).toEqual({ "0": 0, "3000": 0.7, "6000": 0.8 });
  expect(part().displayUnits).toEqual({ drag_torque: { axes: ["rad/s"] } });

  // a cell typed in rad/s is stored in 1/min; the other cells stay exactly as they were
  const grid = q<HTMLElement>(`[data-param="drag_torque"] .ss-grid-frame`);
  const td = (r: number, c: number) => grid.querySelectorAll("tr")[r].querySelectorAll("td")[c];
  act(() => void td(3, 0).dispatchEvent(new MouseEvent("mousedown", { bubbles: true })));
  key(grid, "1");
  const input = q<HTMLInputElement>(".ss-cell-input");
  type(input, "1000");
  key(input, "Enter");
  expect(part().parameterOverrides.drag_torque).toEqual({ "0": 0, "3000": 0.7, "9549.29658551": 0.8 });

  // the values' chooser from its visible control: the numbers were lbf·ft
  click(q('button[aria-label="N·m, unit of the Drag Torque (unpowered) values"]'));
  const groups = document.querySelectorAll('[aria-label="Unit of the Drag Torque (unpowered) values"] [role="group"]');
  click([...groups[1].querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "lbf·ft")!);
  const t = part().parameterOverrides.drag_torque as Record<string, number>;
  expect(t["3000"]).toBeCloseTo(0.94907, 5);
  expect(cells()[5]).toBe("0.7");
  expect(cells()[1]).toBe("Drag Torque (unpowered) (lbf·ft)");

  // Enter on a header opens its chooser from the keyboard
  act(() => void td(0, 0).dispatchEvent(new MouseEvent("mousedown", { bubbles: true })));
  key(grid, "Enter");
  expect(q('[role="group"][aria-label="Unit of the Speed axis"]')).toBeTruthy();
});
