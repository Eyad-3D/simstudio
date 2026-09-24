import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { DataCheck } from "../types";
import { useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { StatusBar } from "./StatusBar";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("counts the errors the latest Data Checks found, not the error lines in the log, and leads to them", async () => {
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
  act(() => host.querySelector("button[title='Show the Data Checks']")!.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  expect(focusPanel).toHaveBeenCalledWith("data-checks");
  expect(useUIStore.getState().ribbonTab).toBe("home");

  checked([]); // the model checks clean again
  expect(count()).toBeNull();
});
