// STD-09, STD-10, STD-36: results out for MATLAB, tables in from CSV and
// Excel files, and the whole parameter set out to one spreadsheet and back.
import { readFile } from "node:fs/promises";
import { expect, test } from "@playwright/test";
import { openApp, ribbonTab, runActiveCase, selectElement } from "./app";

test("STD-10: a table imports from a CSV file with a preview, converted units and one undo", async ({ page }) => {
  await openApp(page);
  await selectElement(page, "HV Battery Pack");
  await page.getByRole("button", { name: /Open-Circuit Voltage/ }).first().click();
  const dialog = page.locator("[data-param='ocv_table']");
  await expect(dialog).toBeVisible();

  // a file with a text cell is refused with its row and cell
  const input = page.getByLabel("Table file to import").first();
  await input.setInputFiles({
    name: "bad.csv",
    mimeType: "text/csv",
    buffer: Buffer.from("SOC [%];OCV [kV]\n0;0,3\n50;abc\n100;0,4\n"),
  });
  const preview = page.getByRole("dialog", { name: /Import .* from bad\.csv/ });
  await expect(preview.getByText("Row 3: 'abc' in cell B3")).toBeVisible();
  await expect(preview.getByRole("button", { name: /^Apply/ })).toBeDisabled();
  await preview.getByRole("button", { name: "Cancel", exact: true }).click();

  // a good one: semicolons, decimal commas and kV, stored in V
  await input.setInputFiles({
    name: "ocv.csv",
    mimeType: "text/csv",
    buffer: Buffer.from("SOC [%];OCV [kV]\n0;0,3\n50;0,35\n100;0,4\n"),
  });
  const good = page.getByRole("dialog", { name: /from ocv\.csv/ });
  await expect(good.getByText("decimal comma", { exact: false })).toBeVisible();
  await good.getByRole("button", { name: "Apply (3 points)" }).click();
  await expect(good).toBeHidden();
  await expect(dialog.getByRole("row", { name: "50 350" })).toBeVisible();
  await page.keyboard.press("Escape");
  await page.keyboard.press("Control+z");
  await page.getByRole("button", { name: /Open-Circuit Voltage/ }).first().click();
  await expect(page.locator("[data-param='ocv_table']").getByRole("row").nth(1)).toBeVisible();
  await expect(page.locator("[data-param='ocv_table']").getByRole("row", { name: "50 350" })).toHaveCount(0);
});

test("STD-09: the Results page saves the run as a MATLAB .mat file", async ({ page }) => {
  await openApp(page);
  await runActiveCase(page);
  const [download] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("button", { name: "MATLAB", exact: true }).click(),
  ]);
  expect(download.suggestedFilename()).toMatch(/\.mat$/);
  const mat = await readFile(await download.path());
  expect(mat.subarray(0, 19).toString()).toBe("MATLAB 5.0 MAT-file");
  expect(mat.subarray(126, 128).toString()).toBe("IM");
});

test("STD-36: the parameter sheet goes out and comes back with a list of changes", async ({ page }) => {
  await openApp(page);
  await ribbonTab(page, "Parameters").click();
  const [download] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("button", { name: "Export CSV", exact: true }).click(),
  ]);
  expect(download.suggestedFilename()).toMatch(/parameters\.csv$/);
  const csv = (await readFile(await download.path())).toString("utf-8");
  expect(csv.charCodeAt(0)).toBe(0xfeff);
  // change the vehicle's mass in the sheet
  const lines = csv.split("\r\n");
  const i = lines.findIndex((l) => l.includes(",mass_kg,"));
  expect(i).toBeGreaterThan(0);
  const cells = lines[i].split(",");
  const old = cells[5];
  cells[5] = "2345";
  lines[i] = cells.join(",");
  await page.getByLabel("Parameter sheet to import").setInputFiles({
    name: "edited.csv",
    mimeType: "text/csv",
    buffer: Buffer.from(lines.join("\r\n")),
  });
  const dialog = page.getByRole("dialog", { name: "Import parameters from edited.csv" });
  await expect(dialog.getByText(/1 change,/)).toBeVisible();
  await expect(dialog.getByRole("row", { name: new RegExp(`Vehicle Mass .*${old}.*2345`) })).toBeVisible();
  await dialog.getByRole("button", { name: "Apply 1 change" }).click();
  await expect(dialog).toBeHidden();

  // the same file again: nothing left to change
  await page.getByLabel("Parameter sheet to import").setInputFiles({
    name: "edited.csv",
    mimeType: "text/csv",
    buffer: Buffer.from(lines.join("\r\n")),
  });
  const again = page.getByRole("dialog", { name: "Import parameters from edited.csv" });
  await expect(again.getByText(/0 changes,/)).toBeVisible();
  await again.getByRole("button", { name: "Close", exact: true }).click();
});
