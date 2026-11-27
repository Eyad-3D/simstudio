// The table of runs and the difference between two (UI piece: research items
// 4.22 and 4.23): every stored run is a row with its main figures; a click
// on a heading sorts; a tick draws a run with the one shown; while exactly
// two are drawn a strip under the chart shows the second less the first.
import { expect, test, type Page } from "@playwright/test";
import { expectProject, openApp, ribbonTab, runActiveCase, runNames, runRow, selectElement } from "./app";
import { importProject } from "./ui-helpers";

const strip = (page: Page) => page.getByRole("img", { name: /^Difference chart: / });
const caption = (page: Page) => page.getByText(/^Difference: /);
const heading = (page: Page, name: string | RegExp) =>
  page.getByRole("region", { name: "Runs" }).getByRole("columnheader", { name });

async function runWithMass(page: Page, kg: string) {
  await ribbonTab(page, "Home").click();
  await selectElement(page, "Vehicle");
  const mass = page.locator("tr", { hasText: "Vehicle Mass" }).locator("input");
  await mass.fill(kg);
  await mass.press("Tab");
  await runActiveCase(page);
}

test("the table of runs sorts, draws ticked runs and shows the difference of two", async ({ page }) => {
  const n = Date.now() % 100000;
  await openApp(page);
  const example = await (await page.request.get("/api/examples/bev-car")).json();
  example.cases[0].duration = 60;
  await importProject(page, { ...example, id: `e2e-runs-${n}`, name: `E2E runs ${n}` });
  await expectProject(page, `E2E runs ${n}`, { unsaved: true });
  await runActiveCase(page);
  await runWithMass(page, "2300");
  await runWithMass(page, "1500");

  // a row per run, newest first, with its figures; the newest is shown
  expect(await runNames(page)).toEqual([
    "City Cycle · Vehicle Mass 1,500 kg",
    "City Cycle · Vehicle Mass 2,300 kg",
    expect.stringMatching(/^City Cycle · \d/),
  ]);
  await expect(runRow(page, 0).row).toHaveAttribute("aria-current", "true");
  await expect(heading(page, /^Consumption/)).toBeVisible();
  await expect(heading(page, /^Distance driven/)).toBeVisible();

  // sorted by consumption, high to low: the heaviest car first
  await heading(page, /^Consumption/).getByRole("button").click();
  await expect(heading(page, /^Consumption/)).toHaveAttribute("aria-sort", "descending");
  await expect(runRow(page, 0).show).toHaveAccessibleName(/^Show City Cycle · Vehicle Mass 2,300 kg · /);
  await heading(page, /^Consumption/).getByRole("button").click();
  await expect(heading(page, /^Consumption/)).toHaveAttribute("aria-sort", "ascending");
  await expect(runRow(page, 0).show).toHaveAccessibleName(/^Show City Cycle · Vehicle Mass 1,500 kg · /);

  // one run drawn: no strip, and no room kept for it
  await expect(strip(page)).toHaveCount(0);
  await expect(caption(page)).toHaveCount(0);

  // two drawn: the strip, the second less the first, read out with the chart
  const heavy = runRow(page, 2); // 2,300 kg, sorted last
  await heavy.tick.check();
  await expect(strip(page)).toBeVisible();
  await expect(caption(page)).toHaveText("Difference: Vehicle Mass 2,300 kg minus Vehicle Mass 1,500 kg");
  await expect(page.getByRole("img", { name: /^Results chart: / })).toBeVisible();

  // a third: no strip; back to two: the strip again
  const first = runRow(page, 1);
  await first.tick.check();
  await expect(strip(page)).toHaveCount(0);
  await first.tick.uncheck();
  await expect(strip(page)).toBeVisible();

  // a click on the drawn run shows it first: the difference turns round
  await heavy.show.click();
  await expect(heavy.row).toHaveAttribute("aria-current", "true");
  await expect(caption(page)).toHaveText("Difference: Vehicle Mass 1,500 kg minus Vehicle Mass 2,300 kg");

  // the strip zooms with the chart
  // (each chart's name ends with its x range: "…; t [s] 0 to 60")
  const chart = page.getByRole("img", { name: /^Results chart: / });
  const xText = async (img: typeof chart) => (await img.getAttribute("aria-label"))!.match(/(\S+ to \S+)$/)![1];
  const whole = await xText(chart);
  await chart.focus();
  await page.keyboard.press("+");
  await expect.poll(() => xText(chart)).not.toBe(whole);
  await expect.poll(async () => (await xText(strip(page))) === (await xText(chart))).toBe(true);

  // the X-Y view has no strip
  await page.getByRole("button", { name: "X-Y", exact: true }).click();
  await expect(strip(page)).toHaveCount(0);
});
