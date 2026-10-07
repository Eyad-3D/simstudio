// RES-18: each y axis fits its data (plus 5 %, at round ends), so SOC moving
// from 90 to 88.8 % fills the plot instead of lying flat on 0-100; a unit's
// axis can start at 0 or take set ends; the time axis reads in s, min or h,
// or the plot runs against the distance driven, and the CSV follows it;
// read-outs keep 4 significant digits; a sweep opens on its consumption.
import { readFile } from "node:fs/promises";
import { expect, test, type Page } from "@playwright/test";
import { openApp, ribbonTab, runActiveCase, showPanel, xRange, yRange } from "./app";
import { importProject } from "./ui-helpers";

/** Export the CSV and read it: its header and its rows as numbers. */
async function csv(page: Page): Promise<{ header: string[]; rows: number[][] }> {
  const [download] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("button", { name: "CSV", exact: true }).click(),
  ]);
  const [head, ...lines] = (await readFile(await download.path(), "utf8")).trim().split("\n");
  return { header: head.split(","), rows: lines.map((l) => l.split(",").map(Number)) };
}

test("RES-18: SOC and voltage fill the plot, and read-outs are rounded", async ({ page }) => {
  await openApp(page);
  await runActiveCase(page);
  await page.getByPlaceholder("Search channels…").fill("Terminal Voltage");
  await page.getByRole("checkbox", { name: /^Terminal Voltage/ }).check();
  await page.getByPlaceholder("Search channels…").fill("");
  const chart = page.getByRole("img", { name: /^Results chart:/ });
  await expect(chart).toHaveAttribute("aria-label", /Terminal Voltage/);

  // each trace spans at least 60 % of its axis (0.012 and 0.019 on a 0-based axis)
  const { header, rows } = await csv(page);
  const span = (name: string) => {
    const col = header.indexOf(name);
    const vs = rows.map((r) => r[col]);
    return Math.max(...vs) - Math.min(...vs);
  };
  const share = async (unit: string, name: string) => {
    const [lo, hi] = await yRange(chart, unit);
    return span(name) / (hi - lo);
  };
  expect(await share("%", "HV Battery Pack · SOC [%]")).toBeGreaterThanOrEqual(0.6);
  expect(await share("V", "HV Battery Pack · Terminal Voltage [V]")).toBeGreaterThanOrEqual(0.6);

  // the values under the pointer: time to the 1 s step, others to 4 digits
  const plot = (await chart.locator(".u-over").boundingBox())!;
  await page.mouse.move(plot.x + plot.width / 2, plot.y + plot.height / 2);
  const values = chart.locator(".u-legend .u-value");
  await expect(values.first()).toHaveText(/^\d[\d,]* s$/);
  for (const text of await values.allTextContents())
    for (const n of text.match(/-?[\d,]*\.\d+/g) ?? [])
      expect(n.replace(/[-,.]/g, "").replace(/^0+/, "").length, `${n} in "${text}"`).toBeLessThanOrEqual(4);

  // the Axes menu: the % axis from 0, the V axis up to 400, then automatic again
  const auto = await yRange(chart, "V");
  await page.getByRole("button", { name: "Axes" }).click();
  const menu = page.getByRole("group", { name: "Y axes" });
  await menu.getByRole("group", { name: "%", exact: true }).getByRole("checkbox", { name: "Start at 0" }).check();
  await expect.poll(async () => (await yRange(chart, "%"))[0]).toBe(0);
  await menu.getByLabel("V axis maximum").fill("400");
  await expect.poll(async () => (await yRange(chart, "V"))[1]).toBe(400);
  // a minimum above the maximum is marked and ignored
  await menu.getByLabel("V axis minimum").fill("500");
  await expect(menu.getByLabel("V axis minimum")).toHaveAttribute("aria-invalid", "true");
  await expect(menu.getByLabel("V axis minimum")).toHaveAccessibleDescription(/minimum must be below the maximum/);
  await expect.poll(() => yRange(chart, "V")).toEqual(auto);
  await menu.getByRole("group", { name: "V", exact: true }).getByRole("button", { name: "Auto" }).click();
  await expect(menu.getByLabel("V axis minimum")).toHaveValue("");
  // a maximum below all the data is kept, the axis the right way up
  await menu.getByLabel("V axis maximum").fill("300");
  await expect.poll(async () => (await yRange(chart, "V"))[1]).toBe(300);
  expect((await yRange(chart, "V"))[0]).toBeLessThan(300);
  await expect(page.getByRole("button", { name: "Axes (2 set)" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
});

test("RES-18: a WLTC run plots against km, and the CSV follows the axis", async ({ page }) => {
  await openApp(page);
  await page.getByTitle("Active simulation case").selectOption("case-wltc");
  await runActiveCase(page);
  const chart = page.getByRole("img", { name: /^Results chart:/ });
  await expect(chart).toHaveAttribute("aria-label", /; t \[s\] 0 to 1,800$/);

  await page.getByLabel("X axis").selectOption("min");
  await expect(chart).toHaveAttribute("aria-label", /; t \[min\] 0 to 30$/);

  await page.getByLabel("X axis").selectOption("distance");
  await expect(chart).toHaveAttribute("aria-label", /; Distance \[km\] 0 to [\d.]+$/);
  expect((await xRange(chart))[1]).toBeCloseTo(23.267, 2);
  // the read-out gives the distance with the sample's time
  const plot = (await chart.locator(".u-over").boundingBox())!;
  await page.mouse.move(plot.x + plot.width / 2, plot.y + plot.height / 2);
  await expect(chart.locator(".u-legend .u-value").first()).toHaveText(/^\d+(\.\d\d?)? km \(t = [\d,]+ s\)$/);

  const { header, rows } = await csv(page);
  expect(header.slice(0, 2)).toEqual(["distance_km", "t_s"]);
  expect(rows.at(-1)![0]).toBeCloseTo(23.267, 2);
  expect(rows.at(-1)![1]).toBe(1800);
  // the table (which reads in s) exports t_s
  await page.getByRole("button", { name: "Table", exact: true }).click();
  expect((await csv(page)).header[0]).toBe("t_s");
});

test("RES-18: the sweep opens on consumption, on an axis that fits", async ({ page }) => {
  const n = Date.now() % 100000;
  await openApp(page);
  const example = await (await page.request.get("/api/examples/bev-car")).json();
  example.cases[0].duration = 120; // long enough (over 100 m) for a Consumption row
  await importProject(page, { ...example, id: `e2e-res18-${n}`, name: `E2E axes ${n}` });

  await ribbonTab(page, "Home").click();
  await showPanel(page, "Cases & Parameters");
  const sweepElement = page.locator("select", { has: page.locator("option", { hasText: "Element…" }) }).nth(1);
  await sweepElement.selectOption({ label: "Vehicle" });
  await page.locator("input[type=number][max='200']").fill("3");
  await page.getByRole("button", { name: "Run sweep (3)" }).click();
  await expect(page.getByPlaceholder("Search channels…")).toBeVisible({ timeout: 60_000 });
  await page.getByRole("button", { name: "Sweep", exact: true }).click();

  await expect(page.getByLabel("Sweep metric")).toHaveValue("Consumption");
  const chart = page.getByRole("img", { name: /^Sweep chart:/ });
  const [lo] = await yRange(chart, "kWh/100km");
  expect(lo).toBeGreaterThan(0);
});
