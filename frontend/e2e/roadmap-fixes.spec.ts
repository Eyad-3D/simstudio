// One test per known P0 bug, written against the fixed behaviour. Each is
// test.fixme (skipped, listed as "fixme" in the report) until its fix lands:
// the fix's pull request switches it to test() so the bug cannot come back.
// Every fixme test fails on today's code; the roadmap id is in the title.
// The one plain test() here (UX-02's status-bar New) passes before and after
// its fix.
import { expect, test, type Page } from "@playwright/test";
import {
  dragComponent,
  drawnLines,
  expectProject,
  openApp,
  openFromMenu,
  ribbonTab,
  runActiveCase,
  selectElement,
  showPanel,
} from "./app";

test("RES-03: the Signal Plot draws a line after the first run", async ({ page }) => {
  await openApp(page);
  await runActiveCase(page);
  await ribbonTab(page, "Home").click();
  await showPanel(page, "Signal Plot");
  // the Results page is unmounted on Home, so this is the Signal Plot's chart
  await expect(drawnLines(page).first()).toBeVisible();
});

test("RES-03: a run started from the empty Results page is drawn", async ({ page }) => {
  await openApp(page);
  await ribbonTab(page, "Results").click();
  await page.getByRole("button", { name: "Run active case" }).click();
  await expect(page.getByText("1 stored run", { exact: true })).toBeVisible({ timeout: 60_000 });
  await expect(drawnLines(page).first()).toBeVisible();
});

test("RES-04: no hidden splitter lies on top of the Results page", async ({ page }) => {
  await openApp(page);
  await ribbonTab(page, "Results").click();
  for (const width of [1280, 1600, 1920]) {
    await page.setViewportSize({ width, height: 900 });
    await page.waitForTimeout(200); // let the dock re-layout
    const topmost = await page.evaluate(() =>
      [...document.querySelectorAll(".dv-sash")].filter((sash) => {
        const r = sash.getBoundingClientRect();
        if (r.width === 0 || r.height === 0) return false;
        const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
        return Boolean(hit?.closest(".dv-sash"));
      }).length,
    );
    expect(topmost, `splitters on top of Results at ${width} px`).toBe(0);
  }
});

test("UX-05: Esc while editing a map cell keeps the parameter dialog open", async ({ page }) => {
  await openApp(page);
  await page.locator(".react-flow__node", { hasText: "E-Motor" }).first().dblclick();
  const close = page.getByTitle("Close (Esc)");
  await expect(close).toBeVisible();

  const cell = page.locator(".ss-grid .ss-cell-body").first();
  const before = await cell.innerText();
  await cell.dblclick();
  await expect(page.locator(".ss-cell-input")).toBeVisible();
  await page.keyboard.press("Escape");

  await expect(page.locator(".ss-cell-input")).toHaveCount(0); // the edit is cancelled…
  await expect(close).toBeVisible(); // …but the dialog stays
  await expect(page.locator(".ss-grid .ss-cell-body").first()).toHaveText(before);
});

test("UX-04: a number field can be cleared and retyped", async ({ page }) => {
  await openApp(page);
  await selectElement(page, "Vehicle");
  const mass = page.locator("tr", { hasText: "Vehicle Mass" }).locator("input");
  await expect(mass).toHaveValue("1800");
  await mass.fill("");
  await expect(mass).toHaveValue(""); // today: snaps to "0"
  await mass.pressSequentially("1.2");
  await expect(mass).toHaveValue("1.2"); // today: "01.2"
});

/** The share of the window the diagram gets, and its zoom. */
async function diagramView(page: Page): Promise<{ share: number; zoom: number }> {
  return page.evaluate(() => {
    const box = document.querySelector(".react-flow")!.getBoundingClientRect();
    const viewport = document.querySelector(".react-flow__viewport")!;
    return {
      share: (box.width * box.height) / (window.innerWidth * window.innerHeight),
      zoom: new DOMMatrixReadOnly(getComputedStyle(viewport).transform).a,
    };
  });
}

// GUI-01's target. Today the diagram gets 9.8 % of a 1366x768 window at a fit
// zoom of 0.16, and 28.7 % of a 1920x1080 one.
for (const { width, height, percent, zoom } of [
  { width: 1366, height: 768, percent: 45, zoom: 0.4 },
  { width: 1920, height: 1080, percent: 55, zoom: 0 },
]) {
  test.describe(`at ${width}x${height}`, () => {
    test.use({ viewport: { width, height } });

    test(`GUI-01: the diagram gets at least ${percent} % of a ${width}x${height} window`, async ({
      page,
    }) => {
      await openApp(page);
      await expect.poll(async () => (await diagramView(page)).share * 100).toBeGreaterThanOrEqual(percent);
      if (zoom) await expect.poll(async () => (await diagramView(page)).zoom).toBeGreaterThanOrEqual(zoom);
    });
  });
}

