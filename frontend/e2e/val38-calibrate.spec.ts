// VAL-38: calibrate lap mode on one logged lap and check it on another.
// The two "logged" laps are LightSim's own (the FS example with 0.9 × its
// grip and 2.5 m² of downforce, with noise), made by backend/tests/
// test_calibrate.py; no team's log is bundled.
import { expect, test } from "@playwright/test";
import { fileURLToPath } from "node:url";
import { openApp, openFromMenu, ribbonTab } from "./app";

const fixture = (name: string) => fileURLToPath(new URL(`./fixtures/${name}`, import.meta.url));

test("VAL-38: a calibration on one logged lap predicts another within 5 %", async ({ page }) => {
  test.setTimeout(150_000);
  await openApp(page);
  await openFromMenu(page, "FS Electric (generic)");
  await ribbonTab(page, "Simulations").click();
  await page.getByRole("button", { name: "Calibrate lap" }).click();
  const dialog = page.getByRole("dialog", { name: "Calibrate lap mode" });
  await dialog.getByLabel("Calibration lap file").setInputFiles(fixture("val38-calibration-lap.csv"));
  await dialog.getByLabel("Check lap file").setInputFiles(fixture("val38-check-lap.csv"));
  await dialog.getByRole("button", { name: "Calibrate" }).click();
  const results = dialog.getByRole("table", { name: "Calibration results" });
  await expect(results).toBeVisible({ timeout: 120_000 });
  const check = results.getByRole("row", { name: /^Check \(blind\)/ });
  const pct = Number((await check.getByRole("cell").nth(2).innerText()).replace(/[^\d.-]/g, ""));
  expect(Math.abs(pct)).toBeLessThan(5);
  await dialog.getByRole("button", { name: "Apply to the model" }).click();
  await expect(dialog).toHaveCount(0);
});
