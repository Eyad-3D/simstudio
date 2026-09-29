// RES-06: two measurement cursors, A and B, on the Results chart. The
// Cursors button (or C) puts them at a quarter and three quarters of the
// view; a time typed into a field, the arrow keys or a dragged line moves
// them, always onto a stored sample. A table under the chart gives each
// plotted series' values at A and B and, between them, its minimum, maximum,
// mean, RMS and integral (kWh from kW). The lines follow zoom and the
// distance axis and go into the PNG; the cursors are kept per run.
import { readFile } from "node:fs/promises";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { openApp, runActiveCase, runButton, xRange } from "./app";

/** Export the CSV and read it: its header and its rows as numbers. */
async function csv(page: Page): Promise<{ header: string[]; rows: number[][] }> {
  const [download] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("button", { name: "CSV", exact: true }).click(),
  ]);
  const [head, ...lines] = (await readFile(await download.path(), "utf8")).trim().split("\n");
  return { header: head.split(","), rows: lines.map((l) => l.split(",").map(Number)) };
}

/** The share of the plot's height over which the pixel column at `frac` of
 *  its width (±2 px) is in the accent colour: a cursor line is (about) 1.
 *  (The canvas is transparent: the band between the lines is the accent at
 *  8 % opacity.) */
async function lineAt(chart: Locator, frac: number): Promise<number> {
  return chart.locator("canvas").evaluate((canvas: HTMLCanvasElement, f) => {
    const over = canvas.parentElement!.querySelector(".u-over")!;
    const c = canvas.getBoundingClientRect();
    const o = over.getBoundingClientRect();
    const k = canvas.width / c.width;
    const x = Math.round((o.left - c.left + f * o.width) * k);
    const top = Math.round((o.top - c.top) * k);
    const h = Math.floor(o.height * k);
    const px = canvas.getContext("2d")!.getImageData(x - 2, top, 5, h).data;
    const accent = getComputedStyle(document.documentElement).getPropertyValue("--ss-accent").trim();
    const rgb = [1, 3, 5].map((i) => parseInt(accent.slice(i, i + 2), 16));
    let rows = 0;
    for (let y = 0; y < h; y++)
      for (let dx = 0; dx < 5; dx++)
        if (px[(y * 5 + dx) * 4 + 3] === 255 && rgb.every((v, j) => Math.abs(px[(y * 5 + dx) * 4 + j] - v) <= 6)) {
          rows++;
          break;
        }
    return rows / h;
  }, frac);
}

/** Where a value sits in a chart's x range, as a share of its width. */
const share = ([lo, hi]: [number, number], v: number) => (v - lo) / (hi - lo);

/** The trapezoid rule over the CSV's rows from t0 to t1 s, in the column
 *  headed `name` (kW), in kWh. */
function kWh({ header, rows }: { header: string[]; rows: number[][] }, name: string, t0: number, t1: number) {
  const t = header.indexOf("t_s");
  const col = header.indexOf(name);
  let sum = 0;
  for (let i = 1; i < rows.length; i++)
    if (rows[i - 1][t] >= t0 && rows[i][t] <= t1) sum += ((rows[i - 1][col] + rows[i][col]) / 2) * (rows[i][t] - rows[i - 1][t]);
  return sum / 3600;
}

const field = (page: Page, name: "A" | "B") => page.getByRole("spinbutton", { name: `Cursor ${name} time [s]` });
const cells = (page: Page, row: RegExp) =>
  page.getByRole("region", { name: "Cursor measurements" }).getByRole("row", { name: row }).getByRole("cell");

