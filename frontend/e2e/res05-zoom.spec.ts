// RES-05: zoom and pan the charts. The wheel zooms around the pointer, a
// dragged box zooms in, Shift+drag pans, double-click (or 0) resets, on the
// Results chart, the X-Y view and the Signal Plot; ticking a channel keeps the
// zoom; the gestures land where they are made at any interface size, and a
// zoom holds while a live run adds samples. (The redraw time on a 1-hour run
// is a probe in the release notes, not a test: it depends on the machine.)
import { expect, test, type Locator, type Page } from "@playwright/test";
import { openApp, ribbonTab, runActiveCase, showPanel, xRange } from "./app";

test.use({ viewport: { width: 1600, height: 900 } });

const width = ([a, b]: [number, number]) => b - a;

async function zoomPanReset(page: Page, chart: Locator, { xy = false } = {}) {
  await expect(chart).toBeVisible();
  const full = await xRange(chart);
  const plot = (await chart.locator(".u-over").boundingBox())!;
  const y = plot.y + plot.height / 2;
  const dy = plot.height / 5;
  const at = (f: number) => plot.x + plot.width * f;

  // the wheel over the first quarter zooms in there; a double-click resets
  await page.mouse.move(at(0.25), y);
  await page.mouse.wheel(0, -200);
  await expect.poll(async () => width(await xRange(chart))).toBeLessThan(0.9 * width(full));
  await page.mouse.dblclick(at(0.5), y);
  await expect.poll(() => xRange(chart)).toEqual(full);

  // a box from 25 % to 50 % of the plot shows that quarter
  await page.mouse.move(at(0.25), y - dy);
  await page.mouse.down();
  await page.mouse.move(at(0.5), xy ? y + dy : y - dy, { steps: 5 });
  await page.mouse.up();
  const boxed = await xRange(chart);
  const q = width(full) / 4;
  expect(boxed[0]).toBeGreaterThan(full[0] + 0.8 * q);
  expect(boxed[1]).toBeLessThan(full[0] + 2.2 * q);

  // Shift+drag to the left shows later values at the same width
  await page.keyboard.down("Shift");
  await page.mouse.move(at(0.6), y);
  await page.mouse.down();
  await page.mouse.move(at(0.4), y, { steps: 5 });
  await page.mouse.up();
  await page.keyboard.up("Shift");
  const panned = await xRange(chart);
  expect(panned[0]).toBeGreaterThan(boxed[0]);
  expect(width(panned)).toBeCloseTo(width(boxed), 3);

  // on the focused chart + zooms in and 0 resets
  await chart.focus();
  await page.keyboard.press("+");
  await expect.poll(async () => width(await xRange(chart))).toBeLessThan(width(panned));
  await page.keyboard.press("0");
  await expect.poll(() => xRange(chart)).toEqual(full);
}

test("RES-05: zoom, pan and reset on the Results chart, X-Y view and Signal Plot", async ({ page }) => {
  await openApp(page);
  await runActiveCase(page);
  const chart = page.getByRole("img", { name: /^Results chart:/ });
  await zoomPanReset(page, chart);

  // ticking another channel keeps the zoomed range
  const plot = (await chart.locator(".u-over").boundingBox())!;
  await page.mouse.move(plot.x + plot.width / 2, plot.y + plot.height / 2);
  await page.mouse.wheel(0, -300);
  const zoomed = await xRange(chart);
  expect(width(zoomed)).toBeLessThan(600);
  await page.getByRole("checkbox", { name: /^Vehicle Speed\b/ }).first().check();
  await expect(chart).toHaveAttribute("aria-label", /Vehicle Speed/);
  expect(await xRange(chart)).toEqual(zoomed);

  // X-Y: the box and the wheel zoom both axes; hover reads the nearest point
  await page.getByRole("button", { name: "X-Y", exact: true }).click();
  const xy = page.getByRole("img", { name: /^X-Y chart:/ });
  await zoomPanReset(page, xy, { xy: true });
  const box = (await xy.locator(".u-over").boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await expect(xy.locator(".u-legend .u-value").first()).toHaveText(/\d.* → .*\d/);

  await ribbonTab(page, "Home").click();
  await showPanel(page, "Signal Plot");
  await zoomPanReset(page, page.getByRole("img", { name: /^Signal Plot:/ }));
});

test("RES-05: at 125 % interface size a zoom box shows what was boxed", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("lightsim-font-scale", "1.25"));
  await openApp(page);
  await runActiveCase(page);
  const chart = page.getByRole("img", { name: /^Results chart:/ });
  const full = await xRange(chart);
  const plot = (await chart.locator(".u-over").boundingBox())!;
  const y = plot.y + plot.height / 2;
  await page.mouse.move(plot.x + plot.width * 0.25, y);
  await page.mouse.down();
  await page.mouse.move(plot.x + plot.width * 0.5, y, { steps: 5 });
  await page.mouse.up();
  const [lo, hi] = await xRange(chart);
  const w = width(full);
  expect(Math.abs(lo - (full[0] + 0.25 * w))).toBeLessThan(0.02 * w);
  expect(Math.abs(hi - (full[0] + 0.5 * w))).toBeLessThan(0.02 * w);
});

test("RES-05: a zoom made during a live run holds while samples arrive", async ({ page }) => {
  await openApp(page);
  await showPanel(page, "Cases & Parameters");
  await page.locator("label", { hasText: "Pacing" }).locator("select").selectOption("1");
  await page.getByTitle(/^Run the active case/).click();
  await ribbonTab(page, "Results").click();
  const chart = page.getByRole("img", { name: /^Results chart:/ });
  await expect.poll(async () => (await xRange(chart))[1], { timeout: 20_000 }).toBeGreaterThan(4);
  const plot = (await chart.locator(".u-over").boundingBox())!;
  const y = plot.y + plot.height / 2;
  await page.mouse.move(plot.x + plot.width * 0.1, y);
  await page.mouse.down();
  const before = await xRange(chart);
  for (let i = 1; i <= 20; i++) {
    // a slow drag, across several live updates (every 120 ms)
    await page.mouse.move(plot.x + plot.width * (0.1 + i * 0.01), y);
    await page.waitForTimeout(40);
  }
  await page.mouse.up();
  const zoomed = await xRange(chart);
  expect(width(zoomed)).toBeLessThan(0.5 * width(before));
  await page.waitForTimeout(1500);
  expect(await xRange(chart)).toEqual(zoomed);
  await page.getByTitle("Stop the running simulation", { exact: true }).click();
});
