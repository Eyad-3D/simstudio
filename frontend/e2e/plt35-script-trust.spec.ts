// PLT-35: a project whose scripts came from another computer shows them and
// asks before they run; nothing of theirs runs until the user says yes.
import { expect, test } from "@playwright/test";
import { logLines, openApp, ribbonTab, runActiveCase, runButton } from "./app";
import { importProject } from "./ui-helpers";

test("PLT-35: scripts from elsewhere never run before the user approves them", async ({ page }) => {
  const n = Date.now() % 100000;
  await openApp(page);
  const example = await (await page.request.get("/api/examples/hybrid-car")).json();
  example.cases = [{ ...example.cases[0], duration: 20, realtimeFactor: 0 }];
  const script = example.systems
    .flatMap((s: { elements: { componentDefId: string; label: string; parameterOverrides: Record<string, unknown> }[] }) => s.elements)
    .find((el: { componentDefId: string }) => el.componentDefId === "signal.script");
  const marker = `# changed on another computer ${n}`;
  script.parameterOverrides.code = `${script.parameterOverrides.code}\n${marker}\n`;
  const project = { ...example, id: `e2e-plt35-${n}`, name: `E2E script trust ${n}` };

  // opening it shows the code and offers to open without running it
  await importProject(page, project);
  const dialog = page.getByRole("dialog", { name: /contains 1 script you have not approved/ });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("figure").filter({ hasText: script.label })).toContainText(marker);
  await dialog.getByRole("button", { name: "Open without running scripts" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(await logLines(page, /Opened without running its scripts/)).toHaveCount(1);

  // Run asks again; Don't run runs nothing
  await runButton(page).click();
  const ask = page.getByRole("dialog", { name: "Run 1 script you have not approved?" });
  await expect(ask).toBeVisible();
  await ask.getByRole("button", { name: "Don't run" }).click();
  await expect(await logLines(page, "Run cancelled: its scripts are not approved.")).toHaveCount(1);
  await expect(page.getByPlaceholder("Search channels…")).toHaveCount(0);

  // the engine refuses the code by itself too, whatever the UI does
  const refused = await (
    await page.request.post("/api/simulate", { data: { project, caseId: project.cases[0].id } })
  ).json();
  expect(refused.status).toBe("failed");
  expect(refused.messages.map((m: { text: string }) => m.text).join(" ")).toContain("not been approved");

  // Run scripts: approved for this exact code, and the run goes ahead
  await runButton(page).click();
  await ask.getByRole("button", { name: "Run scripts" }).click();
  await expect(page.getByPlaceholder("Search channels…")).toBeVisible({ timeout: 60_000 });

  // the approval is remembered: opening the project again asks nothing
  await ribbonTab(page, "Home").click();
  await importProject(page, { ...project, id: `e2e-plt35-${n}-again` });
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(await logLines(page, /Opened without running its scripts/)).toHaveCount(1); // only the first time
  await runActiveCase(page);
});
