// Defects a pass through the app as a user found in Results, the study
// views, the Signal Plot and the exports (app pass, 26 Nov), each checked
// where a person sees it.
import { expect, test } from "@playwright/test";
import { finishNotice, openApp, openFromMenu, runButton, runRow, showPanel } from "./app";

test("app pass #7: the Signal Plot names each channel with its part", async ({ page }) => {
  await openApp(page);
  await runButton(page).click();
  await expect(finishNotice(page)).toBeVisible({ timeout: 60_000 });
  const channel = page.locator("select[title='Channel to plot']");
  if (!(await channel.isVisible())) await page.locator(".dv-tab", { hasText: "Signal Plot" }).first().click();
  const names = await channel.locator("option").allInnerTexts();
  // five parts have a Brake Command: each is named with its part
  expect(names.filter((n) => n.endsWith("· Brake Command [-]"))).toHaveLength(5);
  expect(names).toContain("Brake RL · Brake Command [-]");
  expect(names.filter((n) => !n.includes(" · "))).toEqual([]);
});

test("app pass #8, #9, #10: an endurance study's charts, run names and DNF count", async ({ page }) => {
  test.setTimeout(240_000);
  await page.setViewportSize({ width: 1366, height: 768 });
  await openApp(page);
  await openFromMenu(page, "FS Electric (generic)");
  await showPanel(page, "Cases & Parameters");
  await page.getByLabel("Case", { exact: true }).selectOption({ label: "Endurance energy" });
  await page
    .locator("label", { hasText: /^FS event/ })
    .locator("select")
    .selectOption("endurance");
  const study = page.getByRole("region", { name: "Endurance energy study" });
  await study.locator("label", { hasText: /^Capacities/ }).locator("input").fill("2.5, 7.2");
  await study.locator("label", { hasText: /^Power limits/ }).locator("input").fill("30, 60");
  await study.getByRole("button", { name: /Run 2 × 2 = 4 runs/ }).click();

  // #10: the notice, the map and the Study view count the same DNF points
  const notice = finishNotice(page);
  await expect(notice).toContainText("Endurance energy study finished: ", { timeout: 180_000 });
  const dnf = Number(/(\d+) did not finish \(DNF\)/.exec(await notice.innerText())?.[1] ?? 0);
  expect(dnf).toBeGreaterThan(0); // a 2.5 kWh pack does not last
  await showPanel(page, "Cases & Parameters");
  await expect(page.getByRole("table", { name: "Endurance energy map" }).getByText(/DNF$/)).toHaveCount(dnf);
  await notice.getByRole("button", { name: "Study charts" }).click();
  await expect(page.getByText(new RegExp(`· ${dnf} did not finish \\(DNF\\)`))).toBeVisible();

  // #8: each chart keeps a plot clear of its legend, and a legend line is
  // the power cap's value, not its whole name
  const charts = page.getByRole("region", { name: "Study charts" }).locator("figure");
  await expect(charts.first().locator(".u-over")).toBeVisible();
  for (const chart of await charts.all()) {
    const plot = (await chart.locator(".u-over").boundingBox())!;
    const legend = (await chart.locator(".u-legend").boundingBox())!;
    expect(plot.height).toBeGreaterThan(100);
    expect(legend.y).toBeGreaterThanOrEqual(plot.y + plot.height - 1);
    await expect(chart.locator(".u-legend")).not.toContainText("Output Power Limit");
    await expect(chart.locator(".u-legend")).toContainText("30 kW");
  }

  // #9: a study run is named by its point
  await expect(runRow(page, 0).row).toContainText(/\d(\.\d)? kWh, \d+ kW/);
  await expect(page.getByRole("region", { name: "Runs" })).not.toContainText("null");
});
