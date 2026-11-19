// AI-30: Project → Copy for AI puts a short summary of the model and its
// last run on the clipboard; AI-29: Connect AI adds LightSim to an AI app in
// one click (the test engine writes the AI apps' settings to a temp folder).
import { expect, test } from "@playwright/test";
import { logLines, openApp, ribbonTab, runActiveCase } from "./app";

test("AI-30: Copy for AI copies the model and its last run, with values hidden on request", async ({
  page,
  context,
}) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await openApp(page);
  await runActiveCase(page);

  await ribbonTab(page, "Project").click();
  await page.getByRole("button", { name: "Copy for AI" }).click();
  await expect(page.getByRole("status").filter({ hasText: /^Copied \(\d+\.\d KB\)$/ })).toBeVisible();
  const text = await page.evaluate(() => navigator.clipboard.readText());
  expect(text).toContain("# LightSim model");
  expect(text).toContain("## Parts and wiring");
  expect(text).toContain("## Last run");
  expect(text).toMatch(/final SOC: \d+(\.\d+)? %/);
  expect(text).toContain("not validated");
  expect(new TextEncoder().encode(text).length).toBeLessThan(8 * 1024);
  await expect(await logLines(page, /Copy for AI: a .* KB summary .* LightSim sent nothing/)).toBeVisible();

  await ribbonTab(page, "Project").click();
  await page.getByLabel("Hide values").check();
  await page.getByRole("button", { name: /Copy for AI|Copied/ }).click();
  await expect
    .poll(() => page.evaluate(() => navigator.clipboard.readText()))
    .toContain("Numbers are hidden ([hidden])");
  const hidden = await page.evaluate(() => navigator.clipboard.readText());
  expect(hidden).not.toMatch(/final SOC: \d/);
});

test("AI-29: Connect AI adds LightSim to an AI app with one click", async ({ page }) => {
  await openApp(page);
  await ribbonTab(page, "Project").click();
  await page.getByRole("button", { name: "Connect AI" }).click();
  const dialog = page.getByRole("dialog", { name: "Connect an AI assistant" });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByTestId("ai-last-used")).toHaveText("No assistant has used LightSim yet.");
  const row = dialog.getByRole("row").filter({ hasText: "Claude Desktop" });
  await row.getByRole("button", { name: "Add" }).click();
  await expect(row).toContainText("Connected");
  await row.getByRole("button", { name: "Remove" }).click();
  await expect(row.getByRole("button", { name: "Add" })).toBeVisible();
  await dialog.getByRole("button", { name: "Close" }).click();
  await expect(dialog).toHaveCount(0);
});
