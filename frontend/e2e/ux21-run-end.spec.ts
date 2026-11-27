// UX-21: a run leaves the page where the user is. A small notice says how
// it ended and offers its results; "Open after each run", on the Results
// page's ribbon, opens the Results page by itself, and is kept.
import { expect, test } from "@playwright/test";
import { finishNotice, openApp, ribbonTab, runButton, showPanel } from "./app";
import { importProject } from "./ui-helpers";

test("UX-21: a run keeps the page and ends with a notice that opens its results", async ({ page }) => {
  const n = Date.now() % 100000;
  await openApp(page);
  const example = await (await page.request.get("/api/examples/bev-car")).json();
  example.cases[0].duration = 20;
  await importProject(page, { ...example, id: `e2e-ux21-${n}`, name: `E2E run end ${n}` });
  await ribbonTab(page, "Home").click();
  await showPanel(page, "Cases & Parameters");

  await runButton(page).click();
  const notice = finishNotice(page);
  await expect(notice).toContainText(/^'City Cycle' finished: success\./, { timeout: 60_000 });
  // still on the model: the diagram and the panel the user had open
  await expect(page.getByPlaceholder("Search channels…")).toHaveCount(0);
  await expect(page.locator(".react-flow__node").first()).toBeVisible();
  await expect(page.getByRole("button", { name: /^Run sweep/ })).toBeVisible();

  // the notice's button shows the run; there, the notice is put away
  await notice.getByRole("button", { name: "Show results" }).click();
  await expect(page.getByPlaceholder("Search channels…")).toBeVisible();
  await expect(page.getByText("1 stored run", { exact: true })).toBeVisible();
  await expect(notice).toHaveCount(0);

  // a run started on the Results page is drawn there: no notice
  await runButton(page).click();
  await expect(page.getByText("2 stored runs", { exact: true })).toBeVisible({ timeout: 60_000 });
  await expect(runButton(page)).toBeVisible();
  await expect(notice).toHaveCount(0);

  // the close button puts a notice away
  await ribbonTab(page, "Home").click();
  await runButton(page).click();
  await expect(notice).toBeVisible({ timeout: 60_000 });
  await notice.getByRole("button", { name: "Close" }).click();
  await expect(notice).toHaveCount(0);
});

test("UX-21: the Results page's Open after each run opens the page by itself, and is kept", async ({ page }) => {
  const n = Date.now() % 100000;
  await openApp(page);
  const example = await (await page.request.get("/api/examples/bev-car")).json();
  example.cases[0].duration = 20;
  await importProject(page, { ...example, id: `e2e-ux21b-${n}`, name: `E2E always ${n}` });

  // the setting is on the Results page's ribbon (it moved there from the
  // run's notice, the owner's choice B); the notice has none
  await ribbonTab(page, "Results").click();
  const always = page.getByRole("checkbox", { name: "Open after each run" });
  await expect(always).not.toBeChecked();
  await always.check();
  await ribbonTab(page, "Home").click();
  await page.reload();
  await ribbonTab(page, "Home").click();
  await runButton(page).click();
  await expect(page.getByPlaceholder("Search channels…")).toBeVisible({ timeout: 60_000 });
  await expect(finishNotice(page).getByRole("checkbox")).toHaveCount(0);
  await expect(always).toBeChecked();
  await always.uncheck();
  expect(await page.evaluate(() => localStorage.getItem("lightsim-results-after-run"))).toBe("0");
});
