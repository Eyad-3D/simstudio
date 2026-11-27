// fix-canvas2: the run's finish notice lies over no control of the tray or
// the status bar, so a real click on the tray's "Collapse to tabs" folds the
// tray while the notice is up; and a duplicated part lands in free room.
// Each wait has its own timeout, so a test fails instead of hanging.
import { expect, test } from "@playwright/test";
import { finishNotice } from "./app";
import { openApp, trayAndDock } from "./ui-helpers";

type Box = { x: number; y: number; width: number; height: number };
const overlap = (a: Box, b: Box) =>
  a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

for (const [width, height] of [
  [1366, 768],
  [1920, 1080],
] as const) {
  test(`the finish notice covers no tray or status-bar control, and Collapse to tabs takes a real click (${width}×${height})`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await openApp(page);
    // the tray open on the Signal Plot, as the user opens it (a run opens no
    // panel since choice B): the notice must keep off its controls
    await page.locator(".dv-tab", { hasText: "Signal Plot" }).first().click();
    await expect.poll(async () => (await trayAndDock(page)).tray, { timeout: 5000 }).toBeGreaterThanOrEqual(120);
    const pane = page.locator(".react-flow__pane").first();
    await pane.click({ position: { x: 5, y: (await pane.boundingBox())!.height - 5 } });
    await page.keyboard.press("Control+Enter");
    const notice = finishNotice(page);
    await expect(notice).toBeVisible({ timeout: 60_000 });
    expect((await trayAndDock(page)).tray, "the tray stays open").toBeGreaterThanOrEqual(120);
    await page.waitForTimeout(1200); // the notice is placed once drawn, then each second
    const n = (await notice.boundingBox())!;
    const controls = await page.evaluate(() => {
      const notice = document.querySelector("[aria-label='Run finished']")!;
      // the tray's controls, and the status bar's (the last 26 px of the window)
      const tray = [...document.querySelectorAll(".dv-edge-group button, .dv-edge-group [role=tab], .dv-edge-group input, .dv-edge-group select")];
      const bar = [...document.querySelectorAll("button, [role=button]")].filter((e) => e.getBoundingClientRect().top >= innerHeight - 26);
      return [...tray, ...bar]
        .filter((e) => !notice.contains(e))
        .map((e) => e.getBoundingClientRect().toJSON() as { x: number; y: number; width: number; height: number })
        .filter((r) => r.width > 0 && r.height > 0);
    });
    for (const c of controls) expect(overlap(n, c), `the notice over a control at ${JSON.stringify(c)}`).toBe(false);
    // a real pointer click on the tray's button, the notice still up
    await page.locator(".dv-edge-group button[aria-label='Collapse to tabs']").first().click({ timeout: 5000 });
    await expect.poll(async () => (await trayAndDock(page)).tray, { timeout: 5000 }).toBeLessThanOrEqual(36);
    await expect(notice).toBeVisible();
  });
}

test("a duplicated part lands in free room, on no other part", async ({ page }) => {
  await openApp(page);
  await page.locator(".react-flow__node", { hasText: "E-Motor" }).first().click();
  await page.keyboard.press("Control+d");
  const added = page.locator(".react-flow__node.selected");
  await expect(added).toHaveCount(1, { timeout: 5000 });
  const a = (await added.locator("[data-node-box]").boundingBox())!;
  const boxes = await page.locator(".react-flow__node:not(.selected) [data-node-box]").evaluateAll((els) =>
    els.map((e) => e.getBoundingClientRect().toJSON() as { x: number; y: number; width: number; height: number }),
  );
  for (const b of boxes) expect(overlap(a, b), "the copy on another part").toBe(false);
});

for (const scale of ["1.1", "1.25", "1.5"])
  test(`the finish notice is wholly inside the window at ${Math.round(Number(scale) * 100)} % interface size`, async ({ page }) => {
    await page.addInitScript((s) => localStorage.setItem("lightsim-font-scale", s), scale);
    await page.setViewportSize({ width: 1366, height: 768 });
    await openApp(page);
    const pane = page.locator(".react-flow__pane").first();
    await pane.click({ position: { x: 5, y: (await pane.boundingBox())!.height - 5 } });
    await page.keyboard.press("Control+Enter");
    const notice = finishNotice(page);
    await expect(notice).toBeVisible({ timeout: 60_000 });
    await page.waitForTimeout(1200); // placed once drawn, then each second
    const n = (await notice.boundingBox())!;
    expect(n.x).toBeGreaterThanOrEqual(0);
    expect(n.y).toBeGreaterThanOrEqual(0);
    expect(n.x + n.width).toBeLessThanOrEqual(1366);
    expect(n.y + n.height).toBeLessThanOrEqual(768);
    // and its button takes a real click
    await notice.getByRole("button", { name: "Show results" }).click({ timeout: 5000 });
    await expect(page.getByPlaceholder("Search channels…")).toBeVisible();
  });
