// AI-01: the AI access tab of Connect AI. The engine's routes are mocked; the
// browser test (e2e/ai01-access.spec.ts) runs them for real.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { AiAccess } from "../api";

vi.mock("../api", () => ({ aiAccess: vi.fn(), changeAiAccess: vi.fn(), allowAiFolder: vi.fn() }));
const api = vi.mocked(await import("../api"));
const { AiAccessPanel } = await import("./AiAccessPanel");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const base: AiAccess = {
  enabled: true,
  on: true,
  managed: null,
  folders: [{ path: "/home/me/LightSim", exists: true, projects: true }],
  projectsFolder: "/home/me/LightSim",
  examples: true,
  trusted: [{ path: "/home/me/LightSim/hybrid.json", exists: true, name: "Hybrid", current: false }],
  maxRunSeconds: 300,
  settingsPath: "/home/me/.config/LightSim/ai-access.json",
  audit: [{ time: 1_700_000_000, tool: "run_case", outcome: "ok", project: "example:bev-car", client: "app", via: "mcp" }],
};

let host: HTMLDivElement;
beforeEach(() => {
  host = document.body.appendChild(document.createElement("div"));
});
afterEach(() => {
  host.remove();
  delete window.lightsimDesktop;
});

async function render(state: AiAccess) {
  api.aiAccess.mockResolvedValue(state);
  await act(async () => createRoot(host).render(<AiAccessPanel />));
}
const button = (name: string) =>
  [...host.querySelectorAll("button")].find((b) => (b.getAttribute("aria-label") ?? b.textContent) === name)!;
const box = (text: string) =>
  [...host.querySelectorAll("label")].find((l) => l.textContent?.includes(text))!.querySelector("input")!;
const click = (el: Element) => act(async () => el.dispatchEvent(new MouseEvent("click", { bubbles: true })));

it("shows the settings and takes trust and folders away", async () => {
  await render(base);
  expect(host.textContent).toContain("AI access is on");
  expect(host.textContent).toContain("/home/me/LightSim (your projects folder)");
  expect(host.textContent).toContain("Hybrid /home/me/LightSim/hybrid.json (its scripts changed since: not trusted now)");
  expect(host.textContent).toContain("run_case by app");
  expect(box("Stop an AI tool's run after").value).toBe("300");

  api.changeAiAccess.mockResolvedValue({ ...base, trusted: [] });
  await click(button("Untrust Hybrid"));
  expect(api.changeAiAccess).toHaveBeenCalledWith({ untrust: ["/home/me/LightSim/hybrid.json"] });
  expect(host.textContent).toContain("Projects whose Script blocks AI tools may runNone.");

  api.changeAiAccess.mockResolvedValue({ ...base, folders: [] });
  await click(button("Remove /home/me/LightSim"));
  expect(api.changeAiAccess).toHaveBeenLastCalledWith({ removeFolders: ["/home/me/LightSim"] });
  expect(host.textContent).toContain("None: AI tools see no project of yours.");
  // there is no way to trust a project here
  expect(host.textContent).not.toMatch(/\bTrust\b/);
});

it("cannot turn AI access on when the organisation's policy keeps it off", async () => {
  await render({ ...base, enabled: false, on: false, managed: "Your organisation's policy turns AI access off." });
  expect(host.querySelector("[data-testid=ai-access-managed]")!.textContent).toBe(
    "Your organisation's policy turns AI access off. AI access cannot be turned on here.",
  );
  expect(box("Let AI tools use LightSim").disabled).toBe(true);
  expect(box("Let AI tools use LightSim").checked).toBe(false);
  expect(host.textContent).toContain("AI access is off");
});

it("adds a folder with the desktop app's folder dialog", async () => {
  const picked = { ...base, folders: [...base.folders, { path: "/team", exists: true, projects: false }] };
  const allowAiFolder = vi.fn().mockResolvedValue(picked);
  window.lightsimDesktop = { allowAiFolder } as unknown as typeof window.lightsimDesktop;
  await render(base);
  expect(host.querySelector("[aria-label='Folder to allow']")).toBeNull(); // no path typed in the desktop app
  await click(button("Add folder…"));
  expect(allowAiFolder).toHaveBeenCalledOnce();
  expect(host.textContent).toContain("/team");
  expect(api.allowAiFolder).not.toHaveBeenCalled();
});

it("shows a refusal and the settings as they are", async () => {
  await render({ ...base, enabled: false, on: false });
  api.changeAiAccess.mockRejectedValue(new Error("403 Your organisation's policy turns AI access off."));
  await click(box("Let AI tools use LightSim"));
  expect(host.textContent).toContain("403 Your organisation's policy turns AI access off.");
  expect(box("Let AI tools use LightSim").checked).toBe(false); // read again after the refusal
});