/** The smallest node name on screen, in px: font size times every scale
 *  applied to it (the diagram's zoom included). */
async function smallestNodeName(page: Page, names: string[]): Promise<number> {
  return page.evaluate((names) => {
    const sizes = [...document.querySelectorAll(".react-flow__node *")]
      .filter((el) => el.children.length === 0 && names.includes(el.textContent!.trim()))
      .map((el) => {
        const scale = el.getBoundingClientRect().height / (el as HTMLElement).offsetHeight;
        return parseFloat(getComputedStyle(el).fontSize) * scale;
      });
    return sizes.length ? Math.min(...sizes) : NaN;
  }, names);
}

// GUI-03's target. Today names are 3.5 px on screen when the example is
// fitted to a 1600x900 window (1.8 px at 1366x768).
test.fixme("GUI-03: node names are at least 11 px on screen, fitted and zoomed out", async ({ page }) => {
  await openApp(page);
  const project = await (await page.request.get("/api/projects/bev-car")).json();
  const names: string[] = project.systems.flatMap((s: { elements: { label: string }[] }) =>
    s.elements.map((e) => e.label),
  );
  await expect.poll(() => smallestNodeName(page, names)).toBeGreaterThanOrEqual(11);
  for (let i = 0; i < 3; i++) await page.getByTitle("Zoom out").click();
  await expect.poll(() => smallestNodeName(page, names)).toBeGreaterThanOrEqual(11);
});

test.describe("UX-02: replacing a project with unsaved changes asks first", () => {
  test.beforeEach(async ({ page }) => {
    await openApp(page);
    await dragComponent(page, "Constant", { x: 60, y: 60 });
    await expectProject(page, "Battery Electric Car", { unsaved: true });
  });

  /** The prompt is up, the edited model is still there, and Cancel keeps it. */
  async function expectPromptThenCancel(page: Page) {
    await expect(page.getByRole("button", { name: "Cancel", exact: true })).toBeVisible();
    await expectProject(page, "Battery Electric Car", { unsaved: true });
    await page.getByRole("button", { name: "Cancel", exact: true }).click();
    await expectProject(page, "Battery Electric Car", { unsaved: true });
    await expect(page.locator(".react-flow__node", { hasText: "Constant 1" })).toHaveCount(1);
  }

  // The status-bar "+" asks already ("Discard unsaved changes?": Cancel or
  // New project); the UX-02 fix makes it Save / Don't save / Cancel like the
  // rest. This test holds for both prompts, so it runs before and after.
  test("UX-02: the status-bar New asks, and its discard choice replaces the project", async ({
    page,
  }) => {
    const plus = page.getByRole("button", { name: "New project", exact: true });
    await plus.click();
    await expectPromptThenCancel(page);
    await plus.click();
    // "New project" in today's prompt, "Don't save" in UX-02's; the dialog
    // comes after the status bar in the page
    await page.getByRole("button", { name: /^(New project|Don't save)$/ }).last().click();
    await expectProject(page, "New Project", { unsaved: false });
    await expect(page.locator(".react-flow__node")).toHaveCount(0);
  });

  test("UX-02: ribbon New", async ({ page }) => {
    await page.getByRole("button", { name: "New", exact: true }).click();
    await expectPromptThenCancel(page);
  });

  test("UX-02: Open another project", async ({ page }) => {
    await openFromMenu(page, "P2 Hybrid Car");
    await expectPromptThenCancel(page);
  });

  test("UX-02: Import a project file", async ({ page }) => {
    // answer the file picker if it opens before the prompt
    page.on("filechooser", (chooser) =>
      chooser.setFiles({
        name: "other.json",
        mimeType: "application/json",
        buffer: Buffer.from(
          JSON.stringify({
            id: "other",
            name: "Other",
            systems: [{ id: "sys", name: "Other", parentId: null, elements: [], connections: [] }],
            dataBusConnections: [],
            cases: [{ id: "case", name: "Case 1", duration: 10, timeStep: 1 }],
          }),
        ),
      }),
    );
    await page.getByRole("button", { name: "Import", exact: true }).click();
    await expectPromptThenCancel(page);
  });
});
