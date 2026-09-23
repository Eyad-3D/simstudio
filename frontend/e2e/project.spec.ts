// Project lifecycle: New → add a component → Save → reopen, and the recovery
// draft that brings unsaved work back after a reload.
import { expect, test, type Page } from "@playwright/test";
import {
  dragComponent,
  expectProject,
  logLines,
  openApp,
  openFromMenu,
  ribbonTab,
} from "./app";

/** Start a new project with a unique name and one Constant block on it. */
async function newProjectWithConstant(page: Page, name: string): Promise<void> {
  await ribbonTab(page, "Home").click();
  await page.getByRole("button", { name: "New", exact: true }).click();
  await expect(page.locator(".react-flow__node")).toHaveCount(0);
  await expectProject(page, "New Project", { unsaved: false });

  await ribbonTab(page, "Project").click();
  await page.getByText("Project name").locator("xpath=following-sibling::input").fill(name);
  await ribbonTab(page, "Home").click();

  await dragComponent(page, "Constant", { x: 240, y: 160 });
  await expect(page.locator(".react-flow__node")).toHaveCount(1);
  await expect(page.locator(".react-flow__node")).toContainText("Constant 1");
  await expectProject(page, name, { unsaved: true });
}

test("New project → add a component → save → reopen", async ({ page }) => {
  const name = `E2E project ${Date.now() % 100000}`;
  await openApp(page);
  await newProjectWithConstant(page, name);

  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expectProject(page, name, { unsaved: false });
  await expect(await logLines(page, `Project '${name}' saved to the server.`)).toBeVisible();

  // switch away, then reopen it from the Open menu
  await openFromMenu(page, "Battery Electric Car");
  await expectProject(page, "Battery Electric Car", { unsaved: false });
  await expect(page.locator(".react-flow__node", { hasText: "Constant 1" })).toHaveCount(0);

  await openFromMenu(page, name);
  await expectProject(page, name, { unsaved: false });
  await expect(page.locator(".react-flow__node")).toHaveCount(1);
  await expect(page.locator(".react-flow__node")).toContainText("Constant 1");

  // and it really is on disk
  const list = await (await page.request.get("/api/projects")).json();
  const saved = list.find((p: { name: string }) => p.name === name);
  expect(saved).toBeTruthy();
  const project = await (await page.request.get(`/api/projects/${saved.id}`)).json();
  const labels = project.systems.flatMap((s: { elements: { label: string }[] }) =>
    s.elements.map((e) => e.label),
  );
  expect(labels).toEqual(["Constant 1"]);
});

test("reload brings back unsaved work, but not after it was saved", async ({ page }) => {
  const name = `E2E draft ${Date.now() % 100000}`;
  await openApp(page);
  await newProjectWithConstant(page, name);

  // unsaved: the recovery draft (written shortly after the last edit) restores it
  await expect
    .poll(() =>
      page.evaluate(() => {
        const draft = JSON.parse(localStorage.getItem("simstudio-draft-v1") ?? "null");
        return draft && `${draft.project.name} clean=${draft.clean}`;
      }),
    )
    .toBe(`${name} clean=false`);
  await page.reload();
  await expectProject(page, name, { unsaved: true });
  await expect(page.locator(".react-flow__node")).toHaveCount(1);
  await expect(await logLines(page, /Restored your unsaved draft/)).toBeVisible();

  // saved: the next launch opens the saved file, clean, without a "restored" note
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expectProject(page, name, { unsaved: false });
  await page.reload();
  await expectProject(page, name, { unsaved: false });
  await expect(page.locator(".react-flow__node")).toHaveCount(1);
  await expect(await logLines(page, `Project '${name}' opened.`)).toBeVisible();
  await expect(await logLines(page, /Restored your unsaved draft/)).toHaveCount(0);
});
