// AI-04 (step 6): an AI agent's edit through the MCP tool shows in the open
// window as it happens, with no reopen and no refused save, and one click
// undoes it; the window and an agent editing at the same time lose nothing.
//
// The agent is the real MCP server (python -m app.ai, as an AI app starts
// it) on the test engine's projects folder, sent one tools/call per edit.
import { expect, test, type Page } from "@playwright/test";
import { execFile } from "node:child_process";
import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { expectProject, openApp, ribbonTab } from "./app";

const backend = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "backend");
const python = process.env.LIGHTSIM_PYTHON || (process.platform === "win32" ? "python" : "python3");

interface Access {
  projectsFolder: string;
  settingsPath: string;
}
let access: Access | null = null;
let settingsBefore: string | null = null;

// AI access goes back to how it was, and the audit log the agent's calls
// wrote goes (other tests expect no assistant to have used LightSim)
test.afterEach(() => {
  if (!access) return;
  if (settingsBefore === null) rmSync(access.settingsPath, { force: true });
  else writeFileSync(access.settingsPath, settingsBefore);
  rmSync(join(access.projectsFolder, ".ai"), { recursive: true, force: true });
  access = null;
});

/** Turn AI access on for the test engine's projects folder. */
async function allowAgents(page: Page): Promise<Access> {
  access = (await (await page.request.get("/api/ai/access")).json()) as Access;
  try {
    settingsBefore = readFileSync(access.settingsPath, "utf8");
  } catch {
    settingsBefore = null;
  }
  writeFileSync(
    access.settingsPath,
    JSON.stringify({ version: 1, enabled: true, examples: true, folders: [access.projectsFolder] }),
  );
  return access;
}

/** One call of an MCP tool, as an AI app makes it (MCP 2026-07-28). */
async function agent(folder: string, name: string, args: Record<string, unknown>) {
  const request = {
    jsonrpc: "2.0",
    id: 1,
    method: "tools/call",
    params: {
      name,
      arguments: args,
      _meta: {
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": { name: "e2e-agent", version: "1" },
      },
    },
  };
  const child = promisify(execFile)(python, ["-m", "app.ai", "--projects-dir", folder], {
    cwd: backend,
    env: { ...process.env, LIGHTSIM_AI_SETTINGS: access!.settingsPath },
    timeout: 60_000,
  });
  child.child.stdin!.end(JSON.stringify(request) + "\n");
  const { stdout } = await child;
  const reply = stdout
    .split("\n")
    .filter((l) => l.trim())
    .map((l) => JSON.parse(l))
    .find((m) => m.id === 1);
  expect(reply?.result?.isError, JSON.stringify(reply)).toBeFalsy();
  return reply.result.structuredContent;
}

/** Save a copy of the BEV example as a project of the user's and open it. */
async function openSavedCopy(page: Page, name: string): Promise<string> {
  const example = await (await page.request.get("/api/examples/bev-car")).json();
  const id = `e2e-live-${Date.now() % 1_000_000}`;
  const put = await page.request.put(`/api/projects/${id}`, {
    data: { ...example, id, name },
    headers: { "If-None-Match": "*" },
  });
  expect(put.ok()).toBe(true);
  const opened = await page.evaluate(
    (pid) => (window as unknown as { lightsimOpenProjectId(id: string): Promise<boolean> }).lightsimOpenProjectId(pid),
    id,
  );
  expect(opened).toBe(true);
  await expectProject(page, name, { unsaved: false });
  return id;
}

type Doc = { name: string; systems: { elements: { label: string }[] }[] };
async function onDisk(page: Page, id: string): Promise<Doc> {
  return (await page.request.get(`/api/projects/${id}`)).json();
}
const labels = (doc: Doc) => doc.systems.flatMap((s) => s.elements.map((e) => e.label));
const addBrake = (id: string, label: string, reason: string) => ({
  project: id,
  operations: [{ op: "add", type: "mech.brake", label }],
  dry_run: false,
  reason,
});

test("AI-04: an agent's edit through the MCP tool shows in the open window, and one click undoes it", async ({
  page,
}) => {
  await openApp(page);
  const { projectsFolder } = await allowAgents(page);
  const name = `E2E live ${Date.now() % 100000}`;
  const id = await openSavedCopy(page, name);

  const done = await agent(projectsFolder, "model_edit", addBrake(id, "Brake by agent", "add a parking brake"));
  expect(done.applied).toBe(true);

  // the open window shows it, with no reopen and no dialog, and stays saved
  const brake = page.locator(".react-flow__node").filter({ hasText: "Brake by agent" });
  await expect(brake).toBeVisible();
  const notice = page.getByLabel("AI change");
  await expect(notice).toContainText("e2e-agent: add a parking brake");
  await expectProject(page, name, { unsaved: false });
  await expect(page.getByText("Project changed on disk")).toHaveCount(0);

  // one click undoes it, in the file and in the window
  await notice.getByRole("button", { name: "Undo", exact: true }).click();
  await expect(brake).toHaveCount(0);
  await expect(notice).toHaveCount(0);
  expect(labels(await onDisk(page, id))).not.toContain("Brake by agent");
  const history = await (await page.request.get(`/api/projects/${id}/history`)).json();
  expect(history[0]).toMatchObject({ tool: "undo" });
  expect(history[1]).toMatchObject({ by: "agent", client: "e2e-agent", reason: "add a parking brake", undone: true });
});

test("AI-04: the window and an agent edit at the same time and neither change is lost", async ({ page }) => {
  await openApp(page);
  const { projectsFolder } = await allowAgents(page);
  const name = `E2E both ${Date.now() % 100000}`;
  const id = await openSavedCopy(page, name);
  const rename = async (to: string) => {
    await ribbonTab(page, "Project").click();
    await page.getByText("Project name").locator("xpath=following-sibling::input").fill(to);
    await ribbonTab(page, "Home").click();
    await expectProject(page, to, { unsaved: true });
  };

  // the window has an unsaved change; the agent saves one of its own
  await rename(`${name} v2`);
  await agent(projectsFolder, "model_edit", addBrake(id, "Brake one", "first brake"));
  await expect(page.locator(".react-flow__node").filter({ hasText: "Brake one" })).toBeVisible();
  await expectProject(page, `${name} v2`, { unsaved: true }); // the window's change is still there
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expectProject(page, `${name} v2`, { unsaved: false });
  let disk = await onDisk(page, id);
  expect(disk.name).toBe(`${name} v2`);
  expect(labels(disk)).toContain("Brake one");

  // the window saves straight after the agent, before it has looked at the
  // file: the save is refused, the agent's save laid over it, and saved again
  await rename(`${name} v3`);
  await agent(projectsFolder, "model_edit", addBrake(id, "Brake two", "second brake"));
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expectProject(page, `${name} v3`, { unsaved: false });
  await expect(page.locator(".react-flow__node").filter({ hasText: "Brake two" })).toBeVisible();
  await expect(page.getByText("Project changed on disk")).toHaveCount(0);
  disk = await onDisk(page, id);
  expect(disk.name).toBe(`${name} v3`);
  expect(labels(disk)).toEqual(expect.arrayContaining(["Brake one", "Brake two"]));
});
