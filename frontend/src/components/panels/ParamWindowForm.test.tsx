// A part's parameter window (U-4.9, U-4.10): its search and Changed only,
// the dot on a changed value and its reset, the sections, and the active
// case's value. The window draws ElementForm (not compact), as here.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import libraryJson from "../../data/componentLibrary.json";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import type { ComponentDef, ElementInstance, Project } from "../../types";
import { ElementForm } from "./PropertiesPanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
Element.prototype.scrollIntoView = () => {};

const libraryById = Object.fromEntries(
  (libraryJson as { components: ComponentDef[] }).components.map((d) => [d.id, d]),
);

function project(overrides: Record<string, unknown> = {}, extra: Partial<ElementInstance> = {}, caseOv?: object): Project {
  return {
    id: "p",
    name: "P",
    dataBusConnections: [],
    cases: [{ id: "c1", name: "City", duration: 600, timeStep: 1, ...(caseOv ? { parameterOverrides: caseOv } : {}) }],
    systems: [
      {
        id: "root",
        name: "P",
        parentId: null,
        connections: [],
        elements: [
          { id: "b", componentDefId: "battery.generic", label: "Pack", position: { x: 0, y: 0 }, parameterOverrides: overrides, ...extra },
          { id: "v", componentDefId: "vehicle.body", label: "Car", position: { x: 0, y: 0 }, parameterOverrides: {} },
        ],
      },
    ],
  } as unknown as Project;
}

let root: Root;
let host: HTMLDivElement;
const el = (id = "b") => useProjectStore.getState().project!.systems[0].elements.find((e) => e.id === id)!;
function Form({ id }: { id: string }) {
  const element = useProjectStore((s) => s.project!.systems[0].elements.find((e) => e.id === id)!);
  return <ElementForm element={element} def={libraryById[element.componentDefId]} />;
}
function show(id = "b") {
  host = document.body.appendChild(document.createElement("div"));
  root = createRoot(host);
  act(() => root.render(<Form id={id} />));
}
const setValue = (input: HTMLInputElement, value: string | boolean) =>
  act(() => {
    if (typeof value === "boolean") {
      if (input.checked !== value) input.click();
    } else {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    }
  });
const search = () => host.querySelector<HTMLInputElement>('input[aria-label="Search parameters"]')!;
const changedOnly = () => [...host.querySelectorAll("label")].find((l) => l.textContent === "Changed only")!.querySelector("input")!;
const rowLabels = () => [...host.querySelectorAll("tr[data-param] td:first-child")].map((td) => td.textContent);
const sections = () => [...host.querySelectorAll<HTMLDetailsElement>("details.ss-param-section")];
const sectionNames = () => sections().map((d) => d.querySelector("summary")!.textContent);

