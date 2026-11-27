// Calm pages: the Start page's example cards fold their descriptions to two
// lines (More / Less, one card at a time, by keyboard too) and so does the
// Open menu (C); a battery's presets are one small control that shows what a
// preset sets before Apply (E).
import { expect, test, type Locator, type Page } from "@playwright/test";
import { openApp, openParams } from "./app";

/** How many lines of text `l` shows. */
const lines = (l: Locator) =>
  l.evaluate((el) => Math.round(el.getBoundingClientRect().height / parseFloat(getComputedStyle(el).lineHeight)));
const startHeading = (page: Page) => page.getByRole("heading", { name: "Start", exact: true });

test("C: an example card shows two lines of its description; More opens the rest in place, Less folds it, one at a time", async ({ page }) => {
  await page.goto("/");
  await expect(startHeading(page)).toBeVisible();
  const card = page.getByRole("button", { name: /^Battery Electric Car, 24 parts · opens as a copy$/ });
  const text = page.locator("#start-example-bev-car-text");
  await expect(card).toHaveAccessibleDescription(/What to expect/); // folded, not cut: all of it is still there
  expect(await lines(text)).toBe(2);
  const more = page.getByRole("button", { name: "More about Battery Electric Car" });
  await more.click();
  expect(await lines(text)).toBeGreaterThan(2);
  await expect(page.getByRole("button", { name: "Less about Battery Electric Car" })).toHaveAttribute("aria-expanded", "true");
  // another card's More folds this one: one at a time
  await page.getByRole("button", { name: "More about P2 Hybrid Car" }).click();
  expect(await lines(text)).toBe(2);
  // by keyboard: from the card, Tab reaches its More, Enter opens and closes it
  await page.getByRole("button", { name: "Less about P2 Hybrid Car" }).click();
  await card.focus();
  await page.keyboard.press("Tab");
  await expect(page.getByRole("button", { name: "More about Battery Electric Car" })).toBeFocused();
  await page.keyboard.press("Enter");
  expect(await lines(text)).toBeGreaterThan(2);
  await page.keyboard.press("Enter");
  expect(await lines(text)).toBe(2);
  // the card itself still opens the example
  await card.click();
  await expect(startHeading(page)).toHaveCount(0);
});

test("C: the Open menu shows the same two lines of each description", async ({ page }) => {
  await openApp(page);
  await page.getByRole("button", { name: "Open", exact: true }).click();
  const entry = page.getByRole("group", { name: "Examples" }).getByRole("menuitem", { name: /^Battery Electric Car/ }).first();
  const text = entry.locator("p");
  expect(await lines(text)).toBe(2);
  expect(await text.getAttribute("title")).toMatch(/What to expect/);
});

test("E: a road car's battery shows one small Presets control; a preset shows what it sets, and only Apply sets it", async ({ page }) => {
  await openApp(page);
  const win = await openParams(page, "HV Battery Pack");
  const presets = win.getByRole("button", { name: "Presets", exact: true });
  await expect(presets).toBeVisible();
  // small, where the wide button was, and no paragraph of rule numbers
  expect((await presets.boundingBox())!.width).toBeLessThan(120);
  await expect(win.getByText(/FS Rules 2026/)).toHaveCount(0);
  const limit = win.getByRole("spinbutton", { name: "Output Power Limit (0 = none)" });
  const before = await limit.inputValue();
  await presets.click();
  await win.getByRole("button", { name: "Formula Student Electric", exact: true }).click();
  const chosen = win.getByRole("group", { name: "Preset: Formula Student Electric" });
  await expect(chosen).toContainText("Output Power Limit (0 = none): 80 kW");
  await expect(chosen).toContainText(/FS Rules 2026 v1\.1 \(FSG\)/);
  // Cancel changes nothing; Esc closes it too
  await chosen.getByRole("button", { name: "Cancel" }).click();
  await expect(chosen).toHaveCount(0);
  await expect(limit).toHaveValue(before);
  await presets.click();
  await win.keyboard.press("Escape");
  await expect(win.getByRole("group", { name: "Presets" })).toHaveCount(0);
  await expect(presets).toBeFocused();
  // Apply sets the values
  await presets.click();
  await win.getByRole("button", { name: "Formula Student Electric", exact: true }).click();
  await win.getByRole("button", { name: "Apply", exact: true }).click();
  await expect(limit).toHaveValue("80");
});
