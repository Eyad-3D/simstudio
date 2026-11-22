// Errors that name the part and the parameter, and take the user there: a
// model the engine refuses to read lists each refused value in Problems, and
// a click on a row (a refused value, or a Data Check about one parameter)
// opens the part's window at that parameter, marked.
import { expect, test, type Page } from "@playwright/test";
import { openApp, paramWindow, runButton, showPanel } from "./app";

const DRAFT = "lightsim-draft-v1";

/** Give parts (by label) these values in the open draft and reload: the
 *  draft is restored as it was, as after a crash. */
async function withValues(page: Page, values: Record<string, Record<string, unknown>>): Promise<void> {
  await expect.poll(() => page.evaluate((key) => localStorage.getItem(key) !== null, DRAFT)).toBe(true);
  await page.evaluate(
    ({ key, values }) => {
      const d = JSON.parse(localStorage.getItem(key)!);
      for (const s of d.project.systems)
        for (const e of s.elements) Object.assign(e.parameterOverrides, values[e.label] ?? {});
      d.clean = false;
      localStorage.setItem(key, JSON.stringify(d));
    },
    { key: DRAFT, values },
  );
  await page.reload();
  await expect(page.locator(".react-flow__node").first()).toBeVisible();
}

/** The Problems row with this text. */
const row = (page: Page, text: string) => page.locator("li button", { hasText: text });

test("a refused model: each value it was refused for is a row, and the row opens the part at it", async ({ page }) => {
  await openApp(page);
  await withValues(page, { Vehicle: { mass_kg: null } });
  await showPanel(page, "Problems");
  await expect(page.getByText("The engine refused the model: 1 problem")).toBeVisible();
  await expect(row(page, "Vehicle Mass: must be a number (got nothing)")).toBeVisible();
  await expect(page.getByText(/0 errors|Not checked/)).toHaveCount(0);

  // Run does not start it: one line in Messages says where the problems are
  await runButton(page).click();
  await showPanel(page, "Messages");
  await expect(page.getByText("The engine refused the model: 1 problem, listed in Problems.")).toBeVisible();
  await expect(page.getByText(/schema error|\[object Object\]/)).toHaveCount(0);

  await showPanel(page, "Problems");
  const win = await paramWindow(page, () => row(page, "Vehicle Mass").click());
  const mass = win.locator('[data-param="mass_kg"]');
  await expect(mass).toHaveClass(/ss-param-mark/);
  // the field shows that it has no value, not the library's
  await expect(mass.getByRole("spinbutton")).toHaveValue("");

  // a value typed there: the model is read again, and the row goes
  await mass.getByRole("spinbutton").fill("1800");
  await expect(page.getByText("The engine refused the model")).toHaveCount(0);
});

test("a Data Check about one parameter opens the part's window at it", async ({ page }) => {
  await openApp(page);
  await withValues(page, { "HV Battery Pack": { capacity_kWh: -5 } });
  await showPanel(page, "Problems");
  const check = row(page, "Usable Capacity of 'HV Battery Pack' must be above 0 kWh");
  await expect(check).toBeVisible();
  const win = await paramWindow(page, () => check.click());
  await expect(win.locator('[data-param="capacity_kWh"]')).toHaveClass(/ss-param-mark/);
});
