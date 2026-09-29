// RES-19: the Results page keeps the user's plot choices (ticked channels,
// view, x axis, zoom) when they leave it, edit the model, run again and
// reload, and draws the previous run of the case, the default baseline, as
// a faint line under the new one.
import { expect, test, type Page } from "@playwright/test";
import { openApp, openSummary, ribbonTab, runActiveCase, runButton, selectElement, xRange } from "./app";

// the Results page is the absolutely positioned overlay beside the Home dock
const results = (page: Page) => page.locator(".ss-zoom.absolute");
const channel = (page: Page, name: RegExp) => results(page).getByRole("checkbox", { name });
const viewButton = (page: Page, name: string) => results(page).getByRole("button", { name, exact: true });
const faint = (page: Page) => results(page).getByRole("checkbox", { name: "Draw the baseline faint on the chart" });
const chart = (page: Page) => results(page).getByRole("img", { name: /^Results chart: / });
const legend = (page: Page, name: string) => results(page).locator(".u-legend").getByText(name, { exact: true });

async function expectPicks(page: Page) {
  await expect(channel(page, /^Discharge Power/)).not.toBeChecked();
  await expect(channel(page, /^Terminal Voltage/)).toBeChecked();
  await expect(channel(page, /^SOC/)).toBeChecked();
}

test("RES-19: plot choices outlast Home, an edit, a new run and a reload; the previous run is drawn faint", async ({ page }) => {
  await openApp(page);
  await runActiveCase(page);
  await channel(page, /^Discharge Power/).uncheck();
  await channel(page, /^Terminal Voltage/).check();
  await results(page).getByRole("combobox", { name: "X axis" }).selectOption("min");
  await chart(page).focus();
  await page.keyboard.press("+");
  const zoomed = await xRange(chart(page));
  expect(zoomed).toEqual([1, 9]); // minutes of the 10-minute City Cycle
  await viewButton(page, "Table").click();

  // Home -> edit -> run -> Results
  await ribbonTab(page, "Home").click();
  await selectElement(page, "Vehicle");
  const mass = page.locator("tr", { hasText: "Vehicle Mass" }).locator("input");
  await mass.fill(String(Number(await mass.inputValue()) + 400));
  await mass.press("Tab");
  await runActiveCase(page);
  await expectPicks(page);
  await expect(viewButton(page, "Table")).toHaveAttribute("aria-pressed", "true");

  // the chart as it was left: the x axis and the zoom, and the first run
  // drawn faint under the new one, its numbers beside the new ones
  await viewButton(page, "Chart").click();
  await expect(results(page).getByRole("combobox", { name: "X axis" })).toHaveValue("min");
  expect(await xRange(chart(page))).toEqual(zoomed);
  await expect(faint(page)).toBeChecked();
  await expect(legend(page, "baseline · SOC")).toBeAttached();
  await expect(legend(page, "baseline · Terminal Voltage")).toBeAttached();
  const summary = await openSummary(page);
  await expect(summary.getByText("Baseline", { exact: true })).toBeVisible();

  // switching the faint line off is kept as well
  await faint(page).uncheck();
  await expect(legend(page, "baseline · SOC")).toHaveCount(0);
  await page.reload();
  await ribbonTab(page, "Results").click();
  await expectPicks(page);
  await expect(viewButton(page, "Chart")).toHaveAttribute("aria-pressed", "true");
  await expect(faint(page)).not.toBeChecked();
  expect(await xRange(chart(page))).toEqual(zoomed);

  // a double-click shows the whole run, and that is kept too
  const box = (await chart(page).locator(".u-over").boundingBox())!;
  await page.mouse.dblclick(box.x + box.width / 2, box.y + box.height / 2);
  await expect.poll(() => xRange(chart(page))).toEqual([0, 10]);
  await viewButton(page, "Table").click();
  await viewButton(page, "Chart").click();
  expect(await xRange(chart(page))).toEqual([0, 10]);

  // a zoom late in the run survives a run started from the Results page,
  // whose first points do not reach it yet
  await chart(page).focus();
  for (const key of ["+", "+", "+", ...Array(10).fill("ArrowRight")]) await page.keyboard.press(key);
  const late = await xRange(chart(page));
  expect(late[0]).toBeGreaterThan(4);
  await runButton(page).click();
  await expect(page.getByText("3 stored runs", { exact: true })).toBeVisible({ timeout: 60_000 });
  await expect.poll(() => xRange(chart(page))).toEqual(late);
});
