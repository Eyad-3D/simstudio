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
  expect(host.textContent).toContain("Trusted projects (Python package only)None.");

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
  // off refuses every tool call, but the connection still serves its skill pack and part library
  expect(host.querySelector("[data-testid=ai-access-state]")!.textContent).toBe(
    "AI access is off: every tool call from an AI tool is refused, though a connected AI app can still read the skill pack and the part library that come with LightSim.",
  );
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

// The page says what the connection does today (AI-01, step 6): it asks before
// an edit and before a Script run, but an answer sent again is not asked about
// again and an app on the newer protocol may answer for you; it does not read
// the trust list, and the trust list belongs to the Python package.
// backend/tests/test_ai_claims.py holds the same facts against the code and
// the help pages.
it("says what the connection for AI apps does today", async () => {
  await render(base);
  const text = (host.textContent ?? "").replace(/\s+/g, " ");
  expect(text).toContain("An AI app you connected reads these settings before every tool call.");
  expect(text).toContain("Python code that uses lightsim.ai_access.AgentSession reads them when it starts a session.");
  expect(text).toContain(
    "Reading, checking and comparing models, and running a project without Script blocks: no question.",
  );
  expect(text).toContain("Saving an edit: LightSim asks you first.");
  expect(text).toContain("Running a project with Script blocks (Python code): LightSim asks you first");
  expect(text).toContain("the question does not show the code");
  expect(text).toContain(
    "An app that sends your answer again repeats a run, or saves another copy of an edited example or new project, without asking you.",
  );
  expect(text).toContain("An AI app on MCP's older handshake that cannot ask you is refused anything that needs your yes");
  expect(text).toContain(
    "LightSim does not check whether the app can ask, so an app that answers for you can save edits and run Script projects.",
  );
  expect(text).toContain("Connect only an AI app that shows you each question.");
  expect(text).toContain(
    "On Windows such a project does not run at all unless the connection was started with --trust-scripts",
  );
  expect(text).toContain("LightSim also refuses Script code you have not approved there (Run scripts)");
  expect(text).toContain("LightSim cannot tell who answered");
  expect(text).toContain("Only Python code that uses lightsim.ai_access.AgentSession reads it");
  expect(text).toContain("The connection for AI apps does not read this list.");
  // the claims it used to make
  expect(text).not.toContain("only once you trust it");
  expect(text).not.toContain("enforced by the engine");
  expect(text).not.toContain("hold for every AI tool");
  expect(text).not.toContain("each time");
  expect(text).not.toContain("before each run");
  expect(text).not.toContain("cannot ask you cannot save");
  expect(text).not.toContain("before every request");
});
