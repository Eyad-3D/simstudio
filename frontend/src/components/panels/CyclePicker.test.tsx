import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { useProjectStore } from "../../store/projectStore";
import { CyclePreview, CycleSelect } from "./CyclePicker";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("sketches the typed profile only while no cycle is named", async () => {
  const host = document.body.appendChild(document.createElement("div"));
  const root = createRoot(host);
  const points: [number, number][] = [
    [0, 0],
    [600, 80],
  ];
  act(() => useProjectStore.setState({ cycles: [] }));
  await act(async () => root.render(<CyclePreview cycleId="" points={points} />));
  expect(host.textContent).toMatch(/^Custom profile \(not a standard cycle\): 600 s/);
  // a newer file's cycle this version lacks: the typed profile is not what runs
  await act(async () => root.render(<CyclePreview cycleId="nedc" points={points} />));
  expect(host.textContent).toBe("");
});

it("lists the project's own cycles and offers to import one (CON-11)", async () => {
  const host = document.body.appendChild(document.createElement("div"));
  const root = createRoot(host);
  const wltc = {
    id: "wltc-3b",
    name: "WLTC class 3b",
    region: "Europe",
    register: "DR-25",
    phases: [],
    duration_s: 1800,
    distance_km: 23.266,
    vmax_kmh: 131.3,
  };
  act(() =>
    useProjectStore.setState({
      offline: false,
      readOnly: null,
      cycles: [wltc],
      project: {
        id: "p",
        name: "P",
        systems: [],
        dataBusConnections: [],
        cases: [],
        cycles: [
          {
            id: "own:lap",
            name: "Test lap",
            axis: "distance",
            x: [0, 400, 800],
            speed: [20, 60, 30],
            grade: [0, 1, 0],
          },
          {
            id: "own:hill",
            name: "Hill road",
            axis: "distance",
            x: [0, 1000],
            grade: [0, 4],
          },
        ],
      },
    }),
  );
  let picked = "";
  await act(async () => root.render(<CycleSelect value="" label="Drive Cycle" onChange={(v) => (picked = v)} />));
  const options = () => [...host.querySelectorAll("option")].map((o) => o.textContent);
  expect(options()).toEqual([
    "Custom profile (typed points)",
    "WLTC class 3b · 1,800 s · 23.27 km",
    "Test lap · 800 m · top 60.0 km/h · with grade",
    "Import a cycle from a file…",
    "This project's cycles…",
  ]);
  expect([...host.querySelectorAll("optgroup")].map((g) => g.label)).toEqual([
    "Europe",
    "This project",
    "Cycles of your own",
  ]);
  // a Road Profile's list: the cycles with a grade
  await act(async () => root.render(<CycleSelect value="" label="Grade" gradeOnly onChange={() => {}} />));
  expect(options().slice(1, 3)).toEqual([
    "Test lap · 800 m · top 60.0 km/h · with grade",
    "Hill road · 1,000 m · grade only",
  ]);
  // the import action opens its dialog and leaves the value as it was
  await act(async () => root.render(<CycleSelect value="" label="Drive Cycle" onChange={(v) => (picked = v)} />));
  const select = host.querySelector("select")!;
  await act(async () => {
    select.value = [...select.options].find((o) => o.textContent === "Import a cycle from a file…")!.value;
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
  expect(picked).toBe("");
  expect(document.body.querySelector('[role="dialog"]')?.textContent).toContain("Import a drive cycle");
  await act(async () => root.unmount());
});

it("sketches a project's own cycle against distance from the project", async () => {
  const host = document.body.appendChild(document.createElement("div"));
  const root = createRoot(host);
  await act(async () => root.render(<CyclePreview cycleId="own:lap" points={[]} />));
  expect(host.querySelector('[role="img"]')!.getAttribute("aria-label")).toBe(
    "Speed over distance, Test lap: 800 m · top 60.0 km/h · with grade",
  );
  expect(host.textContent).toContain("Test lap (this project's own, against distance): 800 m");
  expect(host.querySelector('[data-testid="cycle-source"]')!.textContent).toBe(
    "Source: your own data (kept in this project; not a standard cycle).",
  );
  await act(async () => root.render(<CyclePreview cycleId="own:hill" points={[]} />));
  expect(host.querySelector('[role="img"]')!.getAttribute("aria-label")).toBe(
    "Grade over distance, Hill road: 1,000 m · grade only",
  );
  await act(async () => root.unmount());
});
