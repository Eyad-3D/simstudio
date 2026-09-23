// Screenshot baselines: three key screens (the topology workspace, a parameter
// dialog and the Results page after a run) at three window sizes in both
// themes, 18 images. They catch what the other tests cannot see: dark-mode
// leaks, stray dividers, and clipped or overlapping panels.
//
// Only layout and theme are compared: visual.css hides what the solver
// decides, and the clock, time zone and locale are fixed. The comparison is
// strict (see SHOT), so that a stray divider fails.
//
// The baselines are Linux renders with the fonts Playwright installs
// (`npx playwright install --with-deps chromium`, as CI does), where the UI's
// font stack falls back to DejaVu Sans, so the spec runs on Linux only. After
// a deliberate UI change, regenerate them on Linux and review the new images
// in the pull request:
//   npm run build && npm run test:visual:update
import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";
import { openApp, runActiveCase, runButton } from "./app";

const SIZES = [
  { width: 1280, height: 800 },
  { width: 1600, height: 900 },
  { width: 1920, height: 1080 },
];
const THEMES = ["light", "dark"] as const;
const SHOT = {
  // a pixel counts as changed when its colour moves by more than 5 % (the
  // default, 20 %, misses a border-coloured line on a panel), and more than
  // 0.02 % changed pixels fail: 205 at 1280x800, 415 at 1920x1080, so a
  // 1 px line of that length
  threshold: 0.05,
  maxDiffPixelRatio: 0.0002,
  stylePath: fileURLToPath(new URL("./visual.css", import.meta.url)),
};
const BASELINE_FONT = "DejaVu Sans";

test.skip(process.platform !== "linux", "the screenshot baselines are Linux renders");
test.describe.configure({ mode: "parallel" });
test.use({ locale: "en-US", timezoneId: "UTC" });

/** The font Chromium actually draws the UI's text with. */
async function uiFont(page: Page): Promise<string> {
  await page.evaluate(() => {
    const probe = document.createElement("span");
    probe.id = "font-probe";
    probe.textContent = "Aa";
    document.body.append(probe);
  });
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("DOM.enable");
  await cdp.send("CSS.enable");
  const { root } = await cdp.send("DOM.getDocument");
  const { nodeId } = await cdp.send("DOM.querySelector", { nodeId: root.nodeId, selector: "#font-probe" });
  const { fonts } = await cdp.send("CSS.getPlatformFontsForNode", { nodeId });
  await cdp.detach();
  await page.evaluate(() => document.getElementById("font-probe")?.remove());
  return fonts.map((f) => f.familyName).join(", ");
}

for (const theme of THEMES) {
  for (const viewport of SIZES) {
    const size = `${viewport.width}x${viewport.height}`;
    test.describe(`${theme}, ${size}`, () => {
      test.use({ viewport });

      test(`topology, parameter dialog and Results (${theme}, ${size})`, async ({ page }) => {
        await page.clock.setFixedTime(new Date("2026-01-05T09:30:00Z")); // log and run times
        await page.addInitScript((t) => localStorage.setItem("simstudio-theme", t), theme);
        await openApp(page);
        const font = await uiFont(page);
        // every glyph would differ; say why instead of failing 18 images
        test.skip(font !== BASELINE_FONT && !process.env.CI, `the UI renders in ${font}, not ${BASELINE_FONT}`);
        expect(font, `the baselines use ${BASELINE_FONT}; install Playwright's fonts`).toBe(BASELINE_FONT);
        await expect(page.getByText("backend connected")).toBeVisible();
        await expect(page).toHaveScreenshot(`topology-${theme}-${size}.png`, SHOT);

        // selected first, so Properties shows it however the double-click lands
        const motor = page.locator(".react-flow__node", { hasText: "E-Motor" }).first();
        await motor.click();
        await expect(motor).toHaveClass(/\bselected\b/);
        await motor.dblclick();
        const close = page.getByTitle("Close (Esc)");
        await expect(close).toBeVisible();
        await expect(page).toHaveScreenshot(`parameter-dialog-${theme}-${size}.png`, SHOT);
        await close.click();

        await runActiveCase(page);
        await expect(page.getByText("1 stored run", { exact: true })).toBeVisible();
        await expect(runButton(page)).toBeEnabled(); // the run has finished
        await expect(page).toHaveScreenshot(`results-${theme}-${size}.png`, SHOT);
      });
    });
  }
}
