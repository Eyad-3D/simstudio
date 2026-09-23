// The core flow on the bundled BEV example: open → Data Checks → run →
// Results lists channels → export CSV.
import { readFile } from "node:fs/promises";
import { expect, test } from "@playwright/test";
import { drawnLines, expectProject, openApp, ribbonTab, runActiveCase } from "./app";

test("BEV example: data checks, run, results channels and CSV export", async ({ page }) => {
  const pageErrors: string[] = [];
  page.on("pageerror", (e) => pageErrors.push(String(e)));

  await openApp(page);
  await expectProject(page, "Battery Electric Car", { unsaved: false });
  await expect(page.getByText("backend connected")).toBeVisible();

  // Data Checks: the shipped example must pass the run gate
  await ribbonTab(page, "Simulations").click();
  await page.getByRole("button", { name: "Checks", exact: true }).click();
  await expect(page.getByText(/^\d+ error\(s\), \d+ warning\(s\), \d+ info$/)).toBeVisible();
  await expect(page.getByText(/^0 error\(s\)/)).toBeVisible();

  // Run: finishes on the Results page with the run stored
  await runActiveCase(page);
  await expect(page.getByText("1 stored run", { exact: true })).toBeVisible();

  // Results lists the run's channels, some ticked and drawn by default
  const channels = page.getByRole("checkbox");
  expect(await channels.count()).toBeGreaterThan(10);
  const ticked = await page.getByRole("checkbox", { checked: true }).count();
  expect(ticked).toBeGreaterThan(0);
  await expect(drawnLines(page).first()).toBeVisible();

  // CSV export: a time column plus one column per ticked channel
  const [download] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("button", { name: "CSV", exact: true }).click(),
  ]);
  expect(download.suggestedFilename()).toMatch(/^simstudio-.+\.csv$/);
  const lines = (await readFile(await download.path(), "utf8")).trim().split("\n");
  const header = lines[0].split(",");
  expect(header[0]).toBe("t_s");
  expect(header).toHaveLength(ticked + 1);
  for (const column of header.slice(1)) expect(column).toMatch(/ \[.+\]$/); // "label [unit]"
  expect(lines.length).toBeGreaterThan(100);
  const first = lines[1].split(",").map(Number);
  expect(first).toHaveLength(header.length);
  expect(first.every(Number.isFinite)).toBe(true);

  expect(pageErrors).toEqual([]);
});