test("RES-06: the energy between two typed times, and cursors that follow the chart", async ({ page }) => {
  await openApp(page);
  await runActiveCase(page);
  const chart = page.getByRole("img", { name: /^Results chart:/ });
  await expect(chart).toBeVisible();
  const cursors = page.getByRole("button", { name: "Cursors" });
  await expect(cursors).toHaveAttribute("aria-pressed", "false");

  // C puts A and B at a quarter and three quarters of the City Cycle's 600 s
  await page.keyboard.press("c");
  await expect(cursors).toHaveAttribute("aria-pressed", "true");
  await expect(field(page, "A")).toHaveValue("150");
  await expect(field(page, "B")).toHaveValue("450");
  expect(await lineAt(chart, 0.25)).toBeGreaterThan(0.9);
  expect(await lineAt(chart, 0.75)).toBeGreaterThan(0.9);
  expect(await lineAt(chart, 0.6)).toBeLessThan(0.2);

  // typed times land on the nearest sample (1 s apart)
  await field(page, "A").fill("150.4");
  await page.keyboard.press("Tab");
  await expect(field(page, "A")).toHaveValue("150");
  await field(page, "B").fill("300");
  await page.keyboard.press("Enter");
  await expect(page.getByText(/^Δt 150 s$/)).toBeVisible();
  expect(await lineAt(chart, 0.5)).toBeGreaterThan(0.9);

  // the battery's energy from 150 to 300 s, as the trapezoid rule gives it
  // from the exported samples
  const data = await csv(page);
  const power = data.header.find((h) => h.endsWith("· Discharge Power [kW]"))!;
  const integral = cells(page, /Discharge Power/).last();
  await expect(integral).toHaveText(/ kWh$/);
  expect(parseFloat(await integral.innerText())).toBeCloseTo(kWh(data, power, 150, 300), 3);

  // the arrow keys step one sample; the cursors stay through the table view
  await field(page, "A").press("ArrowUp");
  await expect(field(page, "A")).toHaveValue("151");
  await page.getByRole("button", { name: "Table", exact: true }).click();
  await expect(field(page, "A")).toHaveValue("151");
  await page.getByRole("button", { name: "Chart", exact: true }).click();
  await expect(field(page, "A")).toHaveValue("151");

  // zoomed in, the line stays on 151 s
  const plot = (await chart.locator(".u-over").boundingBox())!;
  await page.mouse.move(plot.x + plot.width * 0.3, plot.y + plot.height / 2);
  await page.mouse.wheel(0, -300);
  await expect.poll(async () => (await xRange(chart))[1] - (await xRange(chart))[0]).toBeLessThan(500);
  expect(await lineAt(chart, share(await xRange(chart), 151))).toBeGreaterThan(0.9);

  // against the distance driven, the line sits at A's distance
  await page.getByRole("combobox", { name: "X axis" }).selectOption("distance");
  await expect(chart).toHaveAttribute("aria-label", /Distance \[km\]/);
  const byDistance = await csv(page);
  const at151 = byDistance.rows.find((r) => r[byDistance.header.indexOf("t_s")] === 151)!;
  const km = at151[byDistance.header.indexOf("distance_km")];
  expect(await lineAt(chart, share(await xRange(chart), km))).toBeGreaterThan(0.9);

  // and it is in the PNG, drawn twice as large
  await page.getByRole("combobox", { name: "X axis" }).selectOption("auto");
  await expect(chart).toHaveAttribute("aria-label", /t \[s\] 0 to 600$/);
  const [download] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("button", { name: "PNG", exact: true }).click(),
  ]);
  const png = (await readFile(await download.path())).toString("base64");
  const box = await chart.locator("canvas").evaluate((canvas: HTMLCanvasElement) => {
    const c = canvas.getBoundingClientRect();
    const o = canvas.parentElement!.querySelector(".u-over")!.getBoundingClientRect();
    return { left: o.left - c.left, top: o.top - c.top, width: o.width, height: o.height };
  });
  const inPng = await page.evaluate(
    async ({ png, box, t }) => {
      const img = new Image();
      img.src = `data:image/png;base64,${png}`;
      await img.decode();
      const canvas = document.createElement("canvas");
      canvas.width = img.width;
      canvas.height = img.height;
      const ctx = canvas.getContext("2d")!;
      ctx.drawImage(img, 0, 0);
      const accent = getComputedStyle(document.documentElement).getPropertyValue("--ss-accent").trim();
      const rgb = [1, 3, 5].map((i) => parseInt(accent.slice(i, i + 2), 16));
      // the picture is the chart at twice its size
      const x = Math.round(2 * (box.left + (t / 600) * box.width));
      const h = Math.floor(2 * box.height);
      const px = ctx.getImageData(x - 4, Math.round(2 * box.top), 9, h).data;
      let rows = 0;
      for (let y = 0; y < h; y++)
        for (let dx = 0; dx < 9; dx++)
          if (rgb.every((v, j) => Math.abs(px[(y * 9 + dx) * 4 + j] - v) <= 6)) {
            rows++;
            break;
          }
      return rows / h;
    },
    { png, box, t: 151 },
  );
  expect(inPng).toBeGreaterThan(0.9);

  // C again takes them away
  await chart.focus();
  await page.keyboard.press("c");
  await expect(cursors).toHaveAttribute("aria-pressed", "false");
  await expect(page.getByRole("region", { name: "Cursor measurements" })).toHaveCount(0);
  expect(await lineAt(chart, 151 / 600)).toBeLessThan(0.2);
});

