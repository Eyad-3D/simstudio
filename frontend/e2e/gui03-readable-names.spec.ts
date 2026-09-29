// GUI-03: part names stay readable (at least 11 px on screen) at any zoom, the
// diagram follows the tray opening and closing, and the zoom is shown with
// Fit / 50 / 100 / 200 % presets.
import { expect, test, type Page } from "@playwright/test";
import { canvasShare, dockTab, nodeLayout, openApp, zoomOf } from "./ui-helpers";

test.use({ viewport: { width: 1366, height: 768 } });

/** The smallest part name on screen in px, and how many names are not
 *  wholly inside the diagram. */
function names(page: Page): Promise<{ smallest: number; cutOff: number }> {
  return page.evaluate(() => {
    const pane = document.querySelector(".react-flow")!.getBoundingClientRect();
    const spans = [...document.querySelectorAll<HTMLElement>(".react-flow__node div.top-full span")];
    const px = spans.map((s) => parseFloat(getComputedStyle(s).fontSize) * (s.getBoundingClientRect().height / s.offsetHeight));
    const cutOff = spans
      .map((s) => s.getBoundingClientRect())
      .filter((r) => r.top < pane.top || r.bottom > pane.bottom || r.left < pane.left || r.right > pane.right).length;
    return { smallest: +Math.min(...px).toFixed(1), cutOff };
  });
}

test("GUI-03: the tray opening and closing keeps the model and its names in view", async ({ page }) => {
  await openApp(page);
  const fitted = await zoomOf(page);
  await dockTab(page, "Messages").click();
  await expect.poll(() => zoomOf(page), { message: "re-fitted to the smaller diagram" }).toBeLessThan(fitted!);
  expect((await nodeLayout(page)).offscreen).toBe(0);
  expect(await names(page)).toEqual({ smallest: 11, cutOff: 0 });
  expect((await canvasShare(page)).pct, "the diagram's share of the window").toBeGreaterThanOrEqual(40);
  await page.locator(".dv-edge-group button[aria-label='Collapse to tabs']").click();
  await expect.poll(() => zoomOf(page), { message: "back to the first fit" }).toBe(fitted);
});

test("GUI-03: the zoom readout shows the zoom and applies its presets", async ({ page }) => {
  await openApp(page);
  const zoom = page.getByLabel("Zoom", { exact: true });
  const shown = () => zoom.evaluate((s: HTMLSelectElement) => s.options[s.selectedIndex].text);
  await expect.poll(shown).toBe(`${Math.round((await zoomOf(page))! * 100)}%`);
  for (const [preset, value] of [
    ["2", 2],
    ["0.5", 0.5],
    ["1", 1],
  ] as const) {
    await zoom.selectOption(preset);
    await expect.poll(() => zoomOf(page)).toBe(value);
    await expect.poll(shown).toBe(`${value * 100}%`);
  }
  await zoom.selectOption("fit");
  await expect.poll(async () => (await nodeLayout(page)).offscreen, { message: "parts cut off" }).toBe(0);
  expect((await names(page)).smallest).toBeGreaterThanOrEqual(11);
});
