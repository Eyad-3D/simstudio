import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import type { TableImport } from "../api";
import { Preview } from "./ImportTableDialog";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("previews a map that is wider than it is tall the right way round", async () => {
  // 5 speeds across (the outer axis), 2 torques down (the inner one); the
  // engine sends values[row][col]
  const map: TableImport = {
    kind: "table2d",
    ok: true,
    value: null,
    points: 10,
    range: "A1:F3",
    sheet: "Sheet1",
    sheets: [],
    cells: [],
    columns: [],
    xColumn: null,
    yColumn: null,
    transpose: false,
    units: {},
    axes: [
      { name: "Speed", unit: "1/min" },
      { name: "Torque", unit: "N·m" },
    ],
    valueName: "Power loss",
    valueUnit: "kW",
    notes: [],
    errors: [],
    warnings: [],
    preview: {
      cols: [1000, 2000, 3000, 4000, 5000],
      rows: [10, 20],
      values: [
        [0.1, 0.2, 0.3, 0.4, 0.5],
        [1.1, 1.2, 1.3, 1.4, 1.5],
      ],
    },
    target: "motor.emotor/power_loss",
  };
  const host = document.body.appendChild(document.createElement("div"));
  await act(async () => createRoot(host).render(<Preview result={map} />));
  const rows = [...host.querySelectorAll("tbody tr")].map((tr) =>
    [...tr.querySelectorAll("th, td")].map((c) => c.textContent),
  );
  expect(rows).toEqual([
    ["10", "0.1", "0.2", "0.3", "0.4", "0.5"],
    ["20", "1.1", "1.2", "1.3", "1.4", "1.5"],
  ]);
  const head = [...host.querySelectorAll("thead th")].map((c) => c.textContent);
  expect(head).toEqual(["Torque \\ Speed", "1,000", "2,000", "3,000", "4,000", "5,000"]);
});