test("RES-06: dragging a cursor line moves it, not the zoom, at 125 % interface size", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("lightsim-font-scale", "1.25"));
  await openApp(page);
  await runActiveCase(page);
  const chart = page.getByRole("img", { name: /^Results chart:/ });
  await page.getByRole("button", { name: "Cursors" }).click();
  await expect(field(page, "A")).toHaveValue("150");
  const full = await xRange(chart);
  const plot = (await chart.locator(".u-over").boundingBox())!;
  const y = plot.y + plot.height / 2;
  const x = plot.x + plot.width * 0.25;

  // the pointer shows that line A can be dragged
  await page.mouse.move(x + 3, y);
  await expect(chart.locator(".u-over")).toHaveCSS("cursor", "ew-resize");
  await page.mouse.down();
  await page.mouse.move(x + 100, y, { steps: 5 });
  await page.mouse.up();
  const expected = 150 + (100 / plot.width) * (full[1] - full[0]);
  await expect.poll(async () => Number(await field(page, "A").inputValue())).toBeGreaterThan(expected - 2);
  expect(Number(await field(page, "A").inputValue())).toBeLessThan(expected + 2);
  expect(await xRange(chart)).toEqual(full);

  // a drag away from the lines still zooms
  await page.mouse.move(plot.x + plot.width * 0.6, y);
  await expect(chart.locator(".u-over")).not.toHaveCSS("cursor", "ew-resize");
  await page.mouse.down();
  await page.mouse.move(plot.x + plot.width * 0.9, y, { steps: 5 });
  await page.mouse.up();
  await expect.poll(() => xRange(chart)).not.toEqual(full);
});

test("RES-06: cursors per run, on overlays, time to reach, and a move in under 8 ms", async ({ page }) => {
  await openApp(page);
  await runActiveCase(page);
  await runButton(page).click();
  await expect(page.getByText("2 stored runs", { exact: true })).toBeVisible({ timeout: 60_000 });
  // the new run is the primary one
  const primary = page.getByRole("combobox", { name: "Primary run" });
  const second = await primary.inputValue();
  const ids = await primary.locator("option").evaluateAll((os) => os.map((o) => (o as HTMLOptionElement).value));
  const first = ids.find((id) => id !== second)!;

  await page.getByRole("button", { name: "Cursors" }).click();
  await field(page, "A").fill("100");
  await page.keyboard.press("Enter");

  // the first run overlaid: measured at the same times, a row for each series
  await page.getByRole("checkbox", { name: /^City Cycle ·/ }).check();
  await expect(field(page, "A")).toHaveValue("100");
  await expect(page.getByRole("region", { name: "Cursor measurements" }).getByRole("row")).toHaveCount(1 + 2 * 4);
  await page.getByRole("button", { name: "Clear", exact: true }).click();

  // each run has cursors of its own
  await primary.selectOption(first);
  await expect(page.getByRole("button", { name: "Cursors" })).toHaveAttribute("aria-pressed", "false");
  await expect(field(page, "A")).toHaveCount(0);
  await primary.selectOption(second);
  await expect(field(page, "A")).toHaveValue("100");

  // time to reach: 0 to 50 km/h puts A and B on the first samples there
  await page.getByRole("combobox", { name: "Time to reach" }).selectOption({ label: "Vehicle Speed" });
  await page.getByRole("spinbutton", { name: "from", exact: true }).fill("0");
  await page.getByRole("spinbutton", { name: "to", exact: true }).fill("50");
  await page.getByRole("button", { name: "Place A, B" }).click();
  await expect(field(page, "A")).toHaveValue("0");
  const speed = cells(page, /^Vehicle Speed/);
  await expect(speed.nth(2)).toHaveText("0");
  expect(parseFloat(await speed.nth(3).innerText())).toBeGreaterThanOrEqual(50);
  const b = Number(await field(page, "B").inputValue());
  await expect(page.getByText(/^Δt [\d.]+ s$/)).toHaveText(`Δt ${b} s`);
  await page.getByRole("spinbutton", { name: "to", exact: true }).fill("500");
  await page.getByRole("button", { name: "Place A, B" }).click();
  await expect(page.getByRole("status").filter({ hasText: "never" })).toHaveText(
    "Vehicle Speed never reaches 500 km/h after 0 km/h.",
  );

  // a move's work takes well under the item's 8 ms: React renders the table
  // in the key's event and uPlot redraws in a microtask after it (the next
  // task would also wait for the screen's next frame)
  await field(page, "A").focus();
  const median = await page.evaluate(async () => {
    const el = document.activeElement!;
    const times: number[] = [];
    for (let i = 0; i < 50; i++) {
      const t0 = performance.now();
      el.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true }));
      for (let m = 0; m < 5; m++) await Promise.resolve();
      times.push(performance.now() - t0);
      await new Promise((done) => setTimeout(done, 0));
    }
    return times.sort((x, y) => x - y)[25];
  });
  await expect(field(page, "A")).toHaveValue("50");
  expect(median).toBeLessThan(8);
});
