// STD-35: a logged lap (a MoTeC-like CSV with three laps) imports into a
// runnable cycle case in a few clicks: Import lap, the layout, the file,
// Add as case; the case then runs on the FS example.
import { expect, test } from "@playwright/test";
import { openApp, openFromMenu, openSummary, ribbonTab, runButton, showResults } from "./app";

function motecCsv(): string {
  const lines = ['"Format","MoTeC CSV File"', "", '"Time","Lap Number","Ground Speed"', '"s","","km/h"'];
  let t = 0;
  for (const [lap, dur] of [
    [0, 20],
    [1, 15],
    [2, 18],
  ]) {
    const n = Math.round(dur / 0.1);
    for (let i = 0; i < n; i++) {
      lines.push(`${t.toFixed(2)},${lap},${(20 + 50 * Math.sin((Math.PI * i) / n)).toFixed(2)}`);
      t += 0.1;
    }
  }
  return lines.join("\n");
}

test("STD-35: a logged lap imports into a runnable cycle case in under a minute", async ({ page }) => {
  await openApp(page);
  await openFromMenu(page, "FS Electric (generic)");
  const t0 = Date.now();
  await ribbonTab(page, "Simulations").click();
  await page.getByRole("button", { name: "Import lap" }).click();
  const dialog = page.getByRole("dialog", { name: "Import a lap" });
  await dialog.locator("label", { hasText: /^Layout/ }).locator("select").selectOption("MoTeC i2 CSV");
  await dialog.getByLabel("Lap file").setInputFiles({
    name: "session.csv",
    mimeType: "text/csv",
    buffer: Buffer.from(motecCsv()),
  });
  // the fastest full lap: lap 1 (lap 0 is the out lap, lap 2 the in lap)
  await expect(dialog.getByText(/^Lap 1: 1[45]\.\d s/)).toBeVisible();
  await expect(dialog.getByRole("img", { name: /Speed of the imported lap/ })).toBeVisible();
  await dialog.getByRole("button", { name: "Add as case" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByTitle("Active simulation case")).toContainText("session");
  expect(Date.now() - t0).toBeLessThan(60_000);

  await runButton(page).click();
  await showResults(page);
  await openSummary(page);
  await expect(page.getByRole("row", { name: /^Accumulator — energy delivered/ })).toBeVisible({ timeout: 30_000 });
  await expect(page.getByText(/Cycle not followed/)).toHaveCount(0);
});