beforeEach(() => {
  localStorage.clear();
  useProjectStore.setState({ project: project(), libraryById, activeCaseId: "c1", past: [], future: [] } as never);
  useProjectStore.getState().beginHistory(); // (a fresh undo step, whatever ran before)
});
afterEach(() => {
  act(() => root.unmount());
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

test("the search filters as you type, on the label and the description, and says when nothing matches", () => {
  show();
  setValue(search(), "initial");
  expect(rowLabels()).toEqual(["Initial SOC"]);
  // a word only in a description ("Minimum SOC": "...the battery stops giving power...")
  const words = libraryById["battery.generic"].parameters.find((p) => p.key === "min_soc_pct")!.description!;
  const word = words.split(/\W+/).find((w) => w.length > 7 && !/soc|minimum/i.test(w))!;
  setValue(search(), word);
  expect(rowLabels()).toContain("Minimum SOC");
  // the other blocks of the form step aside while searching
  expect(host.textContent).not.toContain("Ports");
  setValue(search(), "zzzz");
  expect(rowLabels()).toEqual([]);
  expect(host.querySelector('[role="status"]')?.textContent).toBe("No parameter matches “zzzz”.");
  setValue(search(), "");
  expect(rowLabels().length).toBeGreaterThan(10);
  expect(host.querySelector('[role="status"]')).toBeNull();
});

test("Changed only shows the values that are not the library's, with the search", () => {
  useProjectStore.setState({ project: project({ min_soc_pct: 15, initial_soc_pct: 90 }) });
  show();
  setValue(changedOnly(), true);
  // Initial SOC's 90 is the library's own value: not changed
  expect(rowLabels()).toEqual(["Minimum SOC"]);
  setValue(search(), "initial");
  expect(host.querySelector('[role="status"]')?.textContent).toBe("No changed parameter matches “initial”.");
  setValue(search(), "");
  setValue(changedOnly(), false);
  act(() => useProjectStore.getState().resetParameter("b", "min_soc_pct"));
  setValue(changedOnly(), true);
  expect(host.querySelector('[role="status"]')?.textContent).toBe("No value here differs from the library's.");
});

test("a dot marks a changed value, and a click on it puts the library's back as one undo step", () => {
  useProjectStore.setState({ project: project({}) });
  show();
  const dots = () => [...host.querySelectorAll<HTMLButtonElement>(".ss-changed-dot")];
  expect(dots()).toEqual([]); // nothing changed: no dot, no room taken
  const minSoc = host.querySelector<HTMLInputElement>('tr[data-param="min_soc_pct"] input')!;
  setValue(minSoc, "15");
  setValue(minSoc, "17"); // (two edits of one field: one undo step)
  expect(dots()).toHaveLength(1);
  expect(dots()[0].getAttribute("aria-label")).toBe("Put Minimum SOC back to the library's 10 %");
  act(() => dots()[0].click());
  expect(el().parameterOverrides).not.toHaveProperty("min_soc_pct");
  expect(minSoc.value).toBe("10");
  expect(dots()).toEqual([]);
  // undo takes back the reset alone, then the edits
  act(() => useProjectStore.getState().undo());
  expect(el().parameterOverrides.min_soc_pct).toBe(17);
  act(() => useProjectStore.getState().undo());
  expect(el().parameterOverrides.min_soc_pct).toBeUndefined();
});

test("a table that is not the library's has its dot beside its heading", () => {
  const ocv = libraryById["battery.generic"].parameters.find((p) => p.key === "ocv_table")!;
  useProjectStore.setState({ project: project({ ocv_table: { ...(ocv.default as object), 50: 351 } }) });
  show();
  const dot = host.querySelector<HTMLButtonElement>('[data-param="ocv_table"] .ss-changed-dot')!;
  expect(dot.getAttribute("aria-label")).toBe("Put Open-Circuit Voltage back to the library's value");
  act(() => dot.click());
  expect(el().parameterOverrides).not.toHaveProperty("ocv_table");
});

test("the battery's parameters sit under its sections, the last folded, and folded or open is remembered", () => {
  show();
  const def = libraryById["battery.generic"];
  expect(sectionNames()).toEqual(def.sections!.map((s) => s.name));
  const advanced = () => sections().find((d) => d.querySelector("summary")!.textContent === "Advanced")!;
  expect(advanced().open).toBe(false);
  expect(advanced().querySelector('[data-param="charger_efficiency_pct"]')).not.toBeNull();
  // inside a section, the library's order
  const inCharge = [...sections()[0].querySelectorAll("tr[data-param]")].map((r) => r.getAttribute("data-param"));
  expect(inCharge).toEqual(def.parameters.filter((p) => p.section === "Capacity and charge" && !p.showIf?.values.includes("Cells")).map((p) => p.key));
  act(() => {
    advanced().open = true;
    advanced().dispatchEvent(new Event("toggle"));
  });
  expect(JSON.parse(localStorage.getItem("lightsim-param-sections")!)).toEqual({ "battery.generic/Advanced": true });
  // the next window remembers it
  act(() => root.unmount());
  show();
  expect(advanced().open).toBe(true);
  // a search opens every section with a match, and hides the others
  setValue(search(), "charger");
  expect(sectionNames()).toEqual(["Advanced"]);
});

test("a part with few parameters has no sections", () => {
  useProjectStore.setState({
    project: {
      ...project(),
      systems: [{ id: "root", name: "P", parentId: null, connections: [], elements: [
        { id: "m", componentDefId: "motor.emotor", label: "Motor", position: { x: 0, y: 0 }, parameterOverrides: {} },
      ] }],
    } as unknown as Project,
  });
  show("m");
  expect(libraryById["motor.emotor"].sections ?? []).toEqual([]);
  expect(sectionNames()).toEqual([]);
  expect(rowLabels()).toContain("Rotor Inertia");
});

test("the active case's own value shows under the parameter, in the unit shown, and goes to Cases & Parameters", () => {
  useProjectStore.setState({
    project: project({}, {}, { v: { initial_speed_kmh: 160.9344 } }),
  });
  // the Vehicle's Initial Speed shown in mph
  act(() => useProjectStore.getState().setDisplayUnit("v", "initial_speed_kmh", "value", "mph"));
  show("v");
  const line = host.querySelector<HTMLButtonElement>(".ss-case-link")!;
  expect(line.textContent).toBe("Case “City” runs with 100 mph, not this part's own value.");
  expect(line.closest("tr")!.previousElementSibling!.getAttribute("data-param")).toBe("initial_speed_kmh");
  const focus = vi.spyOn(useUIStore.getState(), "focusPanel");
  act(() => line.click());
  expect(focus).toHaveBeenCalledWith("cases");
  // another case active, or the same value as the part's: no line
  act(() => useProjectStore.setState({ activeCaseId: null }));
  expect(host.querySelector(".ss-case-link")).toBeNull();
});

test("a unit chooser opened under its row is not covered by the row's help card", () => {
  show("v");
  const speed = host.querySelector<HTMLInputElement>('tr[data-param="initial_speed_kmh"] input')!;
  const card = () => host.querySelector(".ss-help-card")!.textContent;
  // the focus in the row: its help card
  act(() => speed.focus());
  expect(card()).toContain("Initial Speed");
  // its unit chooser opened: the card steps aside (in a narrow window it
  // had no room beside the row and lay over the chooser)
  act(() => host.querySelector<HTMLButtonElement>('[aria-label="km/h, unit of Initial Speed"]')!.click());
  expect(host.querySelector('[role="group"]')).not.toBeNull();
  expect(card()).toBe("");
});

test("the form scrolls with room for the sticky search row, so nothing scrolled into view lies under it", async () => {
  const css = (await import("../../index.css?raw")).default;
  const rule = (selector: string) => css.split(`\n${selector} {`)[1]?.split("}")[0] ?? "";
  expect(rule(".ss-param-tools")).toMatch(/position: sticky;/);
  // at least as tall as the row: 8 px padding, a 13 px field, 8 px padding, its border
  const pad = /scroll-padding-top: calc\((\d+)px \* var\(--ss-ui-scale, 1\)\);/.exec(rule(".ss-param-scroll"));
  expect(Number(pad?.[1])).toBeGreaterThanOrEqual(44);
});
