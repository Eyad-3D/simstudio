import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import type { SimRun } from "../../types";
import { RunInfo } from "./RunInfo";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("lists the live edits a run took, one it did not make marked with its note", async () => {
  const note = "'Ambient.temperature_C' changed — ... it takes effect on the next run [NEW-LIVE-NEXT-RUN].";
  const run = {
    id: "r",
    caseId: "c",
    caseName: "City",
    startedAt: 0,
    status: "success",
    result: { caseId: "c", status: "success", messages: [], channels: [], summary: [] },
    snapshot: {
      project: { id: "p", name: "Car", systems: [{ id: "s", elements: [] }] },
      case: { id: "c", name: "City", kind: "cycle", duration: 600, timeStep: 0.01 },
      appVersion: "0.4.0",
      liveEdits: [
        { t: 205.7, elementId: "el-vehicle", key: "cd", value: 0.35 },
        { t: 205.7, elementId: "el-ambient", key: "temperature_C", value: 65, note },
      ],
    },
  } as unknown as SimRun;
  const host = document.body.appendChild(document.createElement("div"));
  await act(async () => createRoot(host).render(<RunInfo run={run} />));
  const items = [...host.querySelectorAll("li")];
  expect(items.map((li) => li.textContent)).toEqual([
    "t ≈ 205.7 s: el-vehicle · cd = 0.35",
    "t ≈ 205.7 s: el-ambient · temperature_C = 65 (not made: on the next run)",
  ]);
  expect(items[1].title).toBe(note);
  expect(items[0].title).toBe("");
});
