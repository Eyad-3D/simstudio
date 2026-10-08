// AI-01: Connect AI → AI access shows and changes the rules every AI tool
// works under, the settings `lightsim ai …` keeps (the test engine keeps
// them in its temp folder): the switch, the allowed folders, the examples,
// the trusted Script projects (which can only be untrusted here), the run
// time cap and the latest calls from the audit log.
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { openApp, ribbonTab } from "./app";

/** the audit log the test writes: other tests expect no assistant to have
 *  used LightSim, so it goes again at the end */
let log: string | null = null;
test.afterEach(() => {
  if (log) rmSync(log, { force: true });
});

async function settings(page: Page) {
  return (await page.request.get("/api/ai/access")).json();
}

test("AI-01: the AI access tab changes what AI tools may do", async ({ page }) => {
  await openApp(page);
  const { projectsFolder, settingsPath } = await settings(page);
  // a call an AI app made, and a project trusted on the command line
  log = join(projectsFolder, ".ai", "audit.jsonl");
  mkdirSync(join(projectsFolder, ".ai"), { recursive: true });
  writeFileSync(
    log,
    JSON.stringify({ t: Date.now() / 1000, tool: "run_case", project: "example:bev-car", ok: true, client: "e2e-app" }) +
      "\n",
  );
  await page.request.put("/api/ai/access", { data: { enabled: false } });
  const file = JSON.parse(readFileSync(settingsPath, "utf8"));
  file.trustedScripts = { [join(projectsFolder, "gone.json")]: "abc" };
  writeFileSync(settingsPath, JSON.stringify(file));

  await ribbonTab(page, "Project").click();
  await page.getByRole("button", { name: "Connect AI" }).click();
  const dialog = page.getByRole("dialog", { name: "Connect an AI assistant" });
  await expect(dialog.getByRole("tab", { name: "AI apps" })).toHaveAttribute("aria-selected", "true");
  await dialog.getByRole("tab", { name: "AI access" }).click();
  const access = dialog.getByTestId("ai-access");

  // the switch
  await expect(access.getByTestId("ai-access-state")).toHaveText(/^AI access is off/);
  await access.getByRole("checkbox", { name: "Let AI tools use LightSim" }).check();
  await expect(access.getByTestId("ai-access-state")).toHaveText(/^AI access is on/);
  expect((await settings(page)).on).toBe(true);

  // folders: remove the projects folder if it is there, add it again
  const folders = access.getByRole("list", { name: "Allowed folders" });
  if ((await settings(page)).folders.length) {
    for (const f of (await settings(page)).folders)
      await folders.getByRole("button", { name: `Remove ${f.path}` }).click();
  }
  await expect(folders).toHaveText("None: AI tools see no project of yours.");
  await access.getByLabel("Folder to allow").fill(projectsFolder);
  await access.getByRole("button", { name: "Add", exact: true }).click();
  await expect(folders.getByRole("listitem")).toHaveText(/\(your projects folder\)/);
  await expect(access.getByLabel("Folder to allow")).toHaveValue("");
  await access.getByLabel("Folder to allow").fill(join(projectsFolder, "no-such-folder"));
  await access.getByRole("button", { name: "Add", exact: true }).click();
  await expect(access.getByText(/^404 No folder/)).toBeVisible();

  // the examples and the run time cap
  await access.getByRole("checkbox", { name: /examples that come with LightSim/ }).uncheck();
  await expect.poll(async () => (await settings(page)).examples).toBe(false);
  await access.getByRole("checkbox", { name: /examples that come with LightSim/ }).check();
  const cap = access.getByLabel("Run time cap in seconds");
  await expect(cap).toHaveValue("300");
  await cap.fill("120");
  await cap.press("Enter");
  await expect.poll(async () => (await settings(page)).maxRunSeconds).toBe(120);
  await cap.fill("0");
  await cap.press("Enter");
  await expect(access.getByText("The run time cap is a number of seconds from 1 to 86400.")).toBeVisible();
  await expect(cap).toHaveValue("120");
  await cap.fill("300");
  await cap.press("Enter");
  await expect.poll(async () => (await settings(page)).maxRunSeconds).toBe(300);

  // a trusted project can be untrusted here (not trusted: that is the command line's)
  const trusted = access.getByRole("list", { name: "Trusted projects" });
  await expect(trusted.getByRole("listitem")).toHaveText(/gone\.json \(not found\)/);
  await trusted.getByRole("button", { name: /^Untrust / }).click();
  await expect(trusted).toHaveText("None.");
  expect((await settings(page)).trusted).toEqual([]);

  // the latest calls
  const calls = access.getByRole("table", { name: "Latest calls from AI tools" });
  await expect(calls.getByRole("row").first()).toContainText("run_case by e2e-app");
  await expect(calls.getByRole("row").first()).toContainText("example:bev-car");
  await expect(calls.getByRole("row").first()).toContainText("ok");

  // the switch off again
  await access.getByRole("checkbox", { name: "Let AI tools use LightSim" }).uncheck();
  await expect(access.getByTestId("ai-access-state")).toHaveText(/^AI access is off/);
  await dialog.getByRole("button", { name: "Close" }).click();
  await expect(dialog).toHaveCount(0);
});
