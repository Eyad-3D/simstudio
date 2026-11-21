import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { SummaryValue } from "../../types";
import type { RunRow } from "./runRows";
import { RunsTable } from "./RunsTable";

// tells React the updates below run inside act() on purpose
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root;
let host: HTMLDivElement;
beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const consumption = (value: number): SummaryValue[] => [
  { key: "consumption_kwh_per_100km", label: "Consumption", value, unit: "kWh/100km" },
];
const row = (id: string, startedAt: number, value: number, extra: Partial<RunRow> = {}): RunRow => ({
  id,
  caseName: "City Cycle",
  startedAt,
  status: "success",
  summary: consumption(value),
  loaded: true,
  ...extra,
});
const rows = [row("a", 3_000, 14.2, { name: "Heavier" }), row("b", 2_000, 13.1), row("c", 1_000, 15, { loaded: false })];

function show(props: Partial<Parameters<typeof RunsTable>[0]> = {}) {
  const onTick = vi.fn();
  const onShow = vi.fn();
  act(() =>
    root.render(
      <RunsTable
        rows={rows}
        shownId="a"
        drawnIds={[]}
        colorOf={() => "#123456"}
        reading={new Set()}
        onTick={onTick}
        onShow={onShow}
        {...props}
      />,
    ),
  );
  return { onTick, onShow };
}
const bodyRows = () => [...host.querySelectorAll("tbody tr")];
const heading = (text: string) =>
  [...host.querySelectorAll<HTMLElement>("th")].find((th) => th.textContent?.startsWith(text))!;
const tick = (i: number) => bodyRows()[i].querySelector<HTMLInputElement>("input[type=checkbox]")!;

test("a click on a heading sorts by it, a second click the other way", () => {
  show();
  expect(heading("Ran").getAttribute("aria-sort")).toBe("descending");
  const ids = () => bodyRows().map((tr) => tr.querySelector("button")!.getAttribute("aria-label"));
  expect(ids()).toEqual([expect.stringContaining("Heavier"), expect.any(String), expect.any(String)]);
  act(() => heading("Consumption").querySelector("button")!.click());
  expect(heading("Consumption").getAttribute("aria-sort")).toBe("descending");
  expect(bodyRows().map((tr) => tr.lastElementChild!.textContent)).toEqual(["15", "14.2", "13.1"]);
  act(() => heading("Consumption").querySelector("button")!.click());
  expect(heading("Consumption").getAttribute("aria-sort")).toBe("ascending");
  expect(bodyRows().map((tr) => tr.lastElementChild!.textContent)).toEqual(["13.1", "14.2", "15"]);
  expect(heading("Ran").getAttribute("aria-sort")).toBe("none");
});

test("the run shown is ticked; a tick on another draws it, a click on its row shows it", () => {
  const { onTick, onShow } = show();
  expect(tick(0).checked).toBe(true);
  // alone, the run shown cannot be unticked
  expect(tick(0).disabled).toBe(true);
  expect(bodyRows()[0].getAttribute("aria-current")).toBe("true");
  act(() => tick(2).click());
  expect(onTick).toHaveBeenCalledWith(rows[2], true);
  expect(onShow).not.toHaveBeenCalled();
  act(() => (bodyRows()[1].querySelectorAll("td")[2] as HTMLElement).click());
  expect(onShow).toHaveBeenCalledWith(rows[1]);
  // the keyboard's way: the button in the Ran cell
  act(() => bodyRows()[2].querySelector("button")!.click());
  expect(onShow).toHaveBeenLastCalledWith(rows[2]);
});

test("with a run drawn, the run shown can be unticked and the drawn run has its colour", () => {
  const { onTick } = show({ drawnIds: ["b"] });
  expect(tick(1).checked).toBe(true);
  expect(tick(0).disabled).toBe(false);
  act(() => tick(0).click());
  expect(onTick).toHaveBeenCalledWith(rows[0], false);
  const dot = bodyRows()[1].querySelector<HTMLElement>("[aria-hidden]")!;
  expect(dot.style.background).toBe("rgb(18, 52, 86)");
});

test("a run being read says so and cannot be ticked meanwhile; no Name column without names", () => {
  show({ rows: rows.map((r) => ({ ...r, name: undefined })), reading: new Set(["c"]) });
  expect(heading("Name")).toBeUndefined();
  expect(tick(2).disabled).toBe(true);
  expect(bodyRows()[2].textContent).toContain("reading…");
});
