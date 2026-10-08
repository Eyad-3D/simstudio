import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { DataCheck, SimRun } from "../types";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { StatusBar } from "./StatusBar";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("counts the errors in the Problems list, not the error lines in the log, and leads to them", async () => {
  const host = document.body.appendChild(document.createElement("div"));
  await act(async () => createRoot(host).render(<StatusBar />));
  const count = () => host.textContent?.match(/\d+ error(\(s\)|s)?/)?.[0] ?? null;
  const checked = (dataChecks: DataCheck[]) => act(() => useProjectStore.setState({ dataChecks }));
  const error: DataCheck = { level: "error", text: "Port 'pos' is not connected." };

  act(() => useProjectStore.setState({ messages: [{ time: "09:30:00", level: "error", text: "Data checks: 1 error(s)." }] }));
  expect(count()).toBeNull(); // nothing checked yet
  checked([error, error, { level: "warning", text: "Brake has no Brake Command signal." }]);
  expect(count()).toBe("2 errors");
  checked([error]);
  expect(count()).toBe("1 error");

  const focusPanel = vi.fn();
  useUIStore.setState({ ribbonTab: "results", focusPanel });
  act(() => host.querySelector("button[title='Show the problems']")!.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  expect(focusPanel).toHaveBeenCalledWith("data-checks");
  expect(useUIStore.getState().ribbonTab).toBe("home");

  checked([]); // the model checks clean again
  expect(count()).toBeNull();

  // the latest run's errors are problems too, until the next run
  const failed: SimRun = {
    id: "r",
    caseId: "c",
    caseName: "City",
    startedAt: 0,
    status: "failed",
    result: { caseId: "c", status: "failed", channels: [], summary: [], messages: [{ level: "error", text: "Stopped." }] },
  };
  act(() => useProjectStore.setState({ runs: [failed] }));
  expect(count()).toBe("1 error");
  act(() => useProjectStore.setState({ runs: [{ ...failed, id: "r2", status: "success", result: { ...failed.result, messages: [] } }, failed] }));
  expect(count()).toBeNull();
});

it("says how a run ended, with its button and the setting to open Results by itself (UX-21)", async () => {
  const host = document.body.appendChild(document.createElement("div"));
  await act(async () => createRoot(host).render(<StatusBar />));
  const notice = () => host.querySelector("[aria-label='Run finished']");
  const button = (name: string) =>
    [...(notice()?.querySelectorAll("button") ?? [])].find((b) => b.textContent === name) ?? null;
  const click = (el: Element | null) => act(() => el!.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  act(() => useUIStore.setState({ ribbonTab: "home", resultsAfterRun: false }));
  expect(notice()).toBeNull();

  act(() =>
    useProjectStore.setState({
      finishNotice: { seq: 1, level: "info", text: "Sweep finished: 3 of 3 points complete.", show: "results", studyId: "sweep-1" },
    }),
  );
  expect(notice()?.getAttribute("role")).toBe("status");
  expect(notice()?.textContent).toContain("Sweep finished: 3 of 3 points complete.");
  expect(button("Study charts")).not.toBeNull();
  // the setting, kept
  const always = notice()!.querySelector<HTMLInputElement>("input[type=checkbox]")!;
  click(always);
  expect(useUIStore.getState().resultsAfterRun).toBe(true);
  expect(localStorage.getItem("lightsim-results-after-run")).toBe("1");
  click(button("Show results"));
  expect(useUIStore.getState().ribbonTab).toBe("results");
  expect(notice()).toBeNull();

  // a failure is an alert, and leads to Messages
  const focusPanel = vi.fn();
  act(() => {
    useUIStore.setState({ focusPanel });
    useProjectStore.setState({ finishNotice: { seq: 2, level: "error", text: "'City' failed: see Messages.", show: "messages" } });
  });
  expect(notice()?.getAttribute("role")).toBe("alert");
  expect(button("Show results")).toBeNull();
  expect(notice()!.querySelector("input[type=checkbox]")).toBeNull();
  click(button("Show messages"));
  expect(useUIStore.getState().ribbonTab).toBe("home");
  expect(focusPanel).toHaveBeenCalledWith("messages");

  // a running sweep: points done and the time left
  act(() =>
    useProjectStore.setState({
      running: true,
      sweepProgress: { total: 4, done: 1, startedAt: Date.now() - 10_000, lastAt: Date.now() },
    }),
  );
  expect(host.textContent).toMatch(/sweep: 1 of 4 points done · about 30 s left/);
  act(() => useProjectStore.setState({ running: false, sweepProgress: null }));
});
