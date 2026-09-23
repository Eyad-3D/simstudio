// PLT-29: every save keeps the version it replaces; Project → Restore lists
// them and opens one as an unsaved copy, never over the project file.
import { expect, test, type Page } from "@playwright/test";
import { expectProject, logLines, openApp, ribbonTab } from "./app";

/** Rename the open project (Project tab) and save it (Home tab). */
async function renameAndSave(page: Page, name: string): Promise<void> {
  await ribbonTab(page, "Project").click();
  await page.getByText("Project name").locator("xpath=following-sibling::input").fill(name);
  await ribbonTab(page, "Home").click();
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expectProject(page, name, { unsaved: false });
}

test("PLT-29: an earlier version opens as an unsaved copy and the file stays as it is", async ({ page }) => {
  const name = `E2E versions ${Date.now() % 100000}`;
  await openApp(page);
  await ribbonTab(page, "Home").click();
  await page.getByRole("button", { name: "New", exact: true }).click();
  await renameAndSave(page, `${name} v1`);
  await renameAndSave(page, `${name} v2`);
  await renameAndSave(page, `${name} v3`);
  const projects: { id: string; name: string }[] = await (await page.request.get("/api/projects")).json();
  const { id } = projects.find((p) => p.name === `${name} v3`)!;

  // the two versions the saves replaced, newest first; the current one is the file
  await ribbonTab(page, "Project").click();
  await page.getByRole("button", { name: "Restore…" }).click();
  const versions = page.getByRole("menu", { name: "Earlier versions" }).getByRole("menuitem");
  await expect(versions).toHaveCount(2);
  await expect(versions.nth(0)).toContainText(`${name} v2`);
  await expect(versions.nth(1)).toContainText(`${name} v1`);

  await versions.nth(1).click();
  await expect(page.getByText(new RegExp(`^${name} v1 \\(version of .+\\) •$`))).toBeVisible();
  await expect(await logLines(page, /as an unsaved copy/)).toBeVisible();
  const onDisk = await (await page.request.get(`/api/projects/${id}`)).json();
  expect(onDisk.name, "restoring did not write the project file").toBe(`${name} v3`);

  // saving the copy makes a new project; the original keeps its file
  await ribbonTab(page, "Home").click();
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByText(new RegExp(`^${name} v1 \\(version of .+\\)$`)).first()).toBeVisible();
  const after: { id: string; name: string }[] = await (await page.request.get("/api/projects")).json();
  expect(after.find((p) => p.id === id)?.name).toBe(`${name} v3`);
  expect(after.filter((p) => p.name.startsWith(`${name} v1 (version of`))).toHaveLength(1);
});
